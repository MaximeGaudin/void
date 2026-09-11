use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{CirclebackClient, Meeting, TranscriptTurn};
use crate::CONNECTOR_ID;

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);
/// Meetings created before this many days ago are not re-checked on regular
/// polls: Circleback publishes notes within minutes, and action-item edits on
/// old meetings are not worth a full history walk every poll.
const RECHECK_WINDOW_DAYS: i64 = 14;

const STATE_LAST_POLL: &str = "last_poll_at";

pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    client: CirclebackClient,
    backfill_days: u32,
    include_transcript: bool,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    info!(
        connection_id,
        backfill_days, "running initial Circleback sync"
    );
    if let Err(e) = sync_once(
        &client,
        db,
        connection_id,
        backfill_days,
        include_transcript,
        &cancel,
        true,
    )
    .await
    {
        error!(connection_id, error = %e, "initial Circleback sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs.max(60)));
    // First tick fires immediately; skip it since we just did the initial sync.
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "Circleback sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                let catching_up = elapsed > IDLE_THRESHOLD + Duration::from_secs(poll_interval_secs);
                if catching_up {
                    warn!(connection_id, idle_secs = elapsed.as_secs(), "Circleback sync was idle, catching up");
                    void_core::status!(
                        "[circleback:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling Circleback");
                }
                if let Err(e) = sync_once(
                    &client,
                    db,
                    connection_id,
                    backfill_days,
                    include_transcript,
                    &cancel,
                    catching_up,
                )
                .await
                {
                    error!(connection_id, error = %e, "Circleback poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SyncStats {
    pub seen: u64,
    pub imported: u64,
    pub transcript_turns: u64,
}

/// Walk `/meetings` newest-first until the backfill / re-check horizon and
/// import every meeting whose `updatedAt` differs from what we stored.
pub(super) async fn sync_once(
    client: &CirclebackClient,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_days: u32,
    include_transcript: bool,
    cancel: &CancellationToken,
    show_progress: bool,
) -> anyhow::Result<SyncStats> {
    let now = chrono::Utc::now().timestamp();
    let backfill_cutoff = (backfill_days > 0).then(|| now - i64::from(backfill_days) * 86_400);
    let last_poll: Option<i64> = db
        .get_sync_state(connection_id, STATE_LAST_POLL)?
        .and_then(|v| v.parse().ok());
    // After the first run only look back a bounded window for late notes.
    let horizon = match (backfill_cutoff, last_poll) {
        (Some(cut), Some(last)) => Some(cut.max(last - RECHECK_WINDOW_DAYS * 86_400)),
        (Some(cut), None) => Some(cut),
        (None, Some(last)) => Some(last - RECHECK_WINDOW_DAYS * 86_400),
        (None, None) => None,
    };

    let mut progress = show_progress.then(|| {
        BackfillProgress::new(&format!("circleback:{connection_id}"), "meetings")
            .with_secondary("imported")
    });

    let mut stats = SyncStats::default();
    let mut cursor: Option<String> = None;
    'pages: loop {
        if cancel.is_cancelled() {
            break;
        }
        let page = client.list_meetings(cursor.as_deref()).await?;
        if let Some(ref mut p) = progress {
            p.inc_page();
        }
        for meeting in &page.meetings {
            if cancel.is_cancelled() {
                break 'pages;
            }
            let created = parse_ts(&meeting.created_at).unwrap_or(now);
            if horizon.is_some_and(|h| created < h) {
                break 'pages;
            }
            stats.seen += 1;
            if let Some(ref mut p) = progress {
                p.inc(1);
            }
            match import_meeting(client, db, connection_id, meeting, include_transcript).await {
                Ok(Some(turns)) => {
                    stats.imported += 1;
                    stats.transcript_turns += turns;
                    if let Some(ref mut p) = progress {
                        p.inc_secondary(1);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(connection_id, meeting = %meeting.id, error = %e, "failed to import meeting")
                }
            }
        }
        match page.next_cursor {
            Some(next) if !page.meetings.is_empty() => cursor = Some(next),
            _ => break,
        }
    }

    if let Some(p) = progress {
        p.finish();
    }
    db.set_sync_state(connection_id, STATE_LAST_POLL, &now.to_string())?;
    info!(
        connection_id,
        seen = stats.seen,
        imported = stats.imported,
        turns = stats.transcript_turns,
        "Circleback sync done"
    );
    Ok(stats)
}

/// Import one meeting. Returns `Some(transcript turns written)` when the
/// meeting was (re)written, `None` when it was up to date or not processed yet.
async fn import_meeting(
    client: &CirclebackClient,
    db: &Arc<Database>,
    connection_id: &str,
    meeting: &Meeting,
    include_transcript: bool,
) -> anyhow::Result<Option<u64>> {
    if !meeting.is_processed() {
        // Recording still being processed: come back on a later poll.
        return Ok(None);
    }
    let version_key = format!("meeting:{}", meeting.id);
    if db.get_sync_state(connection_id, &version_key)?.as_deref() == Some(meeting.version()) {
        return Ok(None);
    }

    let conv = build_conversation(meeting, connection_id);
    db.upsert_conversation(&conv)?;
    db.upsert_message(&build_notes_message(meeting, connection_id, &conv.id))?;
    if let Some(actions) = build_action_items_message(meeting, connection_id, &conv.id) {
        db.upsert_message(&actions)?;
    }

    let mut turns_written = 0u64;
    let transcript_key = format!("transcript:{}", meeting.id);
    if include_transcript && db.get_sync_state(connection_id, &transcript_key)?.is_none() {
        let turns = client.transcript(&meeting.id).await?;
        for msg in build_transcript_messages(meeting, &turns, connection_id, &conv.id) {
            db.upsert_message(&msg)?;
            turns_written += 1;
        }
        db.set_sync_state(connection_id, &transcript_key, &turns.len().to_string())?;
    }

    db.set_sync_state(connection_id, &version_key, meeting.version())?;
    Ok(Some(turns_written))
}

fn parse_ts(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|d| d.timestamp())
}

fn meeting_start(meeting: &Meeting) -> i64 {
    parse_ts(&meeting.created_at).unwrap_or_else(|| chrono::Utc::now().timestamp())
}

fn meeting_end(meeting: &Meeting) -> i64 {
    meeting_start(meeting) + meeting.duration.unwrap_or(0.0).round() as i64
}

fn meeting_title(meeting: &Meeting) -> String {
    meeting
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("Untitled meeting")
        .to_string()
}

fn attendee_names(meeting: &Meeting) -> Vec<String> {
    meeting
        .attendees
        .iter()
        .filter_map(|a| {
            a.name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(String::from)
                .or_else(|| a.email.clone())
        })
        .collect()
}

fn attendees_json(meeting: &Meeting) -> serde_json::Value {
    serde_json::Value::Array(
        meeting
            .attendees
            .iter()
            .map(|a| {
                serde_json::json!({
                    "name": a.name,
                    "email": a.email,
                    "title": a.title,
                    "company": a.company_name,
                    "organizer": a.is_calendar_event_organizer.unwrap_or(false),
                })
            })
            .collect(),
    )
}

fn base_message(meeting: &Meeting, connection_id: &str, conv_id: &str, suffix: &str) -> Message {
    let now = chrono::Utc::now().timestamp();
    Message {
        id: format!("{connection_id}-{}-{suffix}", meeting.id),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id: format!("{CONNECTOR_ID}_{connection_id}_{}_{suffix}", meeting.id),
        sender: "circleback".to_string(),
        sender_name: Some("Circleback".to_string()),
        sender_avatar_url: None,
        body: None,
        timestamp: meeting_end(meeting),
        synced_at: Some(now),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        // Every message of a meeting shares one context: the inbox shows one
        // row per meeting, `void messages` shows notes + transcript together.
        context_id: Some(meeting.id.clone()),
        context: None,
    }
}

pub fn build_conversation(meeting: &Meeting, connection_id: &str) -> Conversation {
    Conversation {
        id: format!("{connection_id}-{}", meeting.id),
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id: meeting.id.clone(),
        name: Some(meeting_title(meeting)),
        kind: ConversationKind::Group,
        last_message_at: Some(meeting_end(meeting)),
        unread_count: 0,
        is_muted: false,
        metadata: Some(serde_json::json!({
            "meeting_id": meeting.id,
            "url": meeting.url,
            "started_at": meeting.created_at,
            "duration_secs": meeting.duration,
            "attendees": attendees_json(meeting),
            "ical_uid": meeting.ical_uid,
            "recording_url": meeting.recording_url,
            "tags": meeting.tags,
            "calendar_event": meeting.calendar_event,
        })),
    }
}

/// The AI notes: one message per meeting, timestamped at the end of the
/// meeting so it is the newest item of the context (the inbox representative).
pub fn build_notes_message(meeting: &Meeting, connection_id: &str, conv_id: &str) -> Message {
    let title = meeting_title(meeting);
    let minutes = meeting.duration.map(|d| (d / 60.0).round() as i64);
    let mut header = vec![title.clone()];
    let mut facts = Vec::new();
    if let Some(m) = minutes {
        facts.push(format!("{m} min"));
    }
    let names = attendee_names(meeting);
    if !names.is_empty() {
        facts.push(names.join(", "));
    }
    if !facts.is_empty() {
        header.push(facts.join(" · "));
    }
    if let Some(url) = meeting.url.as_deref().filter(|u| !u.is_empty()) {
        header.push(url.to_string());
    }
    let notes = meeting
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or("(no notes)");
    let body = format!("{}\n\n{notes}", header.join("\n"));

    let mut msg = base_message(meeting, connection_id, conv_id, "notes");
    msg.timestamp = meeting_end(meeting) + 2;
    msg.body = Some(body);
    msg.metadata = Some(serde_json::json!({
        "kind": "notes",
        "meeting_id": meeting.id,
        "title": title,
        "url": meeting.url,
        "started_at": meeting.created_at,
        "duration_secs": meeting.duration,
        "attendees": attendees_json(meeting),
        "action_items": meeting.action_items.len(),
        "recording_url": meeting.recording_url,
    }));
    msg
}

/// Action items as a checklist; `None` when the meeting has none.
pub fn build_action_items_message(
    meeting: &Meeting,
    connection_id: &str,
    conv_id: &str,
) -> Option<Message> {
    if meeting.action_items.is_empty() {
        return None;
    }
    let mut lines = vec![format!("Action items ({})", meeting.action_items.len())];
    let mut items = Vec::new();
    for item in &meeting.action_items {
        let title = item
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("(untitled)");
        let who = item
            .assignee
            .as_ref()
            .and_then(|a| a.name.clone().or_else(|| a.email.clone()));
        let mut line = format!("- [{}] {title}", if item.is_done() { "x" } else { " " });
        if let Some(who) = &who {
            line.push_str(&format!(" — {who}"));
        }
        lines.push(line);
        items.push(serde_json::json!({
            "id": item.id,
            "title": title,
            "description": item.description,
            "status": item.status,
            "done": item.is_done(),
            "assignee": who,
            "assignee_email": item.assignee.as_ref().and_then(|a| a.email.clone()),
        }));
    }
    let mut msg = base_message(meeting, connection_id, conv_id, "actions");
    msg.timestamp = meeting_end(meeting) + 1;
    msg.body = Some(lines.join("\n"));
    msg.metadata = Some(serde_json::json!({
        "kind": "action_items",
        "meeting_id": meeting.id,
        "items": items,
    }));
    Some(msg)
}

/// One message per speaker turn, timestamped from the meeting start.
pub fn build_transcript_messages(
    meeting: &Meeting,
    turns: &[TranscriptTurn],
    connection_id: &str,
    conv_id: &str,
) -> Vec<Message> {
    let start = meeting_start(meeting);
    turns
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.text.trim().is_empty())
        .map(|(i, t)| {
            let offset = t.timestamp.unwrap_or(0.0).max(0.0);
            let speaker = t
                .speaker
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Unknown speaker");
            let mut msg = base_message(meeting, connection_id, conv_id, &format!("t{i}"));
            msg.sender = speaker.to_string();
            msg.sender_name = Some(speaker.to_string());
            msg.body = Some(t.text.trim().to_string());
            msg.timestamp = start + offset.floor() as i64;
            msg.metadata = Some(serde_json::json!({
                "kind": "transcript",
                "meeting_id": meeting.id,
                "turn": i,
                "offset_secs": offset,
            }));
            msg
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ActionItem, Assignee, Attendee};

    fn meeting(notes: Option<&str>, duration: Option<f64>) -> Meeting {
        Meeting {
            id: "m1".into(),
            name: Some("Daily Model".into()),
            url: Some("https://meet.google.com/abc".into()),
            created_at: "2026-09-10T14:00:00Z".into(),
            updated_at: Some("2026-09-10T15:00:00Z".into()),
            duration,
            notes: notes.map(String::from),
            private_notes: None,
            ical_uid: None,
            recording_url: None,
            tags: vec![],
            attendees: vec![
                Attendee {
                    profile_id: Some(1),
                    name: Some("Ada".into()),
                    email: Some("ada@example.com".into()),
                    title: None,
                    company_name: None,
                    is_calendar_invitee: Some(true),
                    is_calendar_event_organizer: Some(true),
                },
                Attendee {
                    profile_id: None,
                    name: None,
                    email: Some("bob@example.com".into()),
                    title: None,
                    company_name: None,
                    is_calendar_invitee: None,
                    is_calendar_event_organizer: None,
                },
            ],
            action_items: vec![ActionItem {
                id: Some(7),
                title: Some("Ship it".into()),
                description: None,
                status: Some("PENDING".into()),
                completed_at: None,
                assignee: Some(Assignee {
                    profile_id: None,
                    name: Some("Bob".into()),
                    email: None,
                }),
            }],
            calendar_event: None,
        }
    }

    #[test]
    fn conversation_is_one_group_per_meeting() {
        let conv = build_conversation(&meeting(Some("n"), Some(600.0)), "cb");
        assert_eq!(conv.id, "cb-m1");
        assert_eq!(conv.external_id, "m1");
        assert_eq!(conv.name.as_deref(), Some("Daily Model"));
        assert_eq!(conv.kind, ConversationKind::Group);
        // ends 10 minutes after createdAt
        assert_eq!(conv.last_message_at, Some(1_789_048_800 + 600));
    }

    #[test]
    fn notes_message_carries_header_and_notes() {
        let msg = build_notes_message(
            &meeting(Some("#### Overview\n* thing"), Some(600.0)),
            "cb",
            "cb-m1",
        );
        let body = msg.body.unwrap();
        assert!(body.starts_with("Daily Model\n10 min · Ada, bob@example.com\nhttps://meet.google.com/abc\n\n#### Overview"));
        assert_eq!(msg.external_id, "circleback_cb_m1_notes");
        assert_eq!(msg.context_id.as_deref(), Some("m1"));
        assert_eq!(msg.timestamp, 1_789_048_800 + 600 + 2);
        assert_eq!(msg.metadata.unwrap()["action_items"], 1);
    }

    #[test]
    fn action_items_render_as_checklist() {
        let msg =
            build_action_items_message(&meeting(Some("n"), Some(60.0)), "cb", "cb-m1").unwrap();
        assert_eq!(
            msg.body.as_deref(),
            Some("Action items (1)\n- [ ] Ship it — Bob")
        );
        assert_eq!(msg.timestamp, 1_789_048_800 + 60 + 1);
        let mut m = meeting(Some("n"), Some(60.0));
        m.action_items.clear();
        assert!(build_action_items_message(&m, "cb", "cb-m1").is_none());
    }

    #[test]
    fn transcript_turns_become_speaker_messages() {
        let turns = vec![
            TranscriptTurn {
                speaker: Some("Ada".into()),
                text: "Hello".into(),
                timestamp: Some(2.4),
            },
            TranscriptTurn {
                speaker: None,
                text: "   ".into(),
                timestamp: Some(3.0),
            },
            TranscriptTurn {
                speaker: None,
                text: "Hi".into(),
                timestamp: None,
            },
        ];
        let msgs =
            build_transcript_messages(&meeting(Some("n"), Some(60.0)), &turns, "cb", "cb-m1");
        assert_eq!(msgs.len(), 2, "blank turns are dropped");
        assert_eq!(msgs[0].sender, "Ada");
        assert_eq!(msgs[0].timestamp, 1_789_048_800 + 2);
        assert_eq!(msgs[0].external_id, "circleback_cb_m1_t0");
        assert_eq!(msgs[1].sender, "Unknown speaker");
        assert_eq!(msgs[1].external_id, "circleback_cb_m1_t2");
        assert!(msgs.iter().all(|m| m.context_id.as_deref() == Some("m1")));
    }

    #[test]
    fn unprocessed_meeting_is_detected() {
        assert!(!meeting(None, None).is_processed());
        assert!(meeting(None, Some(1.0)).is_processed());
    }
}
