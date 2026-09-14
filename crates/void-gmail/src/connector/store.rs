//! Read Gmail messages/threads from the local store when a usable body is there.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tracing::debug;
use void_core::db::Database;
use void_core::models::Message;

use crate::api::{
    FileAttachment, GmailMessage, GmailThread, MessageHeader, MessagePart, MessagePartBody,
    MessagePayload,
};
use crate::CONNECTOR_ID;

/// Bodies shorter than this are treated as snippets, not a full read.
pub(super) const MIN_STORED_BODY_CHARS: usize = 200;

/// Upper bound on messages fetched when loading a stored thread.
const MAX_THREAD_MESSAGES: i64 = 500;

pub(super) fn open_store(store_path: &std::path::Path) -> anyhow::Result<Option<Database>> {
    let path = store_path.join("void.db");
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(Database::open(&path)?))
}

pub(super) fn stored_body_is_complete(msg: &Message) -> bool {
    let Some(body) = msg.body.as_deref() else {
        return false;
    };
    if body.is_empty() {
        return false;
    }
    let meta = msg.metadata.as_ref();
    let has_html = meta
        .and_then(|m| m.get("has_html"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    has_html || body.chars().count() >= MIN_STORED_BODY_CHARS
}

pub(super) fn load_stored_message(db: &Database, message_id: &str) -> Option<GmailMessage> {
    let msg = db
        .find_message_by_connector_external_id(CONNECTOR_ID, message_id)
        .ok()
        .flatten()?;
    if !stored_body_is_complete(&msg) {
        return None;
    }
    let thread_id = db
        .get_conversation(&msg.conversation_id)
        .ok()
        .flatten()
        .map(|c| c.external_id)
        .unwrap_or_else(|| msg.conversation_id.clone());
    Some(gmail_message_from_store(&msg, &thread_id))
}

pub(super) fn load_stored_thread(db: &Database, thread_id: &str) -> Option<GmailThread> {
    let conv = db
        .find_conversation_by_connector_external_id(CONNECTOR_ID, thread_id)
        .ok()
        .flatten()?;
    let stored = db
        .list_messages(&conv.id, MAX_THREAD_MESSAGES, None, None)
        .ok()?;
    if stored.is_empty() {
        return None;
    }
    let mut messages = Vec::with_capacity(stored.len());
    for msg in stored {
        if !stored_body_is_complete(&msg) {
            return None;
        }
        messages.push(gmail_message_from_store(&msg, thread_id));
    }
    debug!(
        thread_id,
        count = messages.len(),
        "gmail: serving thread from local store"
    );
    Some(GmailThread {
        id: Some(thread_id.to_string()),
        snippet: messages.first().and_then(|m| m.snippet.clone()),
        messages: Some(messages),
    })
}

pub(super) fn gmail_message_from_store(msg: &Message, thread_id: &str) -> GmailMessage {
    let subject = msg
        .metadata
        .as_ref()
        .and_then(|m| m.get("subject"))
        .and_then(|v| v.as_str())
        .unwrap_or("(no subject)")
        .to_string();
    let snippet = msg
        .metadata
        .as_ref()
        .and_then(|m| m.get("snippet"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            msg.body
                .as_ref()
                .map(|b| b.chars().take(120).collect::<String>())
        });
    let from = match &msg.sender_name {
        Some(name) if !name.is_empty() => format!("{name} <{}>", msg.sender),
        _ => msg.sender.clone(),
    };
    let body_text = msg.body.clone().unwrap_or_default();
    let encoded = URL_SAFE_NO_PAD.encode(body_text.as_bytes());

    let mut parts = vec![MessagePart {
        mime_type: Some("text/plain".into()),
        filename: None,
        headers: None,
        body: Some(MessagePartBody {
            data: Some(encoded),
            size: Some(body_text.len() as u64),
            attachment_id: None,
        }),
        parts: None,
    }];
    if let Some(atts) = msg
        .metadata
        .as_ref()
        .and_then(|m| m.get("attachments"))
        .and_then(|v| serde_json::from_value::<Vec<FileAttachment>>(v.clone()).ok())
    {
        for att in atts {
            parts.push(MessagePart {
                mime_type: att.mime_type,
                filename: Some(att.filename),
                headers: None,
                body: Some(MessagePartBody {
                    data: None,
                    size: att.size,
                    attachment_id: Some(att.attachment_id),
                }),
                parts: None,
            });
        }
    }

    let labels = if msg.is_archived {
        None
    } else {
        Some(vec!["INBOX".into()])
    };

    GmailMessage {
        id: Some(msg.external_id.clone()),
        thread_id: Some(thread_id.to_string()),
        snippet,
        internal_date: Some((msg.timestamp * 1000).to_string()),
        label_ids: labels,
        payload: Some(MessagePayload {
            mime_type: Some("multipart/mixed".into()),
            headers: Some(vec![
                MessageHeader {
                    name: "From".into(),
                    value: from,
                },
                MessageHeader {
                    name: "Subject".into(),
                    value: subject,
                },
            ]),
            body: None,
            parts: Some(parts),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_core::models::{Conversation, ConversationKind};

    fn seed(body: &str, has_html: bool) -> Message {
        let mut metadata = serde_json::Map::new();
        metadata.insert("subject".into(), serde_json::json!("Hello"));
        if has_html {
            metadata.insert("has_html".into(), serde_json::json!(true));
        }
        Message {
            id: "id".into(),
            conversation_id: "c".into(),
            connection_id: "acct".into(),
            connector: "gmail".into(),
            external_id: "m1".into(),
            sender: "a@b.com".into(),
            sender_name: Some("Ann".into()),
            sender_avatar_url: None,
            body: Some(body.into()),
            timestamp: 1_700_000_000,
            synced_at: None,
            is_archived: false,
            is_saved: false,
            reply_to_id: None,
            media_type: None,
            metadata: Some(serde_json::Value::Object(metadata)),
            context_id: None,
            context: None,
        }
    }

    #[test]
    fn short_body_is_not_complete_unless_html() {
        assert!(!stored_body_is_complete(&seed("short snippet", false)));
        assert!(stored_body_is_complete(&seed("short snippet", true)));
        let long = "x".repeat(MIN_STORED_BODY_CHARS);
        assert!(stored_body_is_complete(&seed(&long, false)));
        assert!(!stored_body_is_complete(&seed("", false)));
    }

    #[test]
    fn from_store_round_trips_headers_and_body() {
        let body = "x".repeat(MIN_STORED_BODY_CHARS);
        let gm = gmail_message_from_store(&seed(&body, false), "t1");
        assert_eq!(gm.id.as_deref(), Some("m1"));
        assert_eq!(gm.thread_id.as_deref(), Some("t1"));
        assert_eq!(gm.get_header("From").as_deref(), Some("Ann <a@b.com>"));
        assert_eq!(gm.get_header("Subject").as_deref(), Some("Hello"));
        assert_eq!(gm.text_body().as_deref(), Some(body.as_str()));
        assert_eq!(gm.label_ids, Some(vec!["INBOX".into()]));
    }

    #[test]
    fn load_thread_requires_every_message_complete() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_conversation(&Conversation {
            id: "c1".into(),
            connection_id: "acct".into(),
            connector: "gmail".into(),
            external_id: "t1".into(),
            name: Some("Hello".into()),
            kind: ConversationKind::Thread,
            last_message_at: None,
            unread_count: 0,
            is_muted: false,
            metadata: None,
        })
        .unwrap();
        let mut complete = seed(&"y".repeat(MIN_STORED_BODY_CHARS), false);
        complete.id = "c1-m1".into();
        complete.conversation_id = "c1".into();
        complete.external_id = "m1".into();
        db.upsert_message(&complete).unwrap();
        let thread = load_stored_thread(&db, "t1").expect("complete thread");
        assert_eq!(thread.messages.as_ref().map(|m| m.len()), Some(1));

        let mut stub = seed("tiny", false);
        stub.id = "c1-m2".into();
        stub.conversation_id = "c1".into();
        stub.external_id = "m2".into();
        db.upsert_message(&stub).unwrap();
        assert!(load_stored_thread(&db, "t1").is_none());
    }
}
