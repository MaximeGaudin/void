use crate::models::{Conversation, ConversationKind, Message};

pub fn make_conversation(id: &str, connection_id: &str, ext_id: &str) -> Conversation {
    Conversation {
        id: id.into(),
        connection_id: connection_id.into(),
        connector: "slack".into(),
        external_id: ext_id.into(),
        name: Some(format!("Conv {id}")),
        kind: ConversationKind::Dm,
        last_message_at: Some(1_700_000_000),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    }
}

pub fn make_conversation_named(
    id: &str,
    ext_id: &str,
    name: &str,
    kind: ConversationKind,
) -> Conversation {
    Conversation {
        id: id.into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: ext_id.into(),
        name: Some(name.into()),
        kind,
        last_message_at: Some(1_700_000_000),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    }
}

pub fn make_message(id: &str, conv_id: &str, connection_id: &str, body: &str, ts: i64) -> Message {
    Message {
        id: id.into(),
        conversation_id: conv_id.into(),
        connection_id: connection_id.into(),
        connector: "slack".into(),
        external_id: format!("ext-{id}"),
        sender: "sender@test".into(),
        sender_name: Some("Test Sender".into()),
        sender_avatar_url: None,
        body: Some(body.into()),
        timestamp: ts,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    }
}

pub fn make_message_with_sender(
    id: &str,
    conv_id: &str,
    sender: &str,
    body: &str,
    ts: i64,
) -> Message {
    Message {
        id: id.into(),
        conversation_id: conv_id.into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: format!("ext-{id}"),
        sender: sender.into(),
        sender_name: Some("Alice Example".into()),
        sender_avatar_url: None,
        body: Some(body.into()),
        timestamp: ts,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    }
}
