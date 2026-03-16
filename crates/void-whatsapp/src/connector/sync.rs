//! History sync, message handling, and QR rendering.

use tracing::{debug, info};

use wa_rs::proto_helpers::MessageExt;
use wa_rs::types::message::MessageInfo;
use wa_rs_proto::whatsapp::{HistorySync, Message as WaMessage};

use void_core::db::Database;
use void_core::models::*;

use super::extract::{extract_media_metadata, extract_media_type, extract_quoted_id, extract_text};

/// Returns true for system/protocol messages that have no user-visible content.
pub(super) fn is_system_message(msg: &WaMessage) -> bool {
    let base = msg.get_base_message();
    base.sender_key_distribution_message.is_some()
        || base.protocol_message.is_some()
        || base.sticker_sync_rmr_message.is_some()
        || base.keep_in_chat_message.is_some()
        || base.pin_in_chat_message.is_some()
        || base
            .fast_ratchet_key_sender_key_distribution_message
            .is_some()
}

pub(super) fn handle_history_sync(
    db: &Database,
    account_id: &str,
    history: &HistorySync,
) -> anyhow::Result<()> {
    let mut total_stored = 0u64;

    for conv in &history.conversations {
        let chat_jid = &conv.id;
        if chat_jid.is_empty() {
            continue;
        }
        let is_group = chat_jid.ends_with("@g.us");
        let conv_id = format!("wa_{account_id}_{chat_jid}");

        let last_ts = conv
            .messages
            .iter()
            .filter_map(|m| m.message.as_ref()?.message_timestamp)
            .max()
            .map(|t| t as i64);

        let conv_name = conv.name.clone().unwrap_or_else(|| chat_jid.clone());
        let conversation = Conversation {
            id: conv_id.clone(),
            account_id: account_id.to_string(),
            connector: "whatsapp".into(),
            external_id: chat_jid.clone(),
            name: Some(conv_name),
            kind: if is_group {
                ConversationKind::Group
            } else {
                ConversationKind::Dm
            },
            last_message_at: last_ts,
            unread_count: conv.unread_count.unwrap_or(0) as i64,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&conversation)?;

        let mut sorted_msgs: Vec<_> = conv
            .messages
            .iter()
            .filter_map(|m| {
                let wmi = m.message.as_ref()?;
                let wa_msg = wmi.message.as_ref()?;
                let ts = wmi.message_timestamp? as i64;
                let key = &wmi.key;
                let msg_id = key.id.as_deref().unwrap_or_default();
                if msg_id.is_empty() {
                    return None;
                }
                Some((wmi, wa_msg, ts, msg_id))
            })
            .collect();
        sorted_msgs.sort_by_key(|&(_, _, ts, _)| ts);

        let mut prev_context_id: Option<String> = None;
        let mut prev_ts: Option<i64> = None;

        for (wmi, wa_msg, msg_ts, msg_id) in &sorted_msgs {
            if is_system_message(wa_msg) {
                continue;
            }

            let body = extract_text(wa_msg);
            let media_type = extract_media_type(wa_msg);
            let media_metadata = extract_media_metadata(wa_msg);

            if body.is_none() && media_type.is_none() {
                continue;
            }

            let from_me = wmi.key.from_me.unwrap_or(false);
            let sender_jid = if from_me {
                account_id.to_string()
            } else if is_group {
                wmi.key
                    .participant
                    .clone()
                    .or_else(|| wmi.participant.clone())
                    .unwrap_or_else(|| chat_jid.clone())
            } else {
                chat_jid.clone()
            };

            let sender_name = wmi.push_name.clone();

            let context_id = if let (Some(prev_cid), Some(pt)) = (&prev_context_id, prev_ts) {
                if (*msg_ts - pt).abs() <= 3600 {
                    prev_cid.clone()
                } else {
                    format!("wa_{account_id}-group-{chat_jid}-{msg_ts}")
                }
            } else {
                format!("wa_{account_id}-group-{chat_jid}-{msg_ts}")
            };

            prev_context_id = Some(context_id.clone());
            prev_ts = Some(*msg_ts);

            let reply_to_id = extract_quoted_id(wa_msg);

            let message = void_core::models::Message {
                id: format!("wa_{account_id}_{msg_id}"),
                conversation_id: conv_id.clone(),
                account_id: account_id.to_string(),
                connector: "whatsapp".into(),
                external_id: msg_id.to_string(),
                sender: sender_jid,
                sender_name,
                body,
                timestamp: *msg_ts,
                synced_at: None,
                is_archived: false,
                reply_to_id,
                media_type,
                metadata: media_metadata,
                context_id: Some(context_id),
                context: None,
            };
            db.upsert_message(&message)?;
            total_stored += 1;
        }
    }

    info!(
        account_id = %account_id,
        sync_type = history.sync_type,
        stored = total_stored,
        "history sync processed"
    );
    Ok(())
}

