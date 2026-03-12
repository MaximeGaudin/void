use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wa_rs::bot::Bot;
use wa_rs::client::Client;
use wa_rs::proto_helpers::MessageExt;
use wa_rs::send::SendOptions;
use wa_rs::types::events::Event;
use wa_rs::types::message::MessageInfo;
use wa_rs::Jid;
use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
use wa_rs_proto::whatsapp::{ContextInfo, HistorySync, Message as WaMessage};
use wa_rs_sqlite_storage::SqliteStore;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

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

    pub async fn download_media(
        &self,
        direct_path: &str,
        media_key_b64: &str,
        file_sha256_b64: &str,
        file_enc_sha256_b64: &str,
        file_length: u64,
        media_type_str: &str,
    ) -> anyhow::Result<Vec<u8>> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        use wa_rs::download::MediaType as WaMediaType;

        self.ensure_connected().await?;
        let guard = self.client.lock().await;
        let client = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WhatsApp not connected"))?;

        let media_key = STANDARD.decode(media_key_b64)?;
        let file_sha256 = STANDARD.decode(file_sha256_b64)?;
        let file_enc_sha256 = STANDARD.decode(file_enc_sha256_b64)?;

        let media_type = match media_type_str {
            "image" => WaMediaType::Image,
            "video" => WaMediaType::Video,
            "audio" => WaMediaType::Audio,
            "document" => WaMediaType::Document,
            "sticker" => WaMediaType::Sticker,
            other => anyhow::bail!("unsupported media type: {other}"),
        };

        let data = client
            .download_from_params(
                direct_path,
                &media_key,
                &file_sha256,
                &file_enc_sha256,
                file_length,
                media_type,
            )
            .await
            .map_err(|e| anyhow::anyhow!("download failed: {e}"))?;

        Ok(data)
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
                                Ok(()) => {
                                    let sender = if info.source.is_from_me {
                                        "me".to_string()
                                    } else {
                                        info.push_name.clone()
                                    };
                                    let preview = extract_text(&msg).unwrap_or_default();
                                    let preview: String = preview.chars().take(80).collect();
                                    if !preview.is_empty() {
                                        eprintln!(
                                            "[whatsapp:{}] new: {} — {}",
                                            account_id, sender, preview
                                        );
                                    }
                                }
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
        let msg = build_wa_message(&content, None)?;
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
        debug!(account_id = %self.config_id, message_id = %msg_id, "WhatsApp reply sent");
        Ok(msg_id)
    }
}

