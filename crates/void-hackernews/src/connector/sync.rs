use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{HnClient, HnItem};

const HN_BASE: &str = "https://news.ycombinator.com/item?id=";

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    keywords: &[String],
    min_score: u32,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = HnClient::new();

    ensure_feed_conversation(db, connection_id)?;

    info!(connection_id, "running initial HN sync");
    if let Err(e) = poll_stories(
        &client,
        db,
        connection_id,
        keywords,
        min_score,
        &cancel,
        true,
    )
    .await
    {
        error!(connection_id, error = %e, "initial HN sync failed");
    }

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(poll_interval_secs));
    // First tick fires immediately; skip it since we just did initial sync.
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "HN sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "HN sync was idle, catching up"
                    );
                    void_core::status!(
                        "[hackernews:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling Hacker News");
                }
                if let Err(e) = poll_stories(
                    &client,
                    db,
                    connection_id,
                    keywords,
                    min_score,
                    &cancel,
                    elapsed > IDLE_THRESHOLD,
                )
                .await
                {
                    error!(connection_id, error = %e, "HN poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

fn ensure_feed_conversation(db: &Arc<Database>, connection_id: &str) -> anyhow::Result<()> {
    let conv_external_id = format!("hackernews_{connection_id}_feed");
    let conv = Conversation {
        id: format!("{connection_id}-feed"),
        connection_id: connection_id.to_string(),
        connector: "hackernews".to_string(),
        external_id: conv_external_id,
        name: Some("Hacker News".to_string()),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;
    Ok(())
}

async fn poll_stories(
    client: &HnClient,
    db: &Arc<Database>,
    connection_id: &str,
    keywords: &[String],
    min_score: u32,
    cancel: &CancellationToken,
    show_progress: bool,
) -> anyhow::Result<()> {
    let story_ids = client.top_stories().await.unwrap_or_default();
    let total = story_ids.len() as u64;

    let conv_id = format!("{connection_id}-feed");

    let mut progress = show_progress.then(|| {
        let mut p = BackfillProgress::new(&format!("hackernews:{connection_id}"), "stories")
            .with_secondary("ingested");
        p.set_items_total(total);
        p
    });

    for id in story_ids {
        if cancel.is_cancelled() {
            break;
        }

        let external_id = format!("hackernews_{connection_id}_{id}");
        if db.message_exists(connection_id, &external_id)? {
            if let Some(ref mut p) = progress {
                p.inc(1);
            }
            continue;
        }

        let item = match client.get_item(id).await {
            Ok(Some(item)) => item,
            Ok(None) => {
                if let Some(ref mut p) = progress {
                    p.inc(1);
                }
                continue;
            }
            Err(e) => {
                warn!(id, error = %e, "failed to fetch HN item");
                if let Some(ref mut p) = progress {
                    p.inc(1);
                }
                continue;
            }
        };

        if !matches_filters(&item, keywords, min_score) {
            if let Some(ref mut p) = progress {
                p.inc(1);
            }
            continue;
        }

        let msg = build_message(&item, connection_id, &conv_id);
        db.upsert_message(&msg)?;
        if let Some(ref mut p) = progress {
            p.inc(1);
            p.inc_secondary(1);
        }
    }

    if let Some(p) = progress {
        p.finish();
    }

    if !cancel.is_cancelled() {
        db.set_sync_state(
            connection_id,
            "hn_last_sync",
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }

    Ok(())
}

fn matches_filters(item: &HnItem, keywords: &[String], min_score: u32) -> bool {
    if item.item_type.as_deref() != Some("story") {
        return false;
    }

    let score = item.score.unwrap_or(0);
    if score < min_score {
        return false;
    }

    if keywords.is_empty() {
        return true;
    }

    let title = item.title.as_deref().unwrap_or("").to_lowercase();
    keywords.iter().any(|kw| title.contains(kw.as_str()))
}

fn build_message(item: &HnItem, connection_id: &str, conv_id: &str) -> Message {
    let id = item.id;
    let title = item.title.as_deref().unwrap_or("(untitled)");
    let author = item.by.as_deref().unwrap_or("unknown");
    let score = item.score.unwrap_or(0);
    let url = item.url.as_deref().unwrap_or("").to_string();
    let hn_url = format!("{HN_BASE}{id}");
    let comments = item.descendants.unwrap_or(0);

    let body = if url.is_empty() {
        format!("{title}\n{hn_url}\n{score} points | {comments} comments")
    } else {
        format!("{title}\n{url}\n{hn_url}\n{score} points | {comments} comments")
    };

    let metadata = serde_json::json!({
        "hn_id": id,
        "score": score,
        "url": url,
        "hn_url": hn_url,
        "descendants": comments,
    });

    Message {
        id: format!("{connection_id}-{id}"),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "hackernews".to_string(),
        external_id: format!("hackernews_{connection_id}_{id}"),
        sender: author.to_string(),
        sender_name: Some(author.to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp: item.time.unwrap_or_else(|| chrono::Utc::now().timestamp()),
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(metadata),
        context_id: None,
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(id: u64, title: &str, score: u32) -> HnItem {
        HnItem {
            id,
            title: Some(title.to_string()),
            url: Some("https://example.com".to_string()),
            score: Some(score),
            by: Some("author".to_string()),
            time: Some(1_700_000_000),
            item_type: Some("story".to_string()),
            descendants: Some(10),
        }
    }

    #[test]
    fn matches_keyword_case_insensitive() {
        let item = make_item(1, "Rust is Amazing", 200);
        let keywords = vec!["rust".to_string()];
        assert!(matches_filters(&item, &keywords, 0));
    }

    #[test]
    fn rejects_below_min_score() {
        let item = make_item(1, "Rust is Amazing", 50);
        let keywords = vec!["rust".to_string()];
        assert!(!matches_filters(&item, &keywords, 100));
    }

    #[test]
    fn rejects_non_matching_keyword() {
        let item = make_item(1, "Python is Great", 200);
        let keywords = vec!["rust".to_string()];
        assert!(!matches_filters(&item, &keywords, 0));
    }

    #[test]
    fn empty_keywords_matches_all_stories() {
        let item = make_item(1, "Anything Goes", 200);
        assert!(matches_filters(&item, &[], 0));
    }

    #[test]
    fn rejects_non_story_type() {
        let mut item = make_item(1, "Rust job", 200);
        item.item_type = Some("job".to_string());
        let keywords = vec!["rust".to_string()];
        assert!(!matches_filters(&item, &keywords, 0));
    }

    #[test]
    fn deserialize_realistic_story_json() {
        let json = r#"{
            "id": 8863,
            "title": "My YC app: Dropbox - Throw away your USB drive",
            "url": "http://www.getdropbox.com/u/2/screencast.html",
            "score": 111,
            "by": "dhouston",
            "time": 1175714200,
            "type": "story",
            "descendants": 71
        }"#;
        let item: HnItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.id, 8863);
        assert_eq!(item.item_type.as_deref(), Some("story"));
        assert_eq!(item.score, Some(111));
        // Above min_score and matching keyword -> included.
        let keywords = vec!["dropbox".to_string()];
        assert!(matches_filters(&item, &keywords, 100));
        // Below threshold -> skipped.
        assert!(!matches_filters(&item, &keywords, 200));
        // Non-matching keyword -> skipped.
        assert!(!matches_filters(&item, &["nonsense".to_string()], 0));
        // Message build round-trips the fields.
        let msg = build_message(&item, "hn", "hn-feed");
        assert!(msg.body.as_ref().unwrap().contains("111 points"));
        assert!(msg.body.as_ref().unwrap().contains("71 comments"));
    }

    #[test]
    fn deserialize_job_type_is_skipped() {
        let json = r#"{
            "id": 192327,
            "title": "Justin.tv is looking for a Lead Flash Engineer!",
            "score": 6,
            "by": "justin",
            "time": 1210981217,
            "type": "job"
        }"#;
        let item: HnItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.item_type.as_deref(), Some("job"));
        // Jobs are never matched regardless of keywords/score.
        assert!(!matches_filters(&item, &[], 0));
    }

    #[test]
    fn deserialize_missing_score_defaults_to_zero() {
        let json = r#"{
            "id": 100,
            "title": "Scoreless Story",
            "type": "story",
            "by": "nobody",
            "time": 1700000000
        }"#;
        let item: HnItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.score, None);
        // Missing score treated as 0 -> passes only with min_score 0.
        assert!(matches_filters(&item, &[], 0));
        assert!(!matches_filters(&item, &[], 1));
        // build_message tolerates missing score/descendants.
        let msg = build_message(&item, "hn", "hn-feed");
        assert!(msg.body.as_ref().unwrap().contains("0 points"));
    }

    #[test]
    fn deserialize_null_fields_tolerated() {
        let json = r#"{
            "id": 200,
            "title": null,
            "url": null,
            "score": null,
            "by": null,
            "time": null,
            "type": "story",
            "descendants": null
        }"#;
        let item: HnItem = serde_json::from_str(json).unwrap();
        assert!(item.title.is_none());
        assert!(item.url.is_none());
        assert!(item.score.is_none());
        // No title, empty keywords -> story still matches at score 0.
        assert!(matches_filters(&item, &[], 0));
        // With a keyword and empty title -> no match.
        assert!(!matches_filters(&item, &["rust".to_string()], 0));
        // build_message falls back to placeholders and HN url (no external url).
        let msg = build_message(&item, "hn", "hn-feed");
        let body = msg.body.as_ref().unwrap();
        assert!(body.contains("(untitled)"));
        assert!(body.contains("https://news.ycombinator.com/item?id=200"));
        assert_eq!(msg.sender, "unknown");
    }

    #[test]
    fn deserialize_malformed_json_returns_err_no_panic() {
        // Missing required `id` field.
        let bad = r#"{ "title": "No id here", "type": "story" }"#;
        assert!(serde_json::from_str::<HnItem>(bad).is_err());
        // Wrong type for `id`.
        let bad2 = r#"{ "id": "not-a-number", "type": "story" }"#;
        assert!(serde_json::from_str::<HnItem>(bad2).is_err());
        // Truncated JSON.
        let bad3 = r#"{ "id": 1, "title": "#;
        assert!(serde_json::from_str::<HnItem>(bad3).is_err());
    }

    #[test]
    fn build_message_includes_all_fields() {
        let item = make_item(42, "Show HN: Cool Tool", 350);
        let msg = build_message(&item, "hn", "hn-feed");
        assert_eq!(msg.id, "hn-42");
        assert_eq!(msg.sender, "author");
        assert!(msg.body.as_ref().unwrap().contains("Show HN: Cool Tool"));
        assert!(msg.body.as_ref().unwrap().contains("350 points"));
        assert!(msg.body.as_ref().unwrap().contains("https://example.com"));
        let meta = msg.metadata.unwrap();
        assert_eq!(meta["hn_id"], 42);
        assert_eq!(meta["score"], 350);
    }
}
