//! Mapping functions to convert Slack API types to void-core models.

use std::collections::HashMap;

use void_core::models::{Conversation, ConversationKind, Message};

use crate::api::{SlackConversation, SlackMessage, SlackReaction};

pub(crate) fn map_conversation(
    conv: &SlackConversation,
    connection_id: &str,
    user_cache: &HashMap<String, String>,
) -> Conversation {
    let kind = if conv.is_im.unwrap_or(false) {
        ConversationKind::Dm
    } else if conv.is_group.unwrap_or(false) || conv.is_mpim.unwrap_or(false) {
        ConversationKind::Group
    } else {
        ConversationKind::Channel
    };

    let name = if conv.is_im.unwrap_or(false) {
        conv.user
            .as_deref()
            .and_then(|uid| user_cache.get(uid).cloned())
            .or_else(|| conv.user.clone())
            .unwrap_or_else(|| conv.id.clone())
    } else {
        conv.name.clone().unwrap_or_else(|| conv.id.clone())
    };

    Conversation {
        id: format!("{}-{}", connection_id, conv.id),
        connection_id: connection_id.to_string(),
        connector: "slack".into(),
        external_id: conv.id.clone(),
        name: Some(name),
        kind,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    }
}

pub(crate) fn map_message_cached(
    msg: &SlackMessage,
    conv: &SlackConversation,
    conversation_id: &str,
    connection_id: &str,
    user_cache: &HashMap<String, String>,
) -> Option<Message> {
    if msg.subtype.is_some() {
        return None;
    }

    let sender = msg.user.clone().unwrap_or_else(|| "unknown".into());
    let sender_name = user_cache
        .get(&sender)
        .cloned()
        .unwrap_or_else(|| sender.clone());

    let mut metadata = build_metadata(conv, &msg.reactions, user_cache);
    let text = msg.text.clone().unwrap_or_default();

    let (body, media_type) = if !msg.files.is_empty() {
        let file_descriptions: Vec<String> = msg
            .files
            .iter()
            .map(|f| {
                let name = f.name.as_deref().or(f.title.as_deref()).unwrap_or("file");
                let icon = match f.mimetype.as_deref() {
                    Some(m) if m.starts_with("image/") => "🖼️",
                    Some(m) if m.starts_with("video/") => "🎬",
                    Some(m) if m.starts_with("audio/") => "🎵",
                    _ => "📎",
                };
                format!("{icon} {name}")
            })
            .collect();

        if let Some(meta) = metadata.as_mut() {
            let files_json: Vec<serde_json::Value> = msg
                .files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "id": f.id,
                        "name": f.name,
                        "title": f.title,
                        "mimetype": f.mimetype,
                        "filetype": f.filetype,
                        "size": f.size,
                        "url_private": f.url_private,
                        "permalink": f.permalink,
                    })
                })
                .collect();
            meta["files"] = serde_json::Value::Array(files_json);
        }

        let first_mime = msg.files[0].mimetype.as_deref();
        let mtype = first_mime.map(|m| {
            if m.starts_with("image/") {
                "image".to_string()
            } else if m.starts_with("video/") {
                "video".to_string()
            } else if m.starts_with("audio/") {
                "audio".to_string()
            } else {
                "file".to_string()
            }
        });

        let body = if text.is_empty() {
            file_descriptions.join(", ")
        } else {
            format!("{text}\n{}", file_descriptions.join(", "))
        };
        (Some(body), mtype)
    } else if !msg.attachments.is_empty() && text.is_empty() {
        let fallback: Vec<String> = msg
            .attachments
            .iter()
            .filter_map(|a| {
                a.title
                    .clone()
                    .or_else(|| a.fallback.clone())
                    .or_else(|| a.text.clone())
            })
            .collect();
        if fallback.is_empty() {
            (Some(text), None)
        } else {
            (Some(fallback.join("\n")), None)
        }
    } else {
        (if text.is_empty() { None } else { Some(text) }, None)
    };

    let context_id = msg
        .thread_ts
        .as_ref()
        .map(|thread_ts| format!("{connection_id}-thread-{thread_ts}"));

    Some(Message {
        id: format!("{connection_id}-{}", msg.ts),
        conversation_id: conversation_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "slack".into(),
        external_id: msg.ts.clone(),
        sender: sender.clone(),
        sender_name: Some(sender_name),
        body,
        timestamp: parse_ts(&msg.ts).unwrap_or(0),
        synced_at: None,
        is_archived: false,
        reply_to_id: msg
            .thread_ts
            .as_ref()
            .map(|ts| format!("{connection_id}-{ts}")),
        media_type,
        metadata,
        context_id,
        context: None,
    })
}

pub(crate) fn build_metadata(
    conv: &SlackConversation,
    reactions: &[SlackReaction],
    user_cache: &HashMap<String, String>,
) -> Option<serde_json::Value> {
    let kind = if conv.is_im.unwrap_or(false) {
        "dm"
    } else if conv.is_mpim.unwrap_or(false) {
        "group_dm"
    } else if conv.is_group.unwrap_or(false) || conv.is_private.unwrap_or(false) {
        "private_channel"
    } else {
        "channel"
    };

    let channel_name = if conv.is_im.unwrap_or(false) {
        conv.user
            .as_deref()
            .and_then(|uid| user_cache.get(uid).map(|s| s.as_str()))
            .or(conv.user.as_deref())
            .unwrap_or(&conv.id)
    } else {
        conv.name.as_deref().unwrap_or(&conv.id)
    };

    let mut meta = serde_json::json!({
        "channel_id": conv.id,
        "channel_name": channel_name,
        "channel_kind": kind,
        "is_private": conv.is_private.unwrap_or(false) || conv.is_im.unwrap_or(false) || conv.is_mpim.unwrap_or(false),
    });

    if !reactions.is_empty() {
        let r: Vec<serde_json::Value> = reactions
            .iter()
            .map(|r| serde_json::json!({"name": r.name, "count": r.count}))
            .collect();
        meta["reactions"] = serde_json::Value::Array(r);
    }

    Some(meta)
}

pub(crate) fn parse_ts(ts: &str) -> Option<i64> {
    ts.split('.').next()?.parse().ok()
}

const TIME_WINDOW_SECS: i64 = 3600;

/// Assign `context_id` to non-threaded messages using a 1-hour time-window grouping.
/// Messages must be sorted by timestamp ASC before calling.
pub(crate) fn assign_time_window_context(
    messages: &mut [Message],
    connection_id: &str,
    channel_id: &str,
) {
    let mut current_group_ts: Option<String> = None;
    let mut last_ts: i64 = 0;

    for msg in messages.iter_mut() {
        if msg.context_id.is_some() {
            last_ts = 0;
            current_group_ts = None;
            continue;
        }

        if current_group_ts.is_some() && (msg.timestamp - last_ts) <= TIME_WINDOW_SECS {
            msg.context_id = current_group_ts.clone();
        } else {
            let group_id = format!("{connection_id}-group-{channel_id}-{}", msg.timestamp);
            msg.context_id = Some(group_id.clone());
            current_group_ts = Some(group_id);
        }
        last_ts = msg.timestamp;
    }
}
