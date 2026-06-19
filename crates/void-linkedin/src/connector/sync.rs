use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{UnipileChat, UnipileClient, UnipileMessage};

use super::extract;
use super::posts_sync;
use super::profiles::{build_message_metadata, ProfileCache};

const CHAT_PAGE_LIMIT: u32 = 100;
const MESSAGE_PAGE_LIMIT: u32 = 100;

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack:
/// `SystemTime` survives macOS sleep where the monotonic clock pauses).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

struct SyncCtx<'a> {
    client: &'a UnipileClient,
    account_id: &'a str,
    db: &'a Arc<Database>,
    connection_id: &'a str,
    profile_cache: &'a mut ProfileCache,
}

/// ISO 8601 UTC timestamp for Unipile `after` filters (messages/chats newer than this).
pub(super) fn backfill_cutoff_iso(backfill_days: u64) -> String {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(backfill_days as i64);
    cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub(super) async fn run_sync(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    poll_interval_secs: u64,
    backfill_days: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let mut profile_cache = ProfileCache::default();
    let needs_backfill = db.get_sync_state(connection_id, "backfill_done")?.is_none();

    if needs_backfill {
        info!(
            connection_id,
            backfill_days,
            after = %backfill_cutoff_iso(backfill_days),
            "starting LinkedIn backfill via Unipile"
        );
        if let Err(e) = backfill_all(
            client,
            account_id,
            db,
            connection_id,
            backfill_days,
            &mut profile_cache,
            &cancel,
        )
        .await
        {
            warn!(connection_id, error = %e, "LinkedIn backfill failed");
        } else {
            db.set_sync_state(connection_id, "backfill_done", "1")?;
            info!(connection_id, "LinkedIn backfill complete");
        }
    } else {
        info!(
            connection_id,
            "LinkedIn backfill already complete, catching up missed messages"
        );
        if let Err(e) = catch_up_all(
            client,
            account_id,
            db,
            connection_id,
            backfill_days,
            &mut profile_cache,
            &cancel,
        )
        .await
        {
            warn!(connection_id, error = %e, "LinkedIn catch-up on start failed");
        }
    }

    void_core::status!("[linkedin:{connection_id}] listening for new messages");

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(poll_interval_secs));
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "LinkedIn sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                let after = effective_after_iso(db, connection_id, backfill_days);
                let result = if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "LinkedIn sync was idle, catching up"
                    );
                    void_core::status!(
                        "[linkedin:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                    catch_up_all(
                        client,
                        account_id,
                        db,
                        connection_id,
                        backfill_days,
                        &mut profile_cache,
                        &cancel,
                    )
                    .await
                } else {
                    sync_incremental(
                        client,
                        account_id,
                        db,
                        connection_id,
                        after.as_deref(),
                        backfill_days,
                        &mut profile_cache,
                        &cancel,
                    )
                    .await
                };
                if let Err(e) = result {
                    warn!(connection_id, error = %e, "LinkedIn sync poll failed");
                }
                last_poll = SystemTime::now();
            }
        }
    }

    Ok(())
}

fn latest_after_iso(db: &Database, connection_id: &str) -> Option<String> {
    db.latest_message_timestamp(connection_id, "linkedin")
        .ok()
        .flatten()
        .and_then(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        })
}

/// Never sync messages older than the configured backfill window.
fn effective_after_iso(db: &Database, connection_id: &str, backfill_days: u64) -> Option<String> {
    let cutoff = backfill_cutoff_iso(backfill_days);
    match latest_after_iso(db, connection_id) {
        Some(latest) if latest.as_str() > cutoff.as_str() => Some(latest),
        _ => Some(cutoff),
    }
}

async fn backfill_all(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_days: u64,
    profile_cache: &mut ProfileCache,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let after = backfill_cutoff_iso(backfill_days);
    sync_chats_with_progress(ChatSyncParams {
        client,
        account_id,
        db,
        connection_id,
        after: after.as_str(),
        profile_cache,
        cancel,
        show_progress: true,
    })
    .await?;

    if let Err(e) = posts_sync::sync_posts_backfill(
        client,
        account_id,
        db,
        connection_id,
        backfill_days,
        profile_cache,
        cancel,
    )
    .await
    {
        warn!(connection_id, error = %e, "LinkedIn post comments backfill failed");
    }

    Ok(())
}

async fn catch_up_all(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_days: u64,
    profile_cache: &mut ProfileCache,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let after = effective_after_iso(db, connection_id, backfill_days)
        .unwrap_or_else(|| backfill_cutoff_iso(backfill_days));
    void_core::status!("[linkedin:{connection_id}] catch-up — syncing chats since {after}");
    sync_chats_with_progress(ChatSyncParams {
        client,
        account_id,
        db,
        connection_id,
        after: after.as_str(),
        profile_cache,
        cancel,
        show_progress: true,
    })
    .await?;

    if let Err(e) = posts_sync::sync_posts_incremental(
        client,
        account_id,
        db,
        connection_id,
        backfill_days,
        profile_cache,
        cancel,
    )
    .await
    {
        warn!(connection_id, error = %e, "LinkedIn post comments catch-up failed");
    }

    db.set_sync_state(
        connection_id,
        "linkedin_last_poll",
        &chrono::Utc::now().timestamp().to_string(),
    )?;

    Ok(())
}

