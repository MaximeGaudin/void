//! Hook execution log persistence.

use rusqlite::{params, Connection};

use crate::error::DbError;

pub(super) fn insert(
    conn: &Connection,
    log: &crate::hooks::HookLogInsert<'_>,
) -> Result<(), DbError> {
    conn.execute(
        "INSERT INTO hook_logs (hook_name, trigger_type, started_at, duration_ms, success, result, error, message_id, input_prompt, raw_output)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            log.hook_name,
            log.trigger_type,
            log.started_at,
            log.duration_ms,
            log.success as i32,
            log.result,
            log.error,
            log.message_id,
            log.input_prompt,
            log.raw_output,
        ],
    )?;
    Ok(())
}

pub(super) fn list(conn: &Connection, limit: usize) -> Result<Vec<crate::hooks::HookLog>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, hook_name, trigger_type, started_at, duration_ms, success, result, error, message_id, input_prompt, raw_output
         FROM (SELECT * FROM hook_logs ORDER BY started_at DESC LIMIT ?1) ORDER BY started_at ASC",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| {
        Ok(crate::hooks::HookLog {
            id: row.get(0)?,
            hook_name: row.get(1)?,
            trigger_type: row.get(2)?,
            started_at: row.get(3)?,
            duration_ms: row.get(4)?,
            success: row.get::<_, i32>(5)? != 0,
            result: row.get(6)?,
            error: row.get(7)?,
            message_id: row.get(8)?,
            input_prompt: row.get(9)?,
            raw_output: row.get(10)?,
        })
    })?;
    let mut logs = Vec::new();
    for row in rows {
        logs.push(row?);
    }
    Ok(logs)
}

/// Latest execution per hook name (used by `void doctor`).
pub(super) fn last_runs(conn: &Connection) -> Result<Vec<crate::hooks::HookRunSummary>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT h.hook_name, h.started_at, h.success, c.runs
         FROM hook_logs h
         JOIN (SELECT hook_name, MAX(id) AS last_id, COUNT(*) AS runs
               FROM hook_logs GROUP BY hook_name) c
           ON h.id = c.last_id
         ORDER BY h.hook_name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(crate::hooks::HookRunSummary {
            hook_name: row.get(0)?,
            last_started_at: row.get(1)?,
            last_success: row.get::<_, i32>(2)? != 0,
            runs: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Messages ingested (by `synced_at`) after `since`, optionally for one connector.
pub(super) fn count_messages_synced_since(
    conn: &Connection,
    connector: Option<&str>,
    since: i64,
) -> Result<i64, DbError> {
    let count = conn.query_row(
        "SELECT COUNT(*) FROM messages
         WHERE synced_at > ?1 AND (?2 IS NULL OR connector = ?2)",
        params![since, connector],
        |row| row.get(0),
    )?;
    Ok(count)
}
