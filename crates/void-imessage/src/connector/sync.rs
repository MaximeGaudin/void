//! Sync loop: poll the local Messages store and mirror new rows into Void.
//!
//! There is no push notification for `chat.db`, so this polls. The cursor is
//! `message.ROWID`, which Messages assigns monotonically, so a poll only ever
//! reads rows appended since the previous pass.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

use super::store::{self, ImessageRow, StoreError};
use crate::CONNECTOR_ID;

/// Same hibernation threshold the other polling connectors use.
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

/// Rows per pass. Bounded so a first sync over a large store cannot hold the
/// runtime for minutes: the cursor advances and the next tick continues.
const BATCH_SIZE: usize = 500;

pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    db_path: &std::path::Path,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    // Fail loudly and immediately on a permission problem rather than looping
    // silently forever: a connector that never ingests anything looks healthy
    // from the outside.
    if let Err(err @ StoreError::AccessDenied(_)) = store::open_read_only(db_path) {
        return Err(anyhow::anyhow!(err));
    }

    info!(connection_id, "running initial iMessage sync");
    if let Err(e) = poll_once(db, connection_id, db_path).await {
        error!(connection_id, error = %e, "initial iMessage sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    interval.tick().await; // first tick is immediate; we just synced
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "iMessage sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "iMessage sync was idle, catching up"
                    );
                    void_core::status!(
                        "[imessage:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                }
                if let Err(e) = poll_once(db, connection_id, db_path).await {
                    error!(connection_id, error = %e, "iMessage poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

/// One pass: read everything past the cursor and store it.
async fn poll_once(
    db: &Arc<Database>,
    connection_id: &str,
    db_path: &std::path::Path,
) -> anyhow::Result<()> {
    let conn = store::open_read_only(db_path)?;
    let mut cursor = load_cursor(db, connection_id)?;

    loop {
        let rows = store::fetch_since(&conn, cursor, BATCH_SIZE)?;
        if rows.is_empty() {
            break;
        }
        let highest = rows.iter().map(|r| r.rowid).max().unwrap_or(cursor);
        for row in &rows {
            if let Err(e) = ingest_row(db, connection_id, row) {
                error!(rowid = row.rowid, error = %e, "could not ingest iMessage row");
            }
        }
        cursor = highest;
        save_cursor(db, connection_id, cursor)?;
        info!(connection_id, cursor, "ingested a batch of iMessage rows");
    }
    Ok(())
}

/// Store one row as a Void message, creating its conversation if needed.
fn ingest_row(db: &Arc<Database>, connection_id: &str, row: &ImessageRow) -> anyhow::Result<()> {
    // A message with neither a chat nor a handle cannot be addressed or
    // displayed; Messages keeps such rows for internal bookkeeping.
    let Some(conv_external) = row
        .chat_guid
        .clone()
        .or_else(|| row.handle.clone())
        .filter(|s| !s.is_empty())
    else {
        return Ok(());
    };

    let external_id = format!("imessage_{connection_id}_{}", row.guid);
    if db.message_exists(connection_id, &external_id)? {
        return Ok(());
    }

    let conv_id = format!("im_{connection_id}_{conv_external}");
    let conv = Conversation {
        id: conv_id.clone(),
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id: conv_external.clone(),
        name: row.handle.clone(),
        // A chat GUID carrying "chat" rather than a bare handle is a group.
        kind: if conv_external.contains("chat") && !conv_external.contains(';') {
            ConversationKind::Group
        } else {
            ConversationKind::Dm
        },
        last_message_at: Some(row.date_unix),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;

    let sender = if row.is_from_me {
        "me".to_string()
    } else {
        row.handle.clone().unwrap_or_else(|| conv_external.clone())
    };

    let message = Message {
        id: format!("{conv_id}_{}", row.guid),
        conversation_id: conv_id,
        connection_id: connection_id.to_string(),
        connector: CONNECTOR_ID.to_string(),
        external_id,
        sender,
        sender_name: row.handle.clone(),
        sender_avatar_url: None,
        body: row.body.clone(),
        timestamp: row.date_unix,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(serde_json::json!({
            "service": row.service,
            "is_from_me": row.is_from_me,
            "date_delivered": row.date_delivered_unix,
            "date_read": row.date_read_unix,
        })),
        context_id: None,
        context: None,
    };
    db.upsert_message(&message)?;
    Ok(())
}

fn cursor_key() -> &'static str {
    "imessage_cursor"
}

fn load_cursor(db: &Arc<Database>, connection_id: &str) -> anyhow::Result<i64> {
    Ok(db
        .get_sync_state(connection_id, cursor_key())?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0))
}

fn save_cursor(db: &Arc<Database>, connection_id: &str, cursor: i64) -> anyhow::Result<()> {
    db.set_sync_state(connection_id, cursor_key(), &cursor.to_string())?;
    Ok(())
}
