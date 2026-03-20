//! Aggregated contact and channel listings.

use rusqlite::Connection;

use super::row;
use crate::error::DbError;
use crate::models::{Contact, Conversation};

pub(super) fn list_contacts(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    search: Option<&str>,
    limit: i64,
) -> Result<Vec<Contact>, DbError> {
    let mut sql = String::from(
        "SELECT sender, sender_name, connection_id, connector, COUNT(*) as msg_count, MAX(timestamp) as last_ts
         FROM messages WHERE sender != connection_id",
    );
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

    if let Some(q) = search {
        let pattern = format!("%{q}%");
        sql.push_str(&format!(
            " AND (sender LIKE ?{} OR sender_name LIKE ?{})",
            param_values.len() + 1,
            param_values.len() + 1
        ));
        param_values.push(Box::new(pattern));
    }

    sql.push_str(" GROUP BY sender, connection_id");
    sql.push_str(&format!(
        " ORDER BY last_ts DESC LIMIT ?{}",
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(Contact {
            sender: row.get(0)?,
            sender_name: row.get(1)?,
            connection_id: row.get(2)?,
            connector: row.get(3)?,
            message_count: row.get(4)?,
            last_message_at: row.get(5)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn list_channels(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    search: Option<&str>,
    limit: i64,
    include_muted: bool,
) -> Result<Vec<Conversation>, DbError> {
    let mut sql = String::from(
        "SELECT id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
         FROM conversations WHERE kind IN ('group', 'channel')",
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

    if let Some(q) = search {
        let pattern = format!("%{q}%");
        sql.push_str(&format!(" AND name LIKE ?{}", param_values.len() + 1));
        param_values.push(Box::new(pattern));
    }

    sql.push_str(&format!(
        " ORDER BY last_message_at DESC NULLS LAST LIMIT ?{}",
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row::row_to_conversation)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
