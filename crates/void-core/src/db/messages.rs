//! Message row operations.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use tracing::debug;

use super::row;
use crate::error::DbError;
use crate::models::Message;

pub(super) fn message_exists(
    conn: &Connection,
    connection_id: &str,
    external_id: &str,
) -> Result<bool, DbError> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM messages WHERE connection_id = ?1 AND external_id = ?2",
            params![connection_id, external_id],
            |_| Ok(()),
        )
        .is_ok())
}

/// Insert or update a message. Returns `true` if the row was newly inserted.
pub(super) fn upsert_row(conn: &Connection, msg: &Message) -> Result<bool, DbError> {
    debug!(message_id = %msg.id, "upserting message");
    let is_new = !message_exists(conn, &msg.connection_id, &msg.external_id)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages (id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(connection_id, external_id) DO UPDATE SET
            body = excluded.body,
            connector = excluded.connector,
            sender_name = excluded.sender_name,
            is_archived = excluded.is_archived,
            media_type = excluded.media_type,
            metadata = excluded.metadata,
            context_id = COALESCE(excluded.context_id, context_id)",
        params![
            msg.id,
            msg.conversation_id,
            msg.connection_id,
            msg.connector,
            msg.external_id,
            msg.sender,
            msg.sender_name,
            msg.body,
            msg.timestamp,
            msg.synced_at.unwrap_or(now),
            msg.is_archived as i32,
            msg.reply_to_id,
            msg.media_type,
            msg.metadata.as_ref().map(|v| v.to_string()),
            msg.context_id,
        ],
    )?;
    Ok(is_new)
}

pub(super) fn list_for_conversation(
    conn: &Connection,
    conversation_id: &str,
    limit: i64,
    offset: i64,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<Vec<Message>, DbError> {
    let suffix_pattern = format!("%-{conversation_id}");
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE (conversation_id = ?1 OR conversation_id LIKE ?2)",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(conversation_id.to_string()),
        Box::new(suffix_pattern),
    ];

    if let Some(s) = since {
        sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len() + 1));
        param_values.push(Box::new(s));
    }
    if let Some(u) = until {
        sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len() + 1));
        param_values.push(Box::new(u));
    }

    sql.push_str(&format!(
        " ORDER BY timestamp ASC LIMIT ?{} OFFSET ?{}",
        param_values.len() + 1,
        param_values.len() + 2
    ));
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row::row_to_message)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn count_for_conversation(
    conn: &Connection,
    conversation_id: &str,
    since: Option<i64>,
    until: Option<i64>,
) -> Result<i64, DbError> {
    let suffix_pattern = format!("%-{conversation_id}");
    let mut sql = String::from(
        "SELECT COUNT(*) FROM messages WHERE (conversation_id = ?1 OR conversation_id LIKE ?2)",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(conversation_id.to_string()),
        Box::new(suffix_pattern),
    ];

    if let Some(s) = since {
        sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len() + 1));
        param_values.push(Box::new(s));
    }
    if let Some(u) = until {
        sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len() + 1));
        param_values.push(Box::new(u));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let count = stmt.query_row(params_ref.as_slice(), |row| row.get(0))?;
    Ok(count)
}

pub(super) fn get(conn: &Connection, id: &str) -> Result<Option<Message>, DbError> {
    let suffix_pattern = format!("%-{id}");
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE id = ?1 OR id LIKE ?2",
        params![id, suffix_pattern],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}

pub(super) fn latest_timestamp(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
) -> Result<Option<i64>, DbError> {
    conn.query_row(
        "SELECT MAX(timestamp) FROM messages WHERE connection_id = ?1 AND connector = ?2",
        params![connection_id, connector],
        |row| row.get::<_, Option<i64>>(0),
    )
    .map_err(Into::into)
}

