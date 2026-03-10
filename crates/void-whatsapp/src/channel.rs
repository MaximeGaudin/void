use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use wa_rs::bot::Bot;
use wa_rs::client::Client;
use wa_rs::send::SendOptions;
use wa_rs::types::events::Event;
use wa_rs::types::message::MessageInfo;
use wa_rs::Jid;
use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};
use wa_rs_sqlite_storage::SqliteStore;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;

use void_core::channel::Channel;
use void_core::db::Database;
use void_core::models::*;

pub struct WhatsAppChannel {
    account_id: String,
    session_db_path: String,
    client: Arc<Mutex<Option<Arc<Client>>>>,
}

impl WhatsAppChannel {
    pub fn new(account_id: &str, session_db_path: &str) -> Self {
        Self {
            account_id: account_id.to_string(),
            session_db_path: session_db_path.to_string(),
            client: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn channel_type(&self) -> ChannelType {
        ChannelType::WhatsApp
    }

    fn account_id(&self) -> &str {
        &self.account_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let backend = Arc::new(SqliteStore::new(&self.session_db_path).await?);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, _client| {
                let tx = tx.clone();
                async move {
                    let _ = tx.send(event);
                }
            })
            .build()
            .await?;

        let bot_future = bot.run().await?;

        tokio::select! {
            _ = bot_future => {
                anyhow::bail!("WhatsApp disconnected before authentication completed");
            }
            result = async {
                loop {
                    match rx.recv().await {
                        Some(Event::PairingQrCode { code, .. }) => {
                            eprintln!("Scan this QR code with WhatsApp > Linked Devices > Link a Device:\n{code}");
                        }
                        Some(Event::PairSuccess(_)) => {
                            info!(account_id = %self.account_id, "WhatsApp paired successfully");
                            return Ok::<(), anyhow::Error>(());
                        }
                        Some(Event::Connected(_)) => {
                            info!(account_id = %self.account_id, "WhatsApp connected (session exists)");
                            return Ok(());
                        }
                        Some(Event::PairError(e)) => {
                            return Err(anyhow::anyhow!("Pairing error: {:?}", e));
                        }
                        None => {
                            return Err(anyhow::anyhow!("Event channel closed"));
                        }
                        _ => {}
                    }
                }
            } => {
                result?;
            }
        }
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        info!(account_id = %self.account_id, "starting WhatsApp sync");

        let backend = Arc::new(SqliteStore::new(&self.session_db_path).await?);
        let db_clone = Arc::clone(&db);
        let account_id = self.account_id.clone();
        let client_holder = Arc::clone(&self.client);

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, client| {
                let db = Arc::clone(&db_clone);
                let account_id = account_id.clone();
                let client_holder = Arc::clone(&client_holder);
                async move {
                    {
                        let mut holder = client_holder.lock().await;
                        if holder.is_none() {
                            *holder = Some(client);
                        }
                    }

                    match event {
                        Event::PairingQrCode { code, .. } => {
                            eprintln!("Scan this QR code with WhatsApp:\n{code}");
                        }
                        Event::Connected(_) => {
                            info!("WhatsApp connected");
                        }
                        Event::Message(msg, info) => {
                            if let Err(e) = handle_message(&db, &account_id, &msg, &info) {
                                warn!("Failed to store WA message: {e}");
                            }
                        }
                        Event::Disconnected(_) => {
                            warn!("WhatsApp disconnected, waiting for reconnect");
                        }
                        _ => {
                            debug!(event = ?std::mem::discriminant(&event), "WhatsApp event");
                        }
                    }
                }
            })
            .build()
            .await?;

        let bot_future = bot.run().await?;

        tokio::select! {
            result = bot_future => {
                result.map_err(|e| anyhow::anyhow!("WhatsApp bot error: {e}"))?;
            }
            _ = cancel.cancelled() => {
                info!("WhatsApp sync cancelled");
            }
        }
        Ok(())
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let connected = self.client.lock().await.is_some();
        Ok(HealthStatus {
            account_id: self.account_id.clone(),
            channel_type: ChannelType::WhatsApp,
            ok: connected,
            message: if connected {
                "connected".into()
            } else {
                "not connected (run void sync first)".into()
            },
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp not connected"))?;
        let jid = parse_jid(to)?;
        let msg = build_wa_message(&content, None)?;
        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        Ok(msg_id)
    }

    /// Reply to a WhatsApp message. `message_id` format: `chat_jid:wa_msg_id`.
    /// When `in_thread` is true, the reply quotes the original message.
    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        let (chat_jid_str, quoted_msg_id) = parse_reply_id(message_id)?;

        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp not connected"))?;

        let jid = parse_jid(&chat_jid_str)?;

        let context = if in_thread {
            Some(ContextInfo {
                stanza_id: Some(quoted_msg_id),
                ..Default::default()
            })
        } else {
            None
        };

        let msg = build_wa_message(&content, context)?;
        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        Ok(msg_id)
    }
}

fn handle_message(
    db: &Database,
    account_id: &str,
    msg: &WaMessage,
    info: &MessageInfo,
) -> anyhow::Result<()> {
    let chat_jid = info.source.chat.to_string();
    let sender_jid = info.source.sender.to_string();
    let is_group = info.source.is_group;

    let conv_id = format!("wa_{account_id}_{chat_jid}");
    let conversation = Conversation {
        id: conv_id.clone(),
        account_id: account_id.to_string(),
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
        metadata: None,
    };
    db.upsert_conversation(&conversation)?;

    let body = extract_text(msg);
    let media_type = extract_media_type(msg);

    let message = void_core::models::Message {
        id: format!("wa_{account_id}_{}", info.id),
        conversation_id: conv_id,
        account_id: account_id.to_string(),
        external_id: info.id.clone(),
        sender: sender_jid,
        sender_name: if info.push_name.is_empty() {
            None
        } else {
            Some(info.push_name.clone())
        },
        body,
        timestamp: info.timestamp.timestamp(),
        is_from_me: info.source.is_from_me,
        reply_to_id: extract_quoted_id(msg),
        media_type,
        metadata: None,
    };
    db.upsert_message(&message)?;

    debug!(msg_id = %info.id, chat = %chat_jid, "stored WA message");
    Ok(())
}

fn extract_text(msg: &WaMessage) -> Option<String> {
    if let Some(ref text) = msg.conversation {
        return Some(text.clone());
    }
    if let Some(ref ext) = msg.extended_text_message {
        return ext.text.clone();
    }
    if let Some(ref img) = msg.image_message {
        return img.caption.clone();
    }
    if let Some(ref vid) = msg.video_message {
        return vid.caption.clone();
    }
    if let Some(ref doc) = msg.document_message {
        return doc.caption.clone();
    }
    None
}

fn extract_media_type(msg: &WaMessage) -> Option<String> {
    if msg.image_message.is_some() {
        return Some("image".into());
    }
    if msg.video_message.is_some() {
        return Some("video".into());
    }
    if msg.audio_message.is_some() {
        return Some("audio".into());
    }
    if msg.document_message.is_some() {
        return Some("document".into());
    }
    if msg.sticker_message.is_some() {
        return Some("sticker".into());
    }
    if msg.location_message.is_some() {
        return Some("location".into());
    }
    if msg.contact_message.is_some() {
        return Some("contact".into());
    }
    None
}

fn extract_quoted_id(msg: &WaMessage) -> Option<String> {
    if let Some(ref ext) = msg.extended_text_message {
        if let Some(ref ctx) = ext.context_info {
            return ctx.stanza_id.clone();
        }
    }
    None
}

fn build_wa_message(
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
        MessageContent::File { .. } => {
            anyhow::bail!("File sending not yet supported for WhatsApp");
        }
    }
}