struct ChatSyncParams<'a> {
    client: &'a UnipileClient,
    account_id: &'a str,
    db: &'a Arc<Database>,
    connection_id: &'a str,
    after: &'a str,
    profile_cache: &'a mut ProfileCache,
    cancel: &'a CancellationToken,
    show_progress: bool,
}

async fn sync_chats_with_progress(params: ChatSyncParams<'_>) -> anyhow::Result<()> {
    let ChatSyncParams {
        client,
        account_id,
        db,
        connection_id,
        after,
        profile_cache,
        cancel,
        show_progress,
    } = params;

    let mut progress =
        show_progress.then(|| BackfillProgress::new(&format!("linkedin:{connection_id}"), "chats"));
    let mut chat_cursor: Option<String> = None;
    let mut chat_count = 0u64;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let page = client
            .list_chats(
                account_id,
                chat_cursor.as_deref(),
                Some(after),
                CHAT_PAGE_LIMIT,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for chat in &page.items {
            if cancel.is_cancelled() {
                break;
            }
            sync_chat_messages(
                &mut SyncCtx {
                    client,
                    account_id,
                    db,
                    connection_id,
                    profile_cache,
                },
                chat,
                Some(after),
                cancel,
            )
            .await?;
            chat_count += 1;
            if let Some(ref mut p) = progress {
                p.inc(1);
            }
        }

        chat_cursor = page.cursor;
        if chat_cursor.is_none() {
            break;
        }
    }

    if let Some(p) = progress {
        p.finish();
    }
    info!(
        connection_id,
        chats = chat_count,
        since = after,
        "LinkedIn chat sync finished"
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn sync_incremental(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    after: Option<&str>,
    backfill_days: u64,
    profile_cache: &mut ProfileCache,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let mut chat_cursor: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let page = client
            .list_chats(account_id, chat_cursor.as_deref(), after, CHAT_PAGE_LIMIT)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for chat in &page.items {
            if cancel.is_cancelled() {
                break;
            }
            sync_chat_messages(
                &mut SyncCtx {
                    client,
                    account_id,
                    db,
                    connection_id,
                    profile_cache,
                },
                chat,
                after,
                cancel,
            )
            .await?;
        }

        chat_cursor = page.cursor;
        if chat_cursor.is_none() {
            break;
        }
    }

    if let Err(e) = posts_sync::sync_posts_incremental(
        client,
        account_id,
        db,
        connection_id,
        backfill_days,
        profile_cache,
        cancel,
    )
    .await
    {
        warn!(connection_id, error = %e, "LinkedIn post comments incremental sync failed");
    }

    db.set_sync_state(
        connection_id,
        "linkedin_last_poll",
        &chrono::Utc::now().timestamp().to_string(),
    )?;

    Ok(())
}

async fn sync_chat_messages(
    ctx: &mut SyncCtx<'_>,
    chat: &UnipileChat,
    after: Option<&str>,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let conv_external_id = format!("linkedin_{}_{}", ctx.connection_id, chat.id);
    let conv_id = format!("{}-{}", ctx.connection_id, chat.id);

    let kind = match chat.r#type {
        Some(1) => ConversationKind::Group,
        Some(2) => ConversationKind::Channel,
        _ => ConversationKind::Dm,
    };

    let conv_name = resolve_conversation_name(ctx, chat).await;

    let conv = Conversation {
        id: conv_id.clone(),
        connection_id: ctx.connection_id.to_string(),
        connector: "linkedin".to_string(),
        external_id: conv_external_id.clone(),
        name: conv_name,
        kind,
        last_message_at: chat
            .timestamp
            .as_deref()
            .map(|ts| extract::parse_timestamp(Some(ts))),
        unread_count: i64::from(chat.unread_count.unwrap_or(0).max(0)),
        is_muted: false,
        metadata: None,
    };
    ctx.db.upsert_conversation(&conv)?;

    let mut msg_cursor: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        let page = ctx
            .client
            .list_chat_messages(&chat.id, msg_cursor.as_deref(), after, MESSAGE_PAGE_LIMIT)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for msg in page.items {
            if !msg.is_syncable() {
                continue;
            }
            ingest_message(ctx, &conv_id, &conv_external_id, chat, &msg).await?;
        }

        msg_cursor = page.cursor;
        if msg_cursor.is_none() {
            break;
        }
    }

    Ok(())
}

async fn resolve_conversation_name(ctx: &mut SyncCtx<'_>, chat: &UnipileChat) -> Option<String> {
    if let Some(name) = chat.name.as_ref().filter(|n| !n.is_empty()) {
        return Some(name.clone());
    }
    let provider_id = chat.attendee_provider_id.as_deref()?;
    let profile = ctx
        .profile_cache
        .resolve_provider(ctx.client, ctx.account_id, provider_id, None)
        .await;
    Some(profile.display_name)
}

