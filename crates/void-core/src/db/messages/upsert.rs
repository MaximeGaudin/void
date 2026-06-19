use rusqlite::{params, Connection};
use tracing::debug;

use crate::error::DbError;
use crate::models::Message;

pub fn message_exists(
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
pub fn upsert_row(conn: &Connection, msg: &Message) -> Result<bool, DbError> {
    debug!(message_id = %msg.id, "upserting message");
    let is_new = !message_exists(conn, &msg.connection_id, &msg.external_id)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "INSERT INTO messages (id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, is_saved, reply_to_id, media_type, metadata, context_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(connection_id, external_id) DO UPDATE SET
            body = excluded.body,
            connector = excluded.connector,
            sender_name = excluded.sender_name,
            sender_avatar_url = COALESCE(excluded.sender_avatar_url, sender_avatar_url),
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
            msg.sender_avatar_url,
            msg.body,
            msg.timestamp,
            msg.synced_at.unwrap_or(now),
            msg.is_archived as i32,
            msg.is_saved as i32,
            msg.reply_to_id,
            msg.media_type,
            msg.metadata.as_ref().map(|v| v.to_string()),
            msg.context_id,
        ],
    )?;
    Ok(is_new)
}
