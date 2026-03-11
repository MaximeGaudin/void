//! Row conversion helpers for database queries.

use rusqlite::Row;

use crate::models::{CalendarEvent, Conversation, ConversationKind, Message};

pub(crate) fn parse_kind(s: &str) -> ConversationKind {
    match s {
        "dm" => ConversationKind::Dm,
        "group" => ConversationKind::Group,
        "channel" => ConversationKind::Channel,
        "thread" => ConversationKind::Thread,
        _ => ConversationKind::Dm,
    }
}

pub(crate) fn parse_json_opt(s: Option<String>) -> Option<serde_json::Value> {
    s.and_then(|v| serde_json::from_str(&v).ok())
}

pub(crate) fn row_to_conversation(row: &Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        account_id: row.get(1)?,
        connector: row.get(2)?,
        external_id: row.get(3)?,
        name: row.get(4)?,
        kind: parse_kind(&row.get::<_, String>(5)?),
        last_message_at: row.get(6)?,
        unread_count: row.get(7)?,
        metadata: parse_json_opt(row.get(8)?),
    })
}

pub(crate) fn row_to_message(row: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        account_id: row.get(2)?,
        connector: row.get(3)?,
        external_id: row.get(4)?,
        sender: row.get(5)?,
        sender_name: row.get(6)?,
        body: row.get(7)?,
        timestamp: row.get(8)?,
        synced_at: row.get(9)?,
        is_from_me: row.get::<_, i32>(10)? != 0,
        is_read: row.get::<_, i32>(11)? != 0,
        is_archived: row.get::<_, i32>(12)? != 0,
        reply_to_id: row.get(13)?,
        media_type: row.get(14)?,
        metadata: parse_json_opt(row.get(15)?),
    })
}

pub(crate) fn row_to_event(row: &Row) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        account_id: row.get(1)?,
        connector: row.get(2)?,
        external_id: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        location: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        all_day: row.get::<_, i32>(9)? != 0,
        attendees: parse_json_opt(row.get(10)?),
        status: row.get(11)?,
        calendar_name: row.get(12)?,
        meet_link: row.get(13)?,
        metadata: parse_json_opt(row.get(14)?),
    })
}
