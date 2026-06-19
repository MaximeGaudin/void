use std::collections::HashSet;

use rusqlite::{params, Connection};

use super::super::row;
use crate::error::DbError;
use crate::models::Message;

const MESSAGE_COLUMNS: &str = "id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved";

/// Reconcile `is_saved` for a connection: messages whose external_id is in
/// `saved_external_ids` get `is_saved = 1`, all others get `is_saved = 0`.
pub fn reconcile_saved(
    conn: &Connection,
    connection_id: &str,
    connector: &str,
    saved_external_ids: &HashSet<String>,
) -> Result<(usize, usize), DbError> {
    let mut stmt = conn.prepare(
        "SELECT external_id, is_saved FROM messages WHERE connection_id = ?1 AND connector = ?2",
    )?;
    let rows: Vec<(String, bool)> = stmt
        .query_map(params![connection_id, connector], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? != 0))
        })?
        .collect::<Result<_, _>>()?;

    let mut newly_saved = 0usize;
    let mut newly_unsaved = 0usize;

    let mut mark_stmt = conn.prepare(
        "UPDATE messages SET is_saved = ?3 WHERE connection_id = ?1 AND external_id = ?2",
    )?;

    for (ext_id, was_saved) in &rows {
        let should_save = saved_external_ids.contains(ext_id);
        if should_save != *was_saved {
            mark_stmt.execute(params![connection_id, ext_id, should_save as i32])?;
            if should_save {
                newly_saved += 1;
            } else {
                newly_unsaved += 1;
            }
        }
    }

    Ok((newly_saved, newly_unsaved))
}

pub fn list_saved(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Message>, DbError> {
    let mut sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE is_saved = 1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

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

pub fn count_saved(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> Result<i64, DbError> {
    let mut sql = String::from("SELECT COUNT(*) FROM messages WHERE is_saved = 1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

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
