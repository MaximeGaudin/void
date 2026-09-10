use rusqlite::{params, Connection};
use tracing::debug;

use super::super::row;
use crate::error::DbError;
use crate::models::Message;

/// Archive all unarchived messages with `timestamp < before_ts`, optionally
/// filtered by connector type. Returns the affected messages (pre-update) so
/// callers can clean up cached files.
pub fn bulk_archive_before(
    conn: &Connection,
    before_ts: i64,
    connector_filter: Option<&str>,
) -> Result<Vec<Message>, DbError> {
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE is_archived = 0 AND timestamp < ?1 AND synced_at < ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(before_ts)];

    if let Some(ct) = connector_filter {
        sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
        param_values.push(Box::new(ct.to_string()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let messages: Vec<Message> = stmt
        .query_map(params_ref.as_slice(), row::row_to_message)?
        .collect::<Result<_, _>>()?;

    let mut update_sql = String::from(
        "UPDATE messages SET is_archived = 1 WHERE is_archived = 0 AND timestamp < ?1 AND synced_at < ?1",
    );
    let mut update_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(before_ts)];

    if let Some(ct) = connector_filter {
        update_sql.push_str(&format!(" AND connector = ?{}", update_params.len() + 1));
        update_params.push(Box::new(ct.to_string()));
    }

    let update_ref: Vec<&dyn rusqlite::types::ToSql> =
        update_params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&update_sql, update_ref.as_slice())?;

    debug!(
        count = messages.len(),
        "bulk archived messages before cutoff"
    );
    Ok(messages)
}

pub fn mark_archived(conn: &Connection, id: &str) -> Result<bool, DbError> {
    debug!(message_id = %id, "marking message as archived");
    let updated = conn.execute(
        "UPDATE messages SET is_archived = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(updated > 0)
}

/// Archive a message and every sibling sharing its `context_id` (Slack thread /
/// time-window group, Gmail thread, …). Inbox dedup shows one row per context, so
/// archiving only the visible id leaves older siblings to reappear as the next
/// representative (GitHub issue #67).
///
/// Messages with no `context_id` are archived alone. Returns the rows that were
/// newly marked archived (for cache cleanup); an already-archived message yields
/// an empty vector. Select and update run in one transaction so a concurrent
/// insert into the same context cannot be archived without being reported.
pub fn mark_archived_with_context(conn: &Connection, id: &str) -> Result<Vec<Message>, DbError> {
    let Some(msg) = super::read::get(conn, id)? else {
        return Ok(Vec::new());
    };

    // Static clauses; the value always travels as a bound parameter.
    let (where_clause, param): (&str, &str) = match msg.context_id.as_deref() {
        Some(ctx) => ("context_id = ?1", ctx),
        None if msg.is_archived => return Ok(Vec::new()),
        None => ("id = ?1", id),
    };

    let tx = conn.unchecked_transaction()?;

    let mut stmt = tx.prepare(&format!(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE {where_clause} AND is_archived = 0"
    ))?;
    let to_archive: Vec<Message> = stmt
        .query_map(params![param], row::row_to_message)?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    if to_archive.is_empty() {
        return Ok(Vec::new());
    }

    tx.execute(
        &format!("UPDATE messages SET is_archived = 1 WHERE {where_clause} AND is_archived = 0"),
        params![param],
    )?;
    tx.commit()?;

    match msg.context_id.as_deref() {
        Some(ctx) => debug!(
            message_id = %id,
            context_id = %ctx,
            count = to_archive.len(),
            "archived message context group"
        ),
        None => debug!(message_id = %id, "archived single message (no context)"),
    }

    Ok(to_archive)
}

pub fn update_metadata(
    conn: &Connection,
    id: &str,
    metadata: &serde_json::Value,
) -> Result<bool, DbError> {
    debug!(message_id = %id, "updating message metadata");
    let json = serde_json::to_string(metadata).map_err(|e| DbError::Other(e.to_string()))?;
    let updated = conn.execute(
        "UPDATE messages SET metadata = ?2 WHERE id = ?1",
        params![id, json],
    )?;
    Ok(updated > 0)
}
