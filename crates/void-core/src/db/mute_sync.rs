//! Mute flags and sync_state / connection rename.

use rusqlite::{params, Connection, OptionalExtension};
use tracing::debug;

use crate::error::DbError;

pub(super) fn update_conversation_mute(
    conn: &Connection,
    conversation_id: &str,
    is_muted: bool,
) -> Result<bool, DbError> {
    debug!(
        conversation_id,
        is_muted, "updating conversation mute state"
    );
    let updated = conn.execute(
        "UPDATE conversations SET is_muted = ?2 WHERE id = ?1",
        params![conversation_id, is_muted as i32],
    )?;
    Ok(updated > 0)
}

pub(super) fn set_mute_by_external_id(
    conn: &Connection,
    connection_id: &str,
    external_id: &str,
    is_muted: bool,
) -> Result<bool, DbError> {
    debug!(
        connection_id,
        external_id, is_muted, "setting mute by external_id"
    );
    let updated = conn.execute(
        "UPDATE conversations SET is_muted = ?3 WHERE connection_id = ?1 AND external_id = ?2",
        params![connection_id, external_id, is_muted as i32],
    )?;
    Ok(updated > 0)
}

/// Auto-mute conversations whose name or external_id matches any of the given
/// patterns (case-insensitive substring match). Only affects non-muted
/// conversations for the specified connection. Returns the number of newly muted
/// conversations.
pub(super) fn auto_mute_matching_conversations(
    conn: &Connection,
    connection_id: &str,
    patterns: &[String],
) -> Result<usize, DbError> {
    if patterns.is_empty() {
        return Ok(0);
    }

    let mut sql = String::from(
        "UPDATE conversations SET is_muted = 1
         WHERE connection_id = ?1 AND is_muted = 0 AND (",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(connection_id.to_string())];

    for (i, pattern) in patterns.iter().enumerate() {
        if i > 0 {
            sql.push_str(" OR ");
        }
        let like = format!("%{}%", pattern.to_lowercase());
        let idx = param_values.len() + 1;
        sql.push_str(&format!(
            "LOWER(COALESCE(name, '')) LIKE ?{idx} OR LOWER(external_id) LIKE ?{idx}"
        ));
        param_values.push(Box::new(like));
    }
    sql.push(')');

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let updated = stmt.execute(params_ref.as_slice())?;

    if updated > 0 {
        debug!(
            connection_id,
            count = updated,
            "auto-muted conversations matching ignore patterns"
        );
    }
    Ok(updated)
}

pub(super) fn list_sync_states(
    conn: &Connection,
) -> Result<Vec<(String, String, String)>, DbError> {
    let mut stmt = conn
        .prepare("SELECT connection_id, key, value FROM sync_state ORDER BY connection_id, key")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(super) fn get_sync_state(
    conn: &Connection,
    connection_id: &str,
    key: &str,
) -> Result<Option<String>, DbError> {
    conn.query_row(
        "SELECT value FROM sync_state WHERE connection_id = ?1 AND key = ?2",
        params![connection_id, key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

pub(super) fn set_sync_state(
    conn: &Connection,
    connection_id: &str,
    key: &str,
    value: &str,
) -> Result<(), DbError> {
    debug!(connection_id, key, "setting sync state");
    conn.execute(
        "INSERT INTO sync_state (connection_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(connection_id, key) DO UPDATE SET value = excluded.value",
        params![connection_id, key, value],
    )?;
    Ok(())
}

pub(super) fn rename_connection(
    conn: &Connection,
    old_id: &str,
    new_id: &str,
) -> Result<(), DbError> {
    // Temporarily disable FKs: we update conversation ids first, which orphans
    // messages; then we update messages. With FKs on, the order would violate.
    conn.execute("PRAGMA foreign_keys = OFF", [])?;
    conn.execute(
        "UPDATE sync_state SET connection_id = ?2 WHERE connection_id = ?1",
        params![old_id, new_id],
    )?;
    conn.execute(
        "UPDATE conversations SET connection_id = ?2, id = REPLACE(id, ?1, ?2) WHERE connection_id = ?1",
        params![old_id, new_id],
    )?;
    conn.execute(
        "UPDATE messages SET connection_id = ?2, id = REPLACE(id, ?1, ?2), conversation_id = REPLACE(conversation_id, ?1, ?2) WHERE connection_id = ?1",
        params![old_id, new_id],
    )?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(())
}
