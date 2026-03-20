//! Calendar event rows and connector-wide deletes.

use rusqlite::{params, Connection};
use tracing::debug;

use super::row;
use crate::error::DbError;
use crate::models::CalendarEvent;

pub(super) fn upsert(conn: &Connection, event: &CalendarEvent) -> Result<(), DbError> {
    debug!(event_id = %event.id, "upserting event");
    conn.execute(
        "INSERT INTO events (id, connection_id, connector, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT(connection_id, external_id) DO UPDATE SET
            title = excluded.title,
            connector = excluded.connector,
            description = excluded.description,
            location = excluded.location,
            start_at = excluded.start_at,
            end_at = excluded.end_at,
            all_day = excluded.all_day,
            attendees = excluded.attendees,
            status = excluded.status,
            calendar_name = excluded.calendar_name,
            meet_link = excluded.meet_link,
            metadata = excluded.metadata",
        params![
            event.id,
            event.connection_id,
            event.connector,
            event.external_id,
            event.title,
            event.description,
            event.location,
            event.start_at,
            event.end_at,
            event.all_day as i32,
            event.attendees.as_ref().map(|v| v.to_string()),
            event.status,
            event.calendar_name,
            event.meet_link,
            event.metadata.as_ref().map(|v| v.to_string()),
        ],
    )?;
    Ok(())
}

pub(super) fn delete(
    conn: &Connection,
    connection_id: &str,
    external_id: &str,
) -> Result<bool, DbError> {
    debug!(connection_id, external_id, "deleting event");
    let deleted = conn.execute(
        "DELETE FROM events WHERE connection_id = ?1 AND external_id = ?2",
        params![connection_id, external_id],
    )?;
    Ok(deleted > 0)
}

/// Delete all data for a connector type. Returns (messages, conversations, events, sync_states).
pub(super) fn clear_connector_data(
    conn: &Connection,
    connector_type: &str,
) -> Result<(usize, usize, usize, usize), DbError> {
    let connection_ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT connection_id FROM conversations WHERE connector = ?1
             UNION SELECT DISTINCT connection_id FROM messages WHERE connector = ?1
             UNION SELECT DISTINCT connection_id FROM events WHERE connector = ?1",
        )?;
        let rows = stmt.query_map(params![connector_type], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };

    let msgs = conn.execute(
        "DELETE FROM messages WHERE connector = ?1",
        params![connector_type],
    )?;
    let convs = conn.execute(
        "DELETE FROM conversations WHERE connector = ?1",
        params![connector_type],
    )?;
    let evts = conn.execute(
        "DELETE FROM events WHERE connector = ?1",
        params![connector_type],
    )?;

    let mut sync_deleted = 0usize;
    for aid in &connection_ids {
        sync_deleted += conn.execute(
            "DELETE FROM sync_state WHERE connection_id = ?1",
            params![aid],
        )?;
    }

    Ok((msgs, convs, evts, sync_deleted))
}

pub(super) fn list(
    conn: &Connection,
    from: Option<i64>,
    to: Option<i64>,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
    limit: i64,
) -> Result<Vec<CalendarEvent>, DbError> {
    let mut sql = String::from(
        "SELECT id, connection_id, connector, external_id, title, description, location, start_at, end_at, all_day, attendees, status, calendar_name, meet_link, metadata FROM events WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(f) = from {
        sql.push_str(&format!(" AND end_at >= ?{}", param_values.len() + 1));
        param_values.push(Box::new(f));
    }
    if let Some(t) = to {
        sql.push_str(&format!(" AND start_at <= ?{}", param_values.len() + 1));
        param_values.push(Box::new(t));
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
        " ORDER BY start_at ASC LIMIT ?{}",
        param_values.len() + 1
    ));
    param_values.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), row::row_to_event)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}
