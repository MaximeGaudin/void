//! Building and sending WhatsApp messages; JID and reply ID parsing.

use wa_rs::Jid;
use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

use void_core::models::MessageContent;

/// Build a WaMessage from MessageContent.
pub(crate) fn build_wa_message(
    content: &MessageContent,
    context_info: Option<ContextInfo>,
) -> anyhow::Result<WaMessage> {
    match content {
        MessageContent::Text(text) => {
            if let Some(ctx) = context_info {
                Ok(WaMessage {
                    extended_text_message: Some(Box::new(ExtendedTextMessage {
                        text: Some(text.clone()),
                        context_info: Some(Box::new(ctx)),
                        ..Default::default()
                    })),
                    ..Default::default()
                })
            } else {
                Ok(WaMessage {
                    conversation: Some(text.clone()),
                    ..Default::default()
                })
            }
        }
        MessageContent::File { .. } => Err(anyhow::Error::msg(
            "Use upload_and_build_media_message for file content",
        )),
    }
}

/// Parse a JID string. Bare phone numbers get `@s.whatsapp.net` appended.
pub fn parse_jid(input: &str) -> anyhow::Result<Jid> {
    if input.contains('@') {
        let (user, server) = input
            .split_once('@')
            .ok_or_else(|| anyhow::Error::msg(format!("invalid JID: {input}")))?;
        Ok(Jid::new(user, server))
    } else {
        Ok(Jid::new(input, "s.whatsapp.net"))
    }
}

/// Parse reply message_id format: `chat_jid:wa_msg_id`
pub(crate) fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (chat_jid, msg_id) = message_id
        .split_once(':')
        .ok_or_else(|| anyhow::Error::msg("invalid reply id format, expected 'chat_jid:msg_id'"))?;
    Ok((chat_jid.to_string(), msg_id.to_string()))
}

/// Normalize a phone number for WhatsApp JID: strip `+` and spaces.
pub fn normalize_phone(phone: &str) -> String {
    phone.chars().filter(|c| c.is_ascii_digit()).collect()
}