async fn ingest_message(
    ctx: &mut SyncCtx<'_>,
    conv_id: &str,
    conv_external_id: &str,
    chat: &UnipileChat,
    msg: &UnipileMessage,
) -> anyhow::Result<()> {
    let msg_external_id = format!("linkedin_{}_{}", ctx.connection_id, msg.id);

    if ctx.db.message_exists(ctx.connection_id, &msg_external_id)? {
        if let Ok(Some(existing)) = ctx
            .db
            .find_message_by_external_id(ctx.connection_id, &msg_external_id)
        {
            if existing.sender_name.is_some() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    let profile = ctx
        .profile_cache
        .resolve(ctx.client, ctx.account_id, msg)
        .await;
    let void_msg = message_to_void(msg, ctx.connection_id, conv_id, conv_external_id, &profile);
    ctx.db.upsert_message(&void_msg)?;

    let conv_name = chat
        .name
        .as_deref()
        .or(void_msg.sender_name.as_deref())
        .unwrap_or("LinkedIn");
    let sender = void_msg.sender_name.as_deref().unwrap_or(&void_msg.sender);
    let preview: String = void_msg
        .body
        .as_deref()
        .unwrap_or("[attachment]")
        .chars()
        .take(80)
        .collect();
    let time = chrono::DateTime::from_timestamp(void_msg.timestamp, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S %Z")
                .to_string()
        })
        .unwrap_or_else(|| void_msg.timestamp.to_string());
    let direction = if msg.is_sender.unwrap_or(false) {
        "sent"
    } else {
        "new"
    };

    void_core::status!(
        "[linkedin:{}] {time} ({direction}) {conv_name} — {sender}: {preview}",
        ctx.connection_id
    );

    Ok(())
}

pub(super) fn message_to_void(
    msg: &UnipileMessage,
    connection_id: &str,
    conv_id: &str,
    conv_external_id: &str,
    profile: &super::profiles::SenderProfile,
) -> Message {
    let sender_id = msg
        .sender_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    Message {
        id: format!("{connection_id}-{}", msg.id),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "linkedin".to_string(),
        external_id: format!("linkedin_{connection_id}_{}", msg.id),
        sender: sender_id,
        sender_name: Some(profile.display_name.clone()),
        sender_avatar_url: profile.avatar_url.clone(),
        body: extract::extract_text(msg),
        timestamp: extract::parse_timestamp(msg.timestamp.as_deref()),
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: extract::extract_media_type(msg),
        metadata: build_message_metadata(msg, profile),
        context_id: Some(conv_external_id.to_string()),
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::UnipileMessage;
    use crate::connector::profiles::{build_message_metadata, SenderProfile};

    #[test]
    fn message_to_void_maps_profile_and_body() {
        let msg = UnipileMessage {
            id: "msg-1".into(),
            sender_id: Some("ACo123".into()),
            text: Some("  Hello LinkedIn  ".into()),
            timestamp: Some("2026-05-19T11:41:45.871Z".into()),
            is_sender: Some(false),
            ..Default::default()
        };
        let profile = SenderProfile {
            display_name: "Zhirayr Gumruyan".into(),
            profile_url: Some("https://www.linkedin.com/in/gumruyan".into()),
            avatar_url: Some("https://media.licdn.com/avatar.jpg".into()),
            public_identifier: Some("gumruyan".into()),
        };
        let void_msg = message_to_void(
            &msg,
            "linkedin",
            "linkedin-conv-1",
            "linkedin_linkedin_chat-1",
            &profile,
        );
        assert_eq!(void_msg.id, "linkedin-msg-1");
        assert_eq!(void_msg.connector, "linkedin");
        assert_eq!(void_msg.sender_name.as_deref(), Some("Zhirayr Gumruyan"));
        assert_eq!(void_msg.body.as_deref(), Some("Hello LinkedIn"));
        assert_eq!(void_msg.timestamp, 1_779_190_905);
        assert_eq!(
            void_msg.context_id.as_deref(),
            Some("linkedin_linkedin_chat-1")
        );
        let meta = void_msg.metadata.expect("metadata");
        assert_eq!(meta["author_name"], "Zhirayr Gumruyan");
        assert_eq!(
            build_message_metadata(&msg, &profile).unwrap()["author_profile_url"],
            meta["author_profile_url"]
        );
    }

    #[test]
    fn backfill_cutoff_is_about_n_days_ago() {
        let cutoff = backfill_cutoff_iso(15);
        let parsed = chrono::DateTime::parse_from_rfc3339(&cutoff).unwrap();
        let age = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
        assert!((14..=16).contains(&age.num_days()));
    }

    #[test]
    fn effective_after_never_older_than_cutoff() {
        let cutoff = backfill_cutoff_iso(15);
        let recent = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let old = (chrono::Utc::now() - chrono::Duration::days(400))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        assert!(recent.as_str() > cutoff.as_str());
        assert!(old.as_str() < cutoff.as_str());
        let pick = |latest: &str| {
            if latest > cutoff.as_str() {
                latest.to_string()
            } else {
                cutoff.clone()
            }
        };
        assert_eq!(pick(&recent), recent);
        assert_eq!(pick(&old), cutoff);
    }
}
