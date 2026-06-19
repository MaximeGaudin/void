use rusqlite::{params, Connection, OptionalExtension};

use super::super::row;
use super::{DEDUP_CONTEXT_CLAUSE, DEDUP_CONTEXT_CLAUSE_UNARCHIVED};
use crate::error::DbError;
use crate::models::Message;

pub fn list_for_conversation(
    conn: &Connection,
    conversation_id: &str,
    limit: i64,
    offset: i64,
    since: Option<i64>,
    until: Option<i64>,
    dedup_context: bool,
) -> Result<Vec<Message>, DbError> {
    let suffix_pattern = format!("%-{conversation_id}");
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE (conversation_id = ?1 OR conversation_id LIKE ?2)",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(conversation_id.to_string()),
        Box::new(suffix_pattern),
    ];

    if dedup_context {
        sql.push_str(DEDUP_CONTEXT_CLAUSE);
    }
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

pub fn count_for_conversation(
    conn: &Connection,
    conversation_id: &str,
    since: Option<i64>,
    until: Option<i64>,
    dedup_context: bool,
) -> Result<i64, DbError> {
    let suffix_pattern = format!("%-{conversation_id}");
    let mut sql = String::from(
        "SELECT COUNT(*) FROM messages WHERE (conversation_id = ?1 OR conversation_id LIKE ?2)",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
        Box::new(conversation_id.to_string()),
        Box::new(suffix_pattern),
    ];

    if dedup_context {
        sql.push_str(DEDUP_CONTEXT_CLAUSE);
    }
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

pub fn get(conn: &Connection, id: &str) -> Result<Option<Message>, DbError> {
    let suffix_pattern = format!("%-{id}");
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE id = ?1 OR id LIKE ?2",
        params![id, suffix_pattern],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}

pub fn latest_timestamp(
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

#[allow(clippy::too_many_arguments)]
pub fn list_recent(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    limit: i64,
    offset: i64,
    include_archived: bool,
    include_muted: bool,
    dedup_context: bool,
) -> Result<Vec<Message>, DbError> {
    let mut sql = String::from(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
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
    if dedup_context {
        if include_archived {
            sql.push_str(DEDUP_CONTEXT_CLAUSE);
        } else {
            sql.push_str(DEDUP_CONTEXT_CLAUSE_UNARCHIVED);
        }
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

pub fn count_recent(
    conn: &Connection,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    include_archived: bool,
    include_muted: bool,
    dedup_context: bool,
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
    if dedup_context {
        if include_archived {
            sql.push_str(DEDUP_CONTEXT_CLAUSE);
        } else {
            sql.push_str(DEDUP_CONTEXT_CLAUSE_UNARCHIVED);
        }
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
