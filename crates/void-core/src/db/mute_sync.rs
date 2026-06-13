//! Mute flags and sync_state / connection rename.

use rusqlite::{params, Connection, OptionalExtension};
use tracing::debug;

use crate::config::conversation_matches_ignore;
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

/// Sync conversation mute flags from config ignore patterns for one connection.
/// Returns `(newly_muted, newly_unmuted)` counts.
pub(super) fn sync_ignore_conversations(
    conn: &Connection,
    connection_id: &str,
    patterns: &[String],
) -> Result<(usize, usize), DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, external_id, is_muted FROM conversations WHERE connection_id = ?1",
    )?;
    let rows = stmt.query_map(params![connection_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i32>(3)? != 0,
        ))
    })?;

    let mut newly_muted = 0;
    let mut newly_unmuted = 0;
    for row in rows {
        let (id, name, external_id, is_muted) = row?;
        let should_mute = conversation_matches_ignore(name.as_deref(), &external_id, patterns);
        if should_mute == is_muted {
            continue;
        }
        update_conversation_mute(conn, &id, should_mute)?;
        if should_mute {
            newly_muted += 1;
        } else {
            newly_unmuted += 1;
        }
    }

    if newly_muted > 0 || newly_unmuted > 0 {
        debug!(
            connection_id,
            newly_muted,
            newly_unmuted,
            "synced conversation mute flags from config ignore patterns"
        );
    }
    Ok((newly_muted, newly_unmuted))
}

/// Auto-mute conversations whose name or external_id matches any of the given
/// patterns (case-insensitive substring match). Only affects non-muted
/// conversations for the specified connection. Returns the number of newly muted
/// conversations.
#[allow(dead_code)]
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

/// WhatsApp sync briefly stored messages under the account JID instead of the
/// config connection name. Remap JID-based rows onto the canonical config id
/// when unambiguous (single non-JID whatsapp connection in the DB).
pub(super) fn migrate_whatsapp_jid_connections(conn: &Connection) -> Result<(), DbError> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT connection_id FROM messages
         WHERE connector = 'whatsapp' AND instr(connection_id, '@') > 0",
    )?;
    let jid_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(Result::ok)
        .collect();
    if jid_ids.is_empty() {
        return Ok(());
    }

    let mut canonical_ids: Vec<String> = conn
        .prepare(
            "SELECT DISTINCT connection_id FROM (
                SELECT connection_id FROM messages WHERE connector = 'whatsapp'
                UNION
                SELECT connection_id FROM conversations WHERE connector = 'whatsapp'
             ) WHERE instr(connection_id, '@') = 0",
        )?
        .query_map([], |row| row.get(0))?
        .filter_map(Result::ok)
        .collect();
    canonical_ids.sort();
    canonical_ids.dedup();

    let Some(canonical) = (canonical_ids.len() == 1).then(|| canonical_ids[0].clone()) else {
        debug!(
            jid_count = jid_ids.len(),
            config_count = canonical_ids.len(),
            "skipping WhatsApp JID connection migration (ambiguous or no canonical id)"
        );
        return Ok(());
    };

    for jid_id in jid_ids {
        merge_connection_id(conn, &jid_id, &canonical)?;
    }
    Ok(())
}

/// Like `rename_connection`, but drops rows whose target id already exists.
fn merge_connection_id(conn: &Connection, old_id: &str, new_id: &str) -> Result<(), DbError> {
    debug!(old_id, new_id, "merging WhatsApp JID connection id");
    conn.execute("PRAGMA foreign_keys = OFF", [])?;

    let mut stmt = conn
        .prepare("SELECT id, conversation_id, context_id FROM messages WHERE connection_id = ?1")?;
    let rows: Vec<(String, String, Option<String>)> = stmt
        .query_map(params![old_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .filter_map(Result::ok)
        .collect();

    for (msg_id, conv_id, context_id) in rows {
        let new_msg_id = msg_id.replace(old_id, new_id);
        let new_conv_id = conv_id.replace(old_id, new_id);
        let new_context_id = context_id.map(|c| c.replace(old_id, new_id));

        let target_exists = conn
            .query_row(
                "SELECT 1 FROM messages WHERE id = ?1",
                params![new_msg_id],
                |_| Ok(()),
            )
            .is_ok();

        if target_exists {
            conn.execute("DELETE FROM messages WHERE id = ?1", params![msg_id])?;
        } else {
            conn.execute(
                "UPDATE messages SET id = ?1, connection_id = ?2, conversation_id = ?3, context_id = ?4 WHERE id = ?5",
                params![new_msg_id, new_id, new_conv_id, new_context_id, msg_id],
            )?;
        }
    }

    let mut stmt = conn.prepare("SELECT id FROM conversations WHERE connection_id = ?1")?;
    let conv_ids: Vec<String> = stmt
        .query_map(params![old_id], |row| row.get(0))?
        .filter_map(Result::ok)
        .collect();

    for conv_id in conv_ids {
        let new_conv_id = conv_id.replace(old_id, new_id);
        let target_exists = conn
            .query_row(
                "SELECT 1 FROM conversations WHERE id = ?1",
                params![new_conv_id],
                |_| Ok(()),
            )
            .is_ok();

        if target_exists {
            conn.execute("DELETE FROM conversations WHERE id = ?1", params![conv_id])?;
        } else {
            conn.execute(
                "UPDATE conversations SET id = ?1, connection_id = ?2 WHERE id = ?3",
                params![new_conv_id, new_id, conv_id],
            )?;
        }
    }

    conn.execute(
        "UPDATE sync_state SET connection_id = ?2 WHERE connection_id = ?1",
        params![old_id, new_id],
    )?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(())
}
