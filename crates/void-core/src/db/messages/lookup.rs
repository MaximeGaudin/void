use rusqlite::{params, Connection, OptionalExtension};

use super::super::row;
use crate::error::DbError;
use crate::models::Message;

pub fn find_by_external_id(
    conn: &Connection,
    connection_id: &str,
    external_id: &str,
) -> Result<Option<Message>, DbError> {
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE connection_id = ?1 AND external_id = ?2",
        params![connection_id, external_id],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}

/// Find a Slack message by its native Slack identifiers: the channel's
/// `external_id` (e.g. `C08UDH5JE57`) and the message `ts` (e.g.
/// `1776936528.857609`).
///
/// Searches across all Slack connections — the void connection ID does not
/// have to match the Slack workspace subdomain, so we must route by the
/// (channel, ts) pair which is globally unique for Slack.
pub fn find_by_slack_link(
    conn: &Connection,
    channel_external_id: &str,
    message_ts: &str,
) -> Result<Option<Message>, DbError> {
    conn.query_row(
        "SELECT m.id, m.conversation_id, m.connection_id, m.connector, m.external_id, m.sender, m.sender_name, m.sender_avatar_url, m.body, m.timestamp, m.synced_at, m.is_archived, m.reply_to_id, m.media_type, m.metadata, m.context_id, m.is_saved
         FROM messages m
         JOIN conversations c ON m.conversation_id = c.id
         WHERE m.connector = 'slack'
           AND m.external_id = ?1
           AND c.external_id = ?2
         LIMIT 1",
        params![message_ts, channel_external_id],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}

/// Find a Slack conversation by its native channel/DM `external_id`,
/// searching across all Slack connections.
pub fn find_slack_conversation_by_external_id(
    conn: &Connection,
    channel_external_id: &str,
) -> Result<Option<crate::models::Conversation>, DbError> {
    conn.query_row(
        "SELECT id, connection_id, connector, external_id, name, kind, last_message_at, unread_count, is_muted, metadata
         FROM conversations
         WHERE connector = 'slack' AND external_id = ?1
         LIMIT 1",
        params![channel_external_id],
        row::row_to_conversation,
    )
    .optional()
    .map_err(Into::into)
}

pub fn last_in_conversation(
    conn: &Connection,
    conversation_id: &str,
) -> Result<Option<Message>, DbError> {
    conn.query_row(
        "SELECT id, conversation_id, connection_id, connector, external_id, sender, sender_name, sender_avatar_url, body, timestamp, synced_at, is_archived, reply_to_id, media_type, metadata, context_id, is_saved
         FROM messages WHERE conversation_id = ?1 ORDER BY timestamp DESC LIMIT 1",
        params![conversation_id],
        row::row_to_message,
    )
    .optional()
    .map_err(Into::into)
}
