//! Conversation row operations.

use rusqlite::{params, Connection, OptionalExtension};
use tracing::debug;

use super::row;
use crate::error::DbError;
use crate::models::Conversation;

pub(super) fn upsert(conn: &Connection, conv: &Conversation) -> Result<(), DbError> {
    debug!(conversation_id = %conv.id, "upserting conversation");
    conn.execute(
        "INSERT INTO conversations (id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(connection_id, external_id) DO UPDATE SET
            name = excluded.name,
            connector = excluded.connector,
            kind = excluded.kind,
            last_message_at = COALESCE(excluded.last_message_at, last_message_at),
            unread_count = excluded.unread_count,
            metadata = excluded.metadata",
        params![
            conv.id,
            conv.connection_id,
            conv.connector,
            conv.external_id,
            conv.name,
            conv.kind.to_string(),
            conv.last_message_at,
            conv.unread_count,
            conv.is_muted as i32,
            conv.metadata.as_ref().map(|v| v.to_string()),
        ],
    )?;
    Ok(())
}

pub(super) fn list(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    limit: i64,
    offset: i64,
    include_muted: bool,
) -> Result<Vec<Conversation>, DbError> {
    let mut sql = String::from(
        "SELECT id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
         FROM conversations WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !include_muted {
        sql.push_str(" AND is_muted = 0");
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
        " ORDER BY last_message_at DESC NULLS LAST LIMIT ?{} OFFSET ?{}",
        param_values.len() + 1,
        param_values.len() + 2
    ));
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row::row_to_conversation)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn count(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    include_muted: bool,
) -> Result<i64, DbError> {
    let mut sql = String::from("SELECT COUNT(*) FROM conversations WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if !include_muted {
        sql.push_str(" AND is_muted = 0");
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

pub(super) fn find_by_name(
    conn: &Connection,
    name: &str,
    connector: &str,
) -> Result<Option<Conversation>, DbError> {
    conn.query_row(
        "SELECT id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
         FROM conversations WHERE name = ?1 AND connector = ?2 LIMIT 1",
        params![name, connector],
        row::row_to_conversation,
    )
    .optional()
    .map_err(Into::into)
}

pub(super) fn get(conn: &Connection, id: &str) -> Result<Option<Conversation>, DbError> {
    conn.query_row(
        "SELECT id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
         FROM conversations WHERE id = ?1",
        params![id],
        row::row_to_conversation,
    )
    .optional()
    .map_err(Into::into)
}
