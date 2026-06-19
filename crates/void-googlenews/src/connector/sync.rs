use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{sanitize_id, strip_html, GoogleNewsClient, RssItem};

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack/HN).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    keywords: &[String],
    when: &str,
    language: &str,
    country: &str,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = GoogleNewsClient::new();

    ensure_feed_conversation(db, connection_id)?;

    info!(connection_id, "running initial Google News sync");
    if let Err(e) = poll_keywords(
        &client,
        db,
        connection_id,
        keywords,
        when,
        language,
        country,
        &cancel,
        true,
    )
    .await
    {
        error!(connection_id, error = %e, "initial Google News sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    // First tick fires immediately; skip it since we just did the initial sync.
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "Google News sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "Google News sync was idle, catching up"
                    );
                    void_core::status!(
                        "[googlenews:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling Google News");
                }
                if let Err(e) = poll_keywords(
                    &client,
                    db,
                    connection_id,
                    keywords,
                    when,
                    language,
                    country,
                    &cancel,
                    elapsed > IDLE_THRESHOLD,
                )
                .await
                {
                    error!(connection_id, error = %e, "Google News poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

fn ensure_feed_conversation(db: &Arc<Database>, connection_id: &str) -> anyhow::Result<()> {
    let conv_external_id = format!("googlenews_{connection_id}_feed");
    let conv = Conversation {
        id: format!("{connection_id}-feed"),
        connection_id: connection_id.to_string(),
        connector: "googlenews".to_string(),
        external_id: conv_external_id,
        name: Some("Google News".to_string()),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn poll_keywords(
    client: &GoogleNewsClient,
    db: &Arc<Database>,
    connection_id: &str,
    keywords: &[String],
    when: &str,
    language: &str,
    country: &str,
    cancel: &CancellationToken,
    show_progress: bool,
) -> anyhow::Result<()> {
    // No keywords configured: nothing to search for.
    let searches: Vec<&str> = if keywords.is_empty() {
        Vec::new()
    } else {
        keywords.iter().map(String::as_str).collect()
    };

    let conv_id = format!("{connection_id}-feed");

    let mut progress = show_progress.then(|| {
        BackfillProgress::new(&format!("googlenews:{connection_id}"), "articles")
            .with_secondary("ingested")
    });

    for keyword in searches {
        if cancel.is_cancelled() {
            break;
        }

        let items = match client.search(keyword, when, language, country).await {
            Ok(items) => items,
            Err(e) => {
                warn!(keyword, error = %e, "Google News search failed");
                continue;
            }
        };

        for item in items {
            if cancel.is_cancelled() {
                break;
            }

            let Some(stable) = item.stable_id() else {
                continue;
            };
            let gid = sanitize_id(stable);
            let external_id = format!("googlenews_{connection_id}_{gid}");

            if let Some(ref mut p) = progress {
                p.inc(1);
            }

            if db.message_exists(connection_id, &external_id)? {
                continue;
            }

            let msg = build_message(&item, connection_id, &conv_id, &gid, keyword);
            db.upsert_message(&msg)?;

            let when_str = chrono::DateTime::from_timestamp(msg.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            let source = item.source_name().unwrap_or("Google News");
            let title = item.title.as_deref().unwrap_or("(untitled)");
            eprintln!("[googlenews:{connection_id}] {when_str} (new) {source} — {title}");

            if let Some(ref mut p) = progress {
                p.inc_secondary(1);
            }
        }
    }

    if let Some(p) = progress {
        p.finish();
    }

    if !cancel.is_cancelled() {
        db.set_sync_state(
            connection_id,
            "gn_last_sync",
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }

    Ok(())
}

fn build_message(
    item: &RssItem,
    connection_id: &str,
    conv_id: &str,
    gid: &str,
    keyword: &str,
) -> Message {
    let title = item.title.as_deref().unwrap_or("(untitled)");
    let source = item.source_name().unwrap_or("Google News");
    let link = item.link.as_deref().unwrap_or("").to_string();
    let snippet = item
        .description
        .as_deref()
        .map(strip_html)
        .filter(|s| !s.is_empty());

    let timestamp = item
        .pub_date
        .as_deref()
        .and_then(parse_pub_date)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let mut body = format!("{title}\n{source}");
    if !link.is_empty() {
        body.push('\n');
        body.push_str(&link);
    }
    if let Some(ref s) = snippet {
        body.push('\n');
        body.push_str(s);
    }

    let metadata = serde_json::json!({
        "link": link,
        "source": source,
        "guid": item.stable_id().unwrap_or(""),
        "keyword": keyword,
    });

    Message {
        id: format!("{connection_id}-{gid}"),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "googlenews".to_string(),
        external_id: format!("googlenews_{connection_id}_{gid}"),
        sender: source.to_string(),
        sender_name: Some(source.to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp,
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

/// Parse an RSS `pubDate` (RFC 2822) into a Unix timestamp.
fn parse_pub_date(raw: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(raw)
        .ok()
        .map(|dt| dt.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Guid, Source};

    fn make_item(id: &str, title: &str) -> RssItem {
        RssItem {
            title: Some(title.to_string()),
            link: Some("https://news.google.com/articles/abc".to_string()),
            guid: Some(Guid {
                value: id.to_string(),
            }),
            pub_date: Some("Mon, 09 Jun 2025 12:00:00 GMT".to_string()),
            description: Some("<a href=\"x\">A snippet</a>".to_string()),
            source: Some(Source {
                url: Some("https://www.lemonde.fr".to_string()),
                name: "Le Monde".to_string(),
            }),
        }
    }

    #[test]
    fn build_message_sets_ids_and_fields() {
        let item = make_item("CBMiQWh0", "Breaking AI news - Le Monde");
        let msg = build_message(&item, "gn", "gn-feed", "CBMiQWh0", "ai");
        assert_eq!(msg.id, "gn-CBMiQWh0");
        assert_eq!(msg.external_id, "googlenews_gn_CBMiQWh0");
        assert_eq!(msg.conversation_id, "gn-feed");
        assert_eq!(msg.connector, "googlenews");
        assert_eq!(msg.sender, "Le Monde");
        let body = msg.body.as_ref().unwrap();
        assert!(body.contains("Breaking AI news"));
        assert!(body.contains("Le Monde"));
        assert!(body.contains("A snippet"));
        let meta = msg.metadata.unwrap();
        assert_eq!(meta["keyword"], "ai");
        assert_eq!(meta["source"], "Le Monde");
    }

    #[test]
    fn pub_date_parses_rfc2822() {
        let ts = parse_pub_date("Mon, 09 Jun 2025 12:00:00 GMT").unwrap();
        // 2025-06-09T12:00:00Z
        assert_eq!(ts, 1_749_470_400);
    }

    #[test]
    fn pub_date_invalid_returns_none() {
        assert!(parse_pub_date("not a date").is_none());
    }

    #[test]
    fn build_message_tolerates_missing_fields() {
        let item = RssItem {
            guid: Some(Guid {
                value: "X".to_string(),
            }),
            ..Default::default()
        };
        let msg = build_message(&item, "gn", "gn-feed", "X", "rust");
        assert_eq!(msg.sender, "Google News");
        assert!(msg.body.as_ref().unwrap().contains("(untitled)"));
    }
}