pub(super) fn list_recent(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    limit: i64,
    offset: i64,
    include_archived: bool,
    include_muted: bool,
) -> Result<Vec<Message>, DbError> {
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !include_archived {
        sql.push_str(" AND is_archived = 0");
    }
    if !include_muted {
        sql.push_str(
            " AND NOT EXISTS (SELECT 1 FROM conversations c WHERE c.id = messages.conversation_id AND c.is_muted = 1)",
        );
    }
    if let Some(acct) = connection_filter {
        let pattern = format!("%{acct}%");
        sql.push_str(&format!(
            " AND connection_id LIKE ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(pattern));
    }
    if let Some(conn_type) = connector_filter {
        sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
        param_values.push(Box::new(conn_type.to_string()));
    }

    sql.push_str(&format!(
        " ORDER BY timestamp DESC LIMIT ?{} OFFSET ?{}",
        param_values.len() + 1,
        param_values.len() + 2
    ));
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row::row_to_message)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn count_recent(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    include_archived: bool,
    include_muted: bool,
) -> Result<i64, DbError> {
    let mut sql = String::from("SELECT COUNT(*) FROM messages WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !include_archived {
        sql.push_str(" AND is_archived = 0");
    }
    if !include_muted {
        sql.push_str(
            " AND NOT EXISTS (SELECT 1 FROM conversations c WHERE c.id = messages.conversation_id AND c.is_muted = 1)",
        );
    }
    if let Some(acct) = connection_filter {
        let pattern = format!("%{acct}%");
        sql.push_str(&format!(
            " AND connection_id LIKE ?{}",
            param_values.len() + 1
        ));
        param_values.push(Box::new(pattern));
    }
    if let Some(conn_type) = connector_filter {
        sql.push_str(&format!(" AND connector = ?{}", param_values.len() + 1));
        param_values.push(Box::new(conn_type.to_string()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let count = stmt.query_row(params_ref.as_slice(), |row| row.get(0))?;
    Ok(count)
}

/// Archive all unarchived messages with `timestamp < before_ts`, optionally
/// filtered by connector type. Returns the affected messages (pre-update) so
/// callers can clean up cached files.
pub(super) fn bulk_archive_before(
    conn: &Connection,
    before_ts: i64,
    connector_filter: Option<&str>,
) -> Result<Vec<Message>, DbError> {
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE is_archived = 0 AND timestamp < ?1",
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

    let mut update_sql =
        String::from("UPDATE messages SET is_archived = 1 WHERE is_archived = 0 AND timestamp < ?1");
    let mut update_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(before_ts)];

    if let Some(ct) = connector_filter {
        update_sql.push_str(&format!(" AND connector = ?{}", update_params.len() + 1));
        update_params.push(Box::new(ct.to_string()));
    }

    let update_ref: Vec<&dyn rusqlite::types::ToSql> =
        update_params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&update_sql, update_ref.as_slice())?;

    debug!(count = messages.len(), "bulk archived messages before cutoff");
    Ok(messages)
}

pub(super) fn mark_archived(conn: &Connection, id: &str) -> Result<bool, DbError> {
    debug!(message_id = %id, "marking message as archived");
    let updated = conn.execute(
        "UPDATE messages SET is_archived = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(updated > 0)
}

pub(super) fn update_metadata(
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

pub(super) fn find_by_external_id(
    conn: &Connection,
    connection_id: &str,
    external_id: &str,
) -> Result<Option<Message>, DbError> {
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE connection_id = ?1 AND external_id = ?2",
        params![connection_id, external_id],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}

pub(super) fn enrich_with_context(
    conn: &Connection,
    messages: &mut [Message],
) -> Result<(), DbError> {
    let context_ids: HashSet<&str> = messages
        .iter()
        .filter_map(|m| m.context_id.as_deref())
        .collect();

    if context_ids.is_empty() {
        return Ok(());
    }

    let mut context_map: HashMap<String, Vec<Message>> = HashMap::new();

    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
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
pub(super) fn reconcile_inbox(
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
pub(super) fn messages_pending_file_download(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
    limit: i64,
) -> Result<Vec<Message>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages
         WHERE connection_id = ?1 AND connector = ?2
           AND metadata LIKE '%url_private%'
           AND metadata NOT LIKE '%local_path%'
         ORDER BY timestamp DESC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![connection_id, connector, limit], row::row_to_message)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn last_in_conversation(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Option<Message>, DbError> {
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id
         FROM messages WHERE conversation_id = ?1 ORDER BY timestamp DESC LIMIT 1",
        params![conversation_id],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}
