use rusqlite::{params, Connection};
use serde_json::Value;

use crate::error::DbError;

const DELETE_BATCH: usize = 1000;

/// Rows removed by [`prune_before`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneResult {
    pub messages_deleted: usize,
    pub conversations_deleted: usize,
    /// `local_path` values from deleted message metadata. Caller removes the files.
    pub file_paths: Vec<String>,
}

/// Delete messages with `timestamp < before_ts`. Saved messages stay.
///
/// Conversations left with no messages are deleted too. Full-text rows follow
/// the existing delete triggers.
pub fn prune_before(conn: &Connection, before_ts: i64) -> Result<PruneResult, DbError> {
    let file_paths = local_paths_before(conn, before_ts)?;

    let mut messages_deleted = 0usize;
    loop {
        let n = conn.execute(
            "DELETE FROM messages WHERE rowid IN (
                SELECT rowid FROM messages WHERE timestamp < ?1 AND is_saved = 0 LIMIT ?2
            )",
            params![before_ts, DELETE_BATCH as i64],
        )?;
        if n == 0 {
            break;
        }
        messages_deleted += n;
    }

    let conversations_deleted = conn.execute(
        "DELETE FROM conversations WHERE NOT EXISTS (
            SELECT 1 FROM messages WHERE messages.conversation_id = conversations.id
        )",
        [],
    )?;

    Ok(PruneResult {
        messages_deleted,
        conversations_deleted,
        file_paths,
    })
}

fn local_paths_before(conn: &Connection, before_ts: i64) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT metadata FROM messages
         WHERE timestamp < ?1 AND is_saved = 0 AND metadata LIKE '%local_path%'",
    )?;
    let rows = stmt.query_map(params![before_ts], |row| row.get::<_, String>(0))?;
    let mut paths = Vec::new();
    for row in rows {
        let Ok(value) = serde_json::from_str::<Value>(&row?) else {
            continue;
        };
        collect_local_paths(&value, &mut paths);
    }
    Ok(paths)
}

fn collect_local_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(path) = map.get("local_path").and_then(|v| v.as_str()) {
                if !path.is_empty() {
                    out.push(path.to_string());
                }
            }
            for child in map.values() {
                collect_local_paths(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_local_paths(child, out);
            }
        }
        _ => {}
    }
}
