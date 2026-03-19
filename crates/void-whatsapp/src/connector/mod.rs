//! WhatsApp connector: struct, Connector impl, and orchestration.

mod extract;
mod media;
mod send;
mod sync;

// Re-export public API for external crates
pub use send::{normalize_phone, parse_jid};

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wa_rs::bot::Bot;
use wa_rs::client::Client;
use wa_rs::send::SendOptions;
use wa_rs::types::events::Event;
use wa_rs_sqlite_storage::SqliteStore;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use media::upload_and_build_media_message;
use send::{build_wa_message, parse_reply_id};
use sync::{handle_history_sync, handle_message, render_qr};

pub struct WhatsAppConnector {
    config_id: String,
    session_db_path: String,
    client: Arc<Mutex<Option<Arc<Client>>>>,
    own_jid: Arc<std::sync::Mutex<Option<String>>>,
}

impl WhatsAppConnector {
    pub fn new(account_id: &str, session_db_path: &str) -> Self {
        Self {
            config_id: account_id.to_string(),
            session_db_path: session_db_path.to_string(),
            client: Arc::new(Mutex::new(None)),
            own_jid: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Connects to WhatsApp if not already connected using the saved session.
    /// Used by send/reply to establish a temporary connection.
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        {
            let guard = self.client.lock().await;
            if guard.is_some() {
                return Ok(());
            }
        }

        info!(account_id = %self.config_id, "starting WhatsApp connection for send");
        let backend = Arc::new(SqliteStore::new(&self.session_db_path).await?);
        let client_holder = Arc::clone(&self.client);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, client| {
                let client_holder = Arc::clone(&client_holder);
                let tx = tx.clone();
                async move {
                    {
                        let mut holder = client_holder.lock().await;
                        if holder.is_none() {
                            *holder = Some(client);
                        }
                    }
                    let _ = tx.send(event);
                }
            })
            .build()
            .await?;

        let bot_future = bot.run().await?;

        tokio::select! {
            _ = bot_future => {
                anyhow::bail!("WhatsApp disconnected before connecting");
            }
            result = async {
                loop {
                    match rx.recv().await {
                        Some(Event::Connected(_)) => {
                            info!("WhatsApp connected for send");
                            return Ok::<(), anyhow::Error>(());
                        }
                        Some(Event::PairError(e)) => {
                            error!(account_id = %self.config_id, error = ?e, "WhatsApp PairError");
                            return Err(anyhow::anyhow!("Auth error: {:?}. Run `void setup` first.", e));
                        }
                        Some(Event::LoggedOut(_)) => {
                            error!(account_id = %self.config_id, "WhatsApp LoggedOut");
                            return Err(anyhow::anyhow!("Session expired. Run `void setup` to re-authenticate."));
                        }
                        None => {
                            error!(account_id = %self.config_id, "WhatsApp connection closed unexpectedly");
                            return Err(anyhow::anyhow!("Connection closed unexpectedly"));
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
}

#[async_trait]
impl Connector for WhatsAppConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::WhatsApp
    }

    fn account_id(&self) -> &str {
        &self.config_id
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
                error!(account_id = %self.config_id, "WhatsApp disconnected before authentication completed");
                anyhow::bail!("WhatsApp disconnected before authentication completed");
            }
            result = async {
                loop {
                    match rx.recv().await {
                        Some(Event::PairingQrCode { code, .. }) => {
                            eprintln!("Scan this QR code with WhatsApp > Linked Devices > Link a Device:\n");
                            render_qr(&code);
                        }
                        Some(Event::PairSuccess(_)) => {
                            info!(account_id = %self.config_id, "WhatsApp paired successfully");
                            return Ok::<(), anyhow::Error>(());
                        }
                        Some(Event::Connected(_)) => {
                            info!(account_id = %self.config_id, "WhatsApp connected (session exists)");
                            return Ok(());
                        }
                        Some(Event::PairError(e)) => {
                            warn!(account_id = %self.config_id, error = ?e, "WhatsApp authenticate PairError");
                            return Err(anyhow::anyhow!("Pairing error: {:?}", e));
                        }
                        None => {
                            error!(account_id = %self.config_id, "WhatsApp authenticate event channel closed");
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
        info!(config_id = %self.config_id, "starting WhatsApp sync");

        let backend = Arc::new(SqliteStore::new(&self.session_db_path).await?);
        let db_clone = Arc::clone(&db);
        let config_id = self.config_id.clone();
        let client_holder = Arc::clone(&self.client);
        let own_jid_holder = Arc::clone(&self.own_jid);

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, client| {
                let db = Arc::clone(&db_clone);
                let config_id = config_id.clone();
                let client_holder = Arc::clone(&client_holder);
                let own_jid_holder = Arc::clone(&own_jid_holder);
                async move {
                    {
                        let mut holder = client_holder.lock().await;
                        if holder.is_none() {
                            *holder = Some(client);
                        }
                    }

                    match event {
                        Event::PairingQrCode { code, .. } => {
                            eprintln!("Scan this QR code with WhatsApp:\n");
                            render_qr(&code);
                        }
                        Event::Connected(_) => {
                            info!("WhatsApp connected");
                        }
                        Event::Message(msg, info) => {
                            if info.source.is_from_me {
                                let mut jid_lock = own_jid_holder.lock().expect("mutex");
                                if jid_lock.is_none() {
                                    let jid = info.source.sender.to_string();
                                    info!(own_jid = %jid, "discovered own WhatsApp JID");
                                    *jid_lock = Some(jid);
                                }
                            }
                            let account_id = own_jid_holder
                                .lock()
                                .expect("mutex")
                                .clone()
                                .unwrap_or_else(|| config_id.clone());
                            match handle_message(&db, &account_id, &msg, &info) {
                                Ok(Some(stored)) => {
                                    let sender = if info.source.is_from_me {
                                        "me".to_string()
                                    } else {
                                        info.push_name.clone()
                                    };
                                    if !stored.body_preview.is_empty() {
                                        let time = chrono::DateTime::from_timestamp(stored.timestamp, 0)
                                            .map(|utc| utc.with_timezone(&chrono::Local))
                                            .map(|local| local.format("%H:%M").to_string())
                                            .unwrap_or_default();
                                        eprintln!(
                                            "[whatsapp:{}] {} {} — {}: {}",
                                            account_id, time, stored.conv_name, sender, stored.body_preview
                                        );
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    warn!("Failed to store WA message: {e}");
                                }
                            }
                        }
                        Event::MuteUpdate(mute) => {
                            let account_id = own_jid_holder
                                .lock()
                                .expect("mutex")
                                .clone()
                                .unwrap_or_else(|| config_id.clone());
                            let external_id = mute.jid.to_string();
                            let is_muted = mute.action.muted.unwrap_or(false);
                            debug!(
                                jid = %external_id,
                                is_muted,
                                from_full_sync = mute.from_full_sync,
                                "WhatsApp mute update"
                            );
                            if let Err(e) =
                                db.set_mute_by_external_id(&account_id, &external_id, is_muted)
                            {
                                warn!("Failed to update mute state for {external_id}: {e}");
                            }
                        }
                        Event::HistorySync(history) => {
                            let account_id = own_jid_holder
                                .lock()
                                .expect("mutex")
                                .clone()
                                .unwrap_or_else(|| config_id.clone());
                            let sync_type = history.sync_type;
                            let conv_count = history.conversations.len();
                            let msg_count: usize =
                                history.conversations.iter().map(|c| c.messages.len()).sum();
                            eprintln!(
                                "[whatsapp:{}] history sync type={} conversations={} messages={}",
                                account_id, sync_type, conv_count, msg_count
                            );
                            if let Err(e) = handle_history_sync(&db, &account_id, &history) {
                                warn!("Failed to process history sync: {e}");
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
        let has_session = std::path::Path::new(&self.session_db_path).exists();
        let connected = self.client.lock().await.is_some();
        let ok = connected || has_session;
        debug!(account_id = %self.config_id, connected, has_session, "WhatsApp health check");
        let message = if connected {
            "connected".into()
        } else if has_session {
            "session found, will connect on sync".into()
        } else {
            "no session found. Run `void setup` to pair.".into()
        };
        Ok(HealthStatus {
            account_id: self.config_id.clone(),
            connector_type: ConnectorType::WhatsApp,
            ok,
            message,
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        self.ensure_connected().await?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp not connected"))?;
        let jid = parse_jid(to)?;
        info!(account_id = %self.config_id, recipient_jid = %jid, "sending WhatsApp message");

        let msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                upload_and_build_media_message(
                    client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                    None,
                )
                .await?
            }
            _ => build_wa_message(&content, None)?,
        };

        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        debug!(account_id = %self.config_id, message_id = %msg_id, "WhatsApp message sent");
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
        info!(account_id = %self.config_id, reply_target = %chat_jid_str, quoted_msg_id = %quoted_msg_id, in_thread, "sending WhatsApp reply");

        self.ensure_connected().await?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp not connected"))?;

        let jid = parse_jid(&chat_jid_str)?;

        let context_info = if in_thread {
            Some(wa_rs_proto::whatsapp::ContextInfo {
                stanza_id: Some(quoted_msg_id),
                ..Default::default()
            })
        } else {
            None
        };

        let msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                upload_and_build_media_message(
                    client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                    context_info,
                )
                .await?
            }
            _ => build_wa_message(&content, context_info)?,
        };

        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        debug!(account_id = %self.config_id, message_id = %msg_id, "WhatsApp reply sent");
        Ok(msg_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wa_rs::download::MediaType as WaMediaType;
    use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
    use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

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
    fn determine_media_type_from_extension() {
        assert_eq!(
            media::determine_media_type(None, "photo.jpg").0,
            WaMediaType::Image
        );
        assert_eq!(
            media::determine_media_type(None, "clip.mp4").0,
            WaMediaType::Video
        );
        assert_eq!(
            media::determine_media_type(None, "voice.ogg").0,
            WaMediaType::Audio
        );
        assert_eq!(
            media::determine_media_type(None, "doc.pdf").0,
            WaMediaType::Document
        );
        assert_eq!(
            media::determine_media_type(None, "file.unknown").0,
            WaMediaType::Document
        );
    }

    #[test]
    fn extract_text_conversation() {
        let msg = WaMessage {
            conversation: Some("hello".into()),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("hello".into()));
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
        assert_eq!(extract::extract_text(&msg), Some("extended hello".into()));
    }

    #[test]
    fn extract_text_ephemeral_wrapper() {
        use wa_rs_proto::whatsapp::message::FutureProofMessage;
        let msg = WaMessage {
            ephemeral_message: Some(Box::new(FutureProofMessage {
                message: Some(Box::new(WaMessage {
                    conversation: Some("ephemeral text".into()),
                    ..Default::default()
                })),
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("ephemeral text".into()));
    }

    #[test]
    fn extract_text_device_sent_wrapper() {
        use wa_rs_proto::whatsapp::message::DeviceSentMessage;
        let msg = WaMessage {
            device_sent_message: Some(Box::new(DeviceSentMessage {
                message: Some(Box::new(WaMessage {
                    extended_text_message: Some(Box::new(ExtendedTextMessage {
                        text: Some("from other device".into()),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            extract::extract_text(&msg),
            Some("from other device".into())
        );
    }

    #[test]
    fn extract_text_view_once_wrapper() {
        use wa_rs_proto::whatsapp::message::{FutureProofMessage, ImageMessage};
        let msg = WaMessage {
            view_once_message: Some(Box::new(FutureProofMessage {
                message: Some(Box::new(WaMessage {
                    image_message: Some(Box::new(ImageMessage {
                        caption: Some("view once caption".into()),
                        ..Default::default()
                    })),
                    ..Default::default()
                })),
            })),
            ..Default::default()
        };
        assert_eq!(
            extract::extract_text(&msg),
            Some("view once caption".into())
        );
    }

    #[test]
    fn extract_text_edited_message_wrapper() {
        use wa_rs_proto::whatsapp::message::FutureProofMessage;
        let msg = WaMessage {
            edited_message: Some(Box::new(FutureProofMessage {
                message: Some(Box::new(WaMessage {
                    conversation: Some("edited text".into()),
                    ..Default::default()
                })),
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("edited text".into()));
    }

    #[test]
    fn extract_text_protocol_message_returns_none() {
        use wa_rs_proto::whatsapp::message::ProtocolMessage;
        let msg = WaMessage {
            protocol_message: Some(Box::new(ProtocolMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), None);
    }

    #[test]
    fn extract_text_image_caption() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage {
                caption: Some("photo caption".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("photo caption".into()));
    }

    #[test]
    fn extract_text_sticker_fallback() {
        use wa_rs_proto::whatsapp::message::StickerMessage;
        let msg = WaMessage {
            sticker_message: Some(Box::new(StickerMessage::default())),
            ..Default::default()
        };
        assert_eq!(
            extract::extract_text(&msg),
            Some("\u{1f5bc}\u{fe0f} Sticker".into())
        );
    }

    #[test]
    fn extract_text_audio_fallback() {
        use wa_rs_proto::whatsapp::message::AudioMessage;
        let msg = WaMessage {
            audio_message: Some(Box::new(AudioMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("\u{1f3b5} Audio".into()));
    }

    #[test]
    fn extract_media_type_image() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_media_type(&msg), Some("image".into()));
    }

    #[test]
    fn extract_media_type_through_ephemeral() {
        use wa_rs_proto::whatsapp::message::{FutureProofMessage, VideoMessage};
        let msg = WaMessage {
            ephemeral_message: Some(Box::new(FutureProofMessage {
                message: Some(Box::new(WaMessage {
                    video_message: Some(Box::new(VideoMessage::default())),
                    ..Default::default()
                })),
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_media_type(&msg), Some("video".into()));
    }

    #[test]
    fn extract_media_type_none_for_text() {
        let msg = WaMessage {
            conversation: Some("just text".into()),
            ..Default::default()
        };
        assert_eq!(extract::extract_media_type(&msg), None);
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
    fn extract_text_reaction_returns_none() {
        use wa_rs_proto::whatsapp::message::ReactionMessage;
        let msg = WaMessage {
            reaction_message: Some(ReactionMessage {
                text: Some("❤️".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Reactions are handled via metadata on the target message, not as separate messages
        assert_eq!(extract::extract_text(&msg), None);
        assert_eq!(extract::extract_media_type(&msg), None);
    }

    #[test]
    fn extract_text_poll() {
        use wa_rs_proto::whatsapp::message::PollCreationMessage;
        let msg = WaMessage {
            poll_creation_message: Some(Box::new(PollCreationMessage {
                name: Some("Favorite color?".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            extract::extract_text(&msg),
            Some("📊 Favorite color?".into())
        );
        assert_eq!(extract::extract_media_type(&msg), Some("poll".into()));
    }

    #[test]
    fn extract_text_group_invite() {
        use wa_rs_proto::whatsapp::message::GroupInviteMessage;
        let msg = WaMessage {
            group_invite_message: Some(Box::new(GroupInviteMessage {
                group_name: Some("My Group".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(
            extract::extract_text(&msg),
            Some("👥 Group invite: My Group".into())
        );
        assert_eq!(extract::extract_media_type(&msg), Some("invite".into()));
    }

    #[test]
    fn extract_text_call() {
        use wa_rs_proto::whatsapp::message::Call;
        let msg = WaMessage {
            call: Some(Box::new(Call::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("📞 Call".into()));
        assert_eq!(extract::extract_media_type(&msg), Some("call".into()));
    }

    #[test]
    fn extract_text_video_note() {
        use wa_rs_proto::whatsapp::message::VideoMessage;
        let msg = WaMessage {
            ptv_message: Some(Box::new(VideoMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("🎥 Video note".into()));
        assert_eq!(extract::extract_media_type(&msg), Some("video".into()));
    }

    #[test]
    fn extract_text_event() {
        use wa_rs_proto::whatsapp::message::EventMessage;
        let msg = WaMessage {
            event_message: Some(Box::new(EventMessage {
                name: Some("Team meeting".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("📅 Team meeting".into()));
        assert_eq!(extract::extract_media_type(&msg), Some("event".into()));
    }

    #[test]
    fn is_system_message_sender_key_distribution() {
        use wa_rs_proto::whatsapp::message::SenderKeyDistributionMessage;
        let msg = WaMessage {
            sender_key_distribution_message: Some(SenderKeyDistributionMessage::default()),
            ..Default::default()
        };
        assert!(sync::is_system_message(&msg));
    }

    #[test]
    fn is_system_message_protocol() {
        use wa_rs_proto::whatsapp::message::ProtocolMessage;
        let msg = WaMessage {
            protocol_message: Some(Box::new(ProtocolMessage::default())),
            ..Default::default()
        };
        assert!(sync::is_system_message(&msg));
    }

    #[test]
    fn is_system_message_false_for_text() {
        let msg = WaMessage {
            conversation: Some("hello".into()),
            ..Default::default()
        };
        assert!(!sync::is_system_message(&msg));
    }

    #[test]
    fn extract_text_image_no_caption_fallback() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("📷 Image".into()));
    }

    #[test]
    fn extract_text_document_fallback() {
        use wa_rs_proto::whatsapp::message::DocumentMessage;
        let msg = WaMessage {
            document_message: Some(Box::new(DocumentMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("📄 Document".into()));
    }

    #[test]
    fn extract_text_document_with_filename() {
        use wa_rs_proto::whatsapp::message::DocumentMessage;
        let msg = WaMessage {
            document_message: Some(Box::new(DocumentMessage {
                file_name: Some("report.pdf".into()),
                ..Default::default()
            })),
            ..Default::default()
        };
        assert_eq!(extract::extract_text(&msg), Some("📄 report.pdf".into()));
    }

    #[test]
    fn extract_media_metadata_document() {
        use wa_rs_proto::whatsapp::message::DocumentMessage;
        let msg = WaMessage {
            document_message: Some(Box::new(DocumentMessage {
                file_name: Some("report.pdf".into()),
                mimetype: Some("application/pdf".into()),
                file_length: Some(102400),
                page_count: Some(5),
                ..Default::default()
            })),
            ..Default::default()
        };
        let meta = extract::extract_media_metadata(&msg).unwrap();
        assert_eq!(meta["file_name"], "report.pdf");
        assert_eq!(meta["mimetype"], "application/pdf");
        assert_eq!(meta["file_size"], 102400);
        assert_eq!(meta["page_count"], 5);
    }

    #[test]
    fn extract_media_metadata_image() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage {
                mimetype: Some("image/jpeg".into()),
                file_length: Some(50000),
                width: Some(1920),
                height: Some(1080),
                ..Default::default()
            })),
            ..Default::default()
        };
        let meta = extract::extract_media_metadata(&msg).unwrap();
        assert_eq!(meta["mimetype"], "image/jpeg");
        assert_eq!(meta["file_size"], 50000);
        assert_eq!(meta["width"], 1920);
        assert_eq!(meta["height"], 1080);
    }

    #[test]
    fn extract_media_metadata_none_for_text() {
        let msg = WaMessage {
            conversation: Some("just text".into()),
            ..Default::default()
        };
        assert!(extract::extract_media_metadata(&msg).is_none());
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