/// Parse a JID string. Bare phone numbers get `@s.whatsapp.net` appended.
pub fn parse_jid(input: &str) -> anyhow::Result<Jid> {
    if input.contains('@') {
        let (user, server) = input
            .split_once('@')
            .ok_or_else(|| anyhow::anyhow!("invalid JID: {input}"))?;
        Ok(Jid::new(user, server))
    } else {
        Ok(Jid::new(input, "s.whatsapp.net"))
    }
}

/// Parse reply message_id format: `chat_jid:wa_msg_id`
fn parse_reply_id(message_id: &str) -> anyhow::Result<(String, String)> {
    let (chat_jid, msg_id) = message_id
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("invalid reply id format, expected 'chat_jid:msg_id'"))?;
    Ok((chat_jid.to_string(), msg_id.to_string()))
}

/// Normalize a phone number for WhatsApp JID: strip `+` and spaces.
pub fn normalize_phone(phone: &str) -> String {
    phone.chars().filter(|c| c.is_ascii_digit()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_jid_phone_number() {
        let jid = parse_jid("33612345678").unwrap();
        assert_eq!(jid.to_string(), "33612345678@s.whatsapp.net");
    }

    #[test]
    fn parse_jid_full_dm() {
        let jid = parse_jid("33612345678@s.whatsapp.net").unwrap();
        assert_eq!(jid.to_string(), "33612345678@s.whatsapp.net");
    }

    #[test]
    fn parse_jid_group() {
        let jid = parse_jid("120363123456789@g.us").unwrap();
        assert_eq!(jid.to_string(), "120363123456789@g.us");
    }

    #[test]
    fn normalize_phone_strips_prefix() {
        assert_eq!(normalize_phone("+33 6 12 34 56 78"), "33612345678");
    }

    #[test]
    fn extract_text_conversation() {
        let msg = WaMessage {
            conversation: Some("hello".into()),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("hello".into()));
    }

    #[test]
    fn extract_text_extended() {
        let msg = WaMessage {
            extended_text_message: Some(Box::new(ExtendedTextMessage {
                text: Some("extended hello".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("extended hello".into()));
    }

    #[test]
    fn extract_media_type_image() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_media_type(&msg), Some("image".into()));
    }

    #[test]
    fn build_text_message_simple() {
        let content = MessageContent::Text("test".into());
        let msg = build_wa_message(&content, None).unwrap();
        assert_eq!(msg.conversation, Some("test".into()));
        assert!(msg.extended_text_message.is_none());
    }

    #[test]
    fn build_quoted_message() {
        let content = MessageContent::Text("reply text".into());
        let ctx = ContextInfo {
            stanza_id: Some("orig_msg_123".into()),
            ..Default::default()
        };
        let msg = build_wa_message(&content, Some(ctx)).unwrap();
        assert!(msg.conversation.is_none());
        let ext = msg.extended_text_message.as_ref().unwrap();
        assert_eq!(ext.text, Some("reply text".into()));
        assert_eq!(
            ext.context_info.as_ref().unwrap().stanza_id,
            Some("orig_msg_123".into())
        );
    }

    #[test]
    fn parse_reply_id_valid() {
        let (chat, msg) = parse_reply_id("33612345678@s.whatsapp.net:ABC123DEF").unwrap();
        assert_eq!(chat, "33612345678@s.whatsapp.net");
        assert_eq!(msg, "ABC123DEF");
    }

    #[test]
    fn parse_reply_id_invalid() {
        assert!(parse_reply_id("no_colon_here").is_err());
    }
}