pub(super) fn handle_message(
    db: &Database,
    account_id: &str,
    msg: &WaMessage,
    info: &MessageInfo,
) -> anyhow::Result<()> {
    if is_system_message(msg) {
        debug!(msg_id = %info.id, "skipping system message");
        return Ok(());
    }

    let base = msg.get_base_message();

    if let Some(ref reaction) = base.reaction_message {
        return handle_reaction(db, account_id, reaction, info);
    }

    let chat_jid = info.source.chat.to_string();
    let sender_jid = info.source.sender.to_string();
    let is_group = info.source.is_group;

    let conv_id = format!("wa_{account_id}_{chat_jid}");
    let conversation = Conversation {
        id: conv_id.clone(),
        account_id: account_id.to_string(),
        connector: "whatsapp".into(),
        external_id: chat_jid.clone(),
        name: if is_group {
            Some(chat_jid.clone())
        } else {
            Some(if info.push_name.is_empty() {
                sender_jid.clone()
            } else {
                info.push_name.clone()
            })
        },
        kind: if is_group {
            ConversationKind::Group
        } else {
            ConversationKind::Dm
        },
        last_message_at: Some(info.timestamp.timestamp()),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conversation)?;

    let body = extract_text(msg);
    let media_type = extract_media_type(msg);
    let media_metadata = extract_media_metadata(msg);

    if body.is_none() && media_type.is_none() {
        debug!(msg_id = %info.id, "skipping message with no extractable content");
        return Ok(());
    }

    let msg_ts = info.timestamp.timestamp();
    let context_id = {
        let last = db.last_message_in_conversation(&conv_id).ok().flatten();
        if let Some(prev) = last {
            if (msg_ts - prev.timestamp).abs() <= 3600 {
                prev.context_id.unwrap_or_else(|| {
                    format!("wa_{account_id}-group-{chat_jid}-{}", prev.timestamp)
                })
            } else {
                format!("wa_{account_id}-group-{chat_jid}-{msg_ts}")
            }
        } else {
            format!("wa_{account_id}-group-{chat_jid}-{msg_ts}")
        }
    };

    let message = void_core::models::Message {
        id: format!("wa_{account_id}_{}", info.id),
        conversation_id: conv_id,
        account_id: account_id.to_string(),
        connector: "whatsapp".into(),
        external_id: info.id.clone(),
        sender: sender_jid,
        sender_name: if info.push_name.is_empty() {
            None
        } else {
            Some(info.push_name.clone())
        },
        body,
        timestamp: msg_ts,
        synced_at: None,
        is_archived: false,
        reply_to_id: extract_quoted_id(msg),
        media_type,
        metadata: media_metadata,
        context_id: Some(context_id),
        context: None,
    };
    db.upsert_message(&message)?;

    debug!(msg_id = %info.id, chat = %chat_jid, "stored WA message");
    Ok(())
}

fn handle_reaction(
    db: &Database,
    account_id: &str,
    reaction: &wa_rs_proto::whatsapp::message::ReactionMessage,
    info: &MessageInfo,
) -> anyhow::Result<()> {
    let target_id = reaction
        .key
        .as_ref()
        .and_then(|k| k.id.as_ref())
        .ok_or_else(|| anyhow::anyhow!("reaction has no target message key"))?;

    let emoji = reaction.text.as_deref().unwrap_or("");
    let sender = info.source.sender.to_string();
    let sender_name = if info.push_name.is_empty() {
        sender.clone()
    } else {
        info.push_name.clone()
    };

    let Some(original) = db.find_message_by_external_id(account_id, target_id)? else {
        debug!(target_id, "reaction target message not found, skipping");
        return Ok(());
    };

    let mut meta = original
        .metadata
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));

    // Ensure metadata is an object; if not (e.g. corrupted data), use empty object
    if !meta.is_object() {
        meta = serde_json::json!({});
    }
    let obj = meta
        .as_object_mut()
        .expect("metadata is object after check");
    let reactions_value = obj
        .entry("reactions")
        .or_insert_with(|| serde_json::json!([]));
    if !reactions_value.is_array() {
        *reactions_value = serde_json::json!([]);
    }
    let reactions = reactions_value
        .as_array_mut()
        .expect("reactions is array after check");

    // Remove any existing reaction from the same sender
    reactions.retain(|r| r.get("sender").and_then(|s| s.as_str()) != Some(&sender));

    // Empty emoji means reaction removed; non-empty means add/replace
    if !emoji.is_empty() {
        reactions.push(serde_json::json!({
            "emoji": emoji,
            "sender": sender,
            "sender_name": sender_name,
        }));
    }

    db.update_message_metadata(&original.id, &meta)?;
    debug!(
        target_id,
        emoji,
        sender = %sender,
        "updated reaction on message"
    );
    Ok(())
}

pub(super) fn render_qr(code: &str) {
    if let Err(e) = qr2term::print_qr(code) {
        eprintln!("Could not render QR code: {e}");
        eprintln!("Raw pairing code: {code}");
    }
}