/// Returns true for system/protocol messages that have no user-visible content.
fn is_system_message(msg: &WaMessage) -> bool {
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

fn handle_history_sync(
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

fn handle_message(
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

fn render_qr(code: &str) {
    if let Err(e) = qr2term::print_qr(code) {
        eprintln!("Could not render QR code: {e}");
        eprintln!("Raw pairing code: {code}");
    }
}

fn extract_text(msg: &WaMessage) -> Option<String> {
    let base = msg.get_base_message();

    if let Some(text) = base.text_content() {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    if let Some(caption) = base.get_caption() {
        if !caption.is_empty() {
            return Some(caption.to_string());
        }
    }

    if let Some(ref loc) = base.location_message {
        return Some(
            loc.name
                .clone()
                .unwrap_or_else(|| "📍 Location".to_string()),
        );
    }
    if let Some(ref contact) = base.contact_message {
        return Some(format!(
            "👤 {}",
            contact.display_name.as_deref().unwrap_or("Contact")
        ));
    }
    if let Some(ref contacts) = base.contacts_array_message {
        let count = contacts.contacts.len();
        return Some(format!("👥 {count} contacts"));
    }
    if base.sticker_message.is_some() || base.lottie_sticker_message.is_some() {
        return Some("🖼️ Sticker".to_string());
    }
    if base.audio_message.is_some() {
        return Some("🎵 Audio".to_string());
    }
    if base.ptv_message.is_some() {
        return Some("🎥 Video note".to_string());
    }

    if let Some(ref poll) = base.poll_creation_message {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v2 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v3 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v5 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if base.poll_update_message.is_some() {
        return Some("📊 Poll vote".to_string());
    }

    if let Some(ref invite) = base.group_invite_message {
        return Some(format!(
            "👥 Group invite: {}",
            invite.group_name.as_deref().unwrap_or("group")
        ));
    }

    if let Some(ref live_loc) = base.live_location_message {
        return Some(
            live_loc
                .caption
                .clone()
                .unwrap_or_else(|| "📍 Live location".to_string()),
        );
    }

    if base.call.is_some() || base.bcall_message.is_some() {
        return Some("📞 Call".to_string());
    }
    if base.call_log_messsage.is_some() {
        return Some("📞 Call".to_string());
    }

    if let Some(ref event) = base.event_message {
        return Some(format!("📅 {}", event.name.as_deref().unwrap_or("Event")));
    }

    if base.image_message.is_some() {
        return Some("📷 Image".to_string());
    }
    if base.video_message.is_some() {
        return Some("🎬 Video".to_string());
    }
    if let Some(ref doc) = base.document_message {
        let name = doc
            .file_name
            .as_deref()
            .or(doc.title.as_deref())
            .unwrap_or("Document");
        return Some(format!("📄 {name}"));
    }

    None
}

fn extract_media_type(msg: &WaMessage) -> Option<String> {
    let base = msg.get_base_message();
    if base.image_message.is_some() {
        return Some("image".into());
    }
    if base.video_message.is_some() || base.ptv_message.is_some() {
        return Some("video".into());
    }
    if base.audio_message.is_some() {
        return Some("audio".into());
    }
    if base.document_message.is_some() {
        return Some("document".into());
    }
    if base.sticker_message.is_some() || base.lottie_sticker_message.is_some() {
        return Some("sticker".into());
    }
    if base.location_message.is_some() || base.live_location_message.is_some() {
        return Some("location".into());
    }
    if base.contact_message.is_some() || base.contacts_array_message.is_some() {
        return Some("contact".into());
    }
    if base.poll_creation_message.is_some()
        || base.poll_creation_message_v2.is_some()
        || base.poll_creation_message_v3.is_some()
        || base.poll_creation_message_v5.is_some()
        || base.poll_update_message.is_some()
    {
        return Some("poll".into());
    }
    if base.group_invite_message.is_some() {
        return Some("invite".into());
    }
    if base.call.is_some() || base.call_log_messsage.is_some() || base.bcall_message.is_some() {
        return Some("call".into());
    }
    if base.event_message.is_some() {
        return Some("event".into());
    }
    None
}

fn insert_download_fields(
    meta: &mut serde_json::Map<String, serde_json::Value>,
    media_key: &Option<Vec<u8>>,
    file_sha256: &Option<Vec<u8>>,
    file_enc_sha256: &Option<Vec<u8>>,
) {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    if let Some(ref key) = media_key {
        meta.insert("media_key".into(), serde_json::json!(STANDARD.encode(key)));
    }
    if let Some(ref sha) = file_sha256 {
        meta.insert(
            "file_sha256".into(),
            serde_json::json!(STANDARD.encode(sha)),
        );
    }
    if let Some(ref sha) = file_enc_sha256 {
        meta.insert(
            "file_enc_sha256".into(),
            serde_json::json!(STANDARD.encode(sha)),
        );
    }
}

fn extract_media_metadata(msg: &WaMessage) -> Option<serde_json::Value> {
    let base = msg.get_base_message();

    if let Some(ref doc) = base.document_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref name) = doc.file_name {
            meta.insert("file_name".into(), serde_json::json!(name));
        }
        if let Some(ref mime) = doc.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = doc.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(ref title) = doc.title {
            meta.insert("title".into(), serde_json::json!(title));
        }
        if let Some(pages) = doc.page_count {
            meta.insert("page_count".into(), serde_json::json!(pages));
        }
        if let Some(ref path) = doc.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("document"));
        insert_download_fields(
            &mut meta,
            &doc.media_key,
            &doc.file_sha256,
            &doc.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref img) = base.image_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = img.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = img.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(w) = img.width {
            meta.insert("width".into(), serde_json::json!(w));
        }
        if let Some(h) = img.height {
            meta.insert("height".into(), serde_json::json!(h));
        }
        if let Some(ref path) = img.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("image"));
        insert_download_fields(
            &mut meta,
            &img.media_key,
            &img.file_sha256,
            &img.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref vid) = base.video_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = vid.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = vid.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(secs) = vid.seconds {
            meta.insert("duration_secs".into(), serde_json::json!(secs));
        }
        if let Some(w) = vid.width {
            meta.insert("width".into(), serde_json::json!(w));
        }
        if let Some(h) = vid.height {
            meta.insert("height".into(), serde_json::json!(h));
        }
        if let Some(ref path) = vid.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("video"));
        insert_download_fields(
            &mut meta,
            &vid.media_key,
            &vid.file_sha256,
            &vid.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref aud) = base.audio_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = aud.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = aud.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(secs) = aud.seconds {
            meta.insert("duration_secs".into(), serde_json::json!(secs));
        }
        if let Some(ptt) = aud.ptt {
            meta.insert("voice_note".into(), serde_json::json!(ptt));
        }
        if let Some(ref path) = aud.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("audio"));
        insert_download_fields(
            &mut meta,
            &aud.media_key,
            &aud.file_sha256,
            &aud.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
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
        assert_eq!(extract_text(&msg), Some("ephemeral text".into()));
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
        assert_eq!(extract_text(&msg), Some("from other device".into()));
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
        assert_eq!(extract_text(&msg), Some("view once caption".into()));
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
        assert_eq!(extract_text(&msg), Some("edited text".into()));
    }

    #[test]
    fn extract_text_protocol_message_returns_none() {
        use wa_rs_proto::whatsapp::message::ProtocolMessage;
        let msg = WaMessage {
            protocol_message: Some(Box::new(ProtocolMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), None);
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
        assert_eq!(extract_text(&msg), Some("photo caption".into()));
    }

    #[test]
    fn extract_text_sticker_fallback() {
        use wa_rs_proto::whatsapp::message::StickerMessage;
        let msg = WaMessage {
            sticker_message: Some(Box::new(StickerMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("\u{1f5bc}\u{fe0f} Sticker".into()));
    }

    #[test]
    fn extract_text_audio_fallback() {
        use wa_rs_proto::whatsapp::message::AudioMessage;
        let msg = WaMessage {
            audio_message: Some(Box::new(AudioMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("\u{1f3b5} Audio".into()));
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
        assert_eq!(extract_media_type(&msg), Some("video".into()));
    }

    #[test]
    fn extract_media_type_none_for_text() {
        let msg = WaMessage {
            conversation: Some("just text".into()),
            ..Default::default()
        };
        assert_eq!(extract_media_type(&msg), None);
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
        assert_eq!(extract_text(&msg), None);
        assert_eq!(extract_media_type(&msg), None);
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
        assert_eq!(extract_text(&msg), Some("📊 Favorite color?".into()));
        assert_eq!(extract_media_type(&msg), Some("poll".into()));
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
        assert_eq!(extract_text(&msg), Some("👥 Group invite: My Group".into()));
        assert_eq!(extract_media_type(&msg), Some("invite".into()));
    }

    #[test]
    fn extract_text_call() {
        use wa_rs_proto::whatsapp::message::Call;
        let msg = WaMessage {
            call: Some(Box::new(Call::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("📞 Call".into()));
        assert_eq!(extract_media_type(&msg), Some("call".into()));
    }

    #[test]
    fn extract_text_video_note() {
        use wa_rs_proto::whatsapp::message::VideoMessage;
        let msg = WaMessage {
            ptv_message: Some(Box::new(VideoMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("🎥 Video note".into()));
        assert_eq!(extract_media_type(&msg), Some("video".into()));
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
        assert_eq!(extract_text(&msg), Some("📅 Team meeting".into()));
        assert_eq!(extract_media_type(&msg), Some("event".into()));
    }

    #[test]
    fn is_system_message_sender_key_distribution() {
        use wa_rs_proto::whatsapp::message::SenderKeyDistributionMessage;
        let msg = WaMessage {
            sender_key_distribution_message: Some(SenderKeyDistributionMessage::default()),
            ..Default::default()
        };
        assert!(is_system_message(&msg));
    }

    #[test]
    fn is_system_message_protocol() {
        use wa_rs_proto::whatsapp::message::ProtocolMessage;
        let msg = WaMessage {
            protocol_message: Some(Box::new(ProtocolMessage::default())),
            ..Default::default()
        };
        assert!(is_system_message(&msg));
    }

    #[test]
    fn is_system_message_false_for_text() {
        let msg = WaMessage {
            conversation: Some("hello".into()),
            ..Default::default()
        };
        assert!(!is_system_message(&msg));
    }

    #[test]
    fn extract_text_image_no_caption_fallback() {
        use wa_rs_proto::whatsapp::message::ImageMessage;
        let msg = WaMessage {
            image_message: Some(Box::new(ImageMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("📷 Image".into()));
    }

    #[test]
    fn extract_text_document_fallback() {
        use wa_rs_proto::whatsapp::message::DocumentMessage;
        let msg = WaMessage {
            document_message: Some(Box::new(DocumentMessage::default())),
            ..Default::default()
        };
        assert_eq!(extract_text(&msg), Some("📄 Document".into()));
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
        assert_eq!(extract_text(&msg), Some("📄 report.pdf".into()));
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
        let meta = extract_media_metadata(&msg).unwrap();
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
        let meta = extract_media_metadata(&msg).unwrap();
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
        assert!(extract_media_metadata(&msg).is_none());
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
