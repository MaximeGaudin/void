use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};
use tracing::debug;

use super::super::row;
use crate::error::DbError;
use crate::models::Message;

pub fn enrich_with_context(conn: &Connection, messages: &mut [Message]) -> Result<(), DbError> {
    let context_ids: HashSet<&str> = messages
        .iter()
        .filter_map(|m| m.context_id.as_deref())
        .collect();

    if context_ids.is_empty() {
        return Ok(());
    }

    let mut context_map: HashMap<String, Vec<Message>> = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE context_id = ?1 ORDER BY timestamp ASC LIMIT 50",
    )?;

    for ctx_id in &context_ids {
        let rows = stmt.query_map(params![ctx_id], row::row_to_message)?;
        let ctx_messages: Vec<Message> = rows.collect::<Result<_, _>>()?;
        context_map.insert(ctx_id.to_string(), ctx_messages);
    }

    for msg in messages.iter_mut() {
        if let Some(ctx_id) = &msg.context_id {
            if let Some(ctx_messages) = context_map.get(ctx_id) {
                if ctx_messages.len() > 1 {
                    msg.context = Some(ctx_messages.clone());
                }
            }
        }
    }

    Ok(())
}

/// Reconcile `is_archived` for a connection: messages whose external_id is in
/// `inbox_external_ids` get `is_archived = 0`, all others get `is_archived = 1`.
pub fn reconcile_inbox(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
    inbox_external_ids: &HashSet<String>,
) -> Result<(usize, usize), DbError> {
    let mut stmt = conn.prepare(
        "SELECT external_id, is_archived FROM messages WHERE connection_id = ?1 AND connector = ?2",
    )?;
    let rows: Vec<(String, bool)> = stmt
        .query_map(params![connection_id, connector], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? != 0))
        })?
        .collect::<Result<_, _>>()?;

    let mut unarchived = 0usize;
    let mut archived = 0usize;

    let mut mark_stmt = conn.prepare(
        "UPDATE messages SET is_archived = ?3 WHERE connection_id = ?1 AND external_id = ?2",
    )?;

    for (ext_id, was_archived) in &rows {
        let should_archive = !inbox_external_ids.contains(ext_id);
        if should_archive != *was_archived {
            mark_stmt.execute(params![connection_id, ext_id, should_archive as i32])?;
            if should_archive {
                archived += 1;
            } else {
                unarchived += 1;
            }
        }
    }

    Ok((unarchived, archived))
}

/// Find messages that have files with `url_private` but no `local_path` yet.
pub fn messages_pending_file_download(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
    limit: i64,
) -> Result<Vec<Message>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages
         WHERE connection_id = ?1 AND connector = ?2
           AND metadata LIKE '%url_private%'
           AND metadata NOT LIKE '%local_path%'
           AND metadata NOT LIKE '%download_skipped%'
         ORDER BY timestamp DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        params![connection_id, connector, limit],
        row::row_to_message,
    )?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Bulk-set `sender_avatar_url` for messages that don't have one yet.
/// Takes a map of sender_id → avatar_url and updates all matching messages
/// within the given connection/connector in a single transaction.
pub fn backfill_avatar_urls(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
    avatars: &HashMap<String, String>,
) -> Result<usize, DbError> {
    let mut stmt = conn.prepare(
        "UPDATE messages SET sender_avatar_url = ?1
         WHERE connection_id = ?2 AND connector = ?3 AND sender = ?4
           AND sender_avatar_url IS NULL",
    )?;
    let mut total = 0usize;
    for (sender, avatar_url) in avatars {
        let updated = stmt.execute(params![avatar_url, connection_id, connector, sender])?;
        total += updated;
    }
    debug!(
        connection_id,
        connector,
        updated = total,
        "backfilled avatar URLs"
    );
    Ok(total)
}

/// Return distinct sender IDs that have no `sender_avatar_url` for the given connection/connector.
pub fn senders_missing_avatar(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT sender FROM messages
         WHERE connection_id = ?1 AND connector = ?2 AND sender_avatar_url IS NULL",
    )?;
    let rows = stmt.query_map(params![connection_id, connector], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
