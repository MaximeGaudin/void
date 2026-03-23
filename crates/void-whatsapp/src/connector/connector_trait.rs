use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use wa_rs::bot::Bot;
use wa_rs::send::SendOptions;
use wa_rs::types::events::Event;
use wa_rs_sqlite_storage::SqliteStore;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::*;

use super::media::upload_and_build_media_message;
use super::send::{build_wa_message, parse_jid, parse_reply_id};
use super::sync::{handle_history_sync, handle_message, render_qr};
use super::WhatsAppConnector;

#[async_trait]
impl Connector for WhatsAppConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::WhatsApp
    }

    fn connection_id(&self) -> &str {
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
                error!(connection_id = %self.config_id, "WhatsApp disconnected before authentication completed");
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
                            info!(connection_id = %self.config_id, "WhatsApp paired successfully");
                            return Ok::<(), anyhow::Error>(());
                        }
                        Some(Event::Connected(_)) => {
                            info!(connection_id = %self.config_id, "WhatsApp connected (session exists)");
                            return Ok(());
                        }
                        Some(Event::PairError(e)) => {
                            warn!(connection_id = %self.config_id, error = ?e, "WhatsApp authenticate PairError");
                            return Err(anyhow::anyhow!("Pairing error: {:?}", e));
                        }
                        None => {
                            error!(connection_id = %self.config_id, "WhatsApp authenticate event channel closed");
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
                            let connection_id = own_jid_holder
                                .lock()
                                .expect("mutex")
                                .clone()
                                .unwrap_or_else(|| config_id.clone());
                            match handle_message(&db, &connection_id, &msg, &info) {
                                Ok(Some(stored)) => {
                                    let sender = if info.source.is_from_me {
                                        "me".to_string()
                                    } else {
                                        info.push_name.clone()
                                    };
                                    if !stored.body_preview.is_empty() {
                                        let time =
                                            chrono::DateTime::from_timestamp(stored.timestamp, 0)
                                                .map(|utc| utc.with_timezone(&chrono::Local))
                                                .map(|local| {
                                                    local.format("%Y-%m-%d %H:%M:%S %Z").to_string()
                                                })
                                                .unwrap_or_default();
                                        eprintln!(
                                            "[whatsapp:{}] {} {} — {}: {}",
                                            connection_id,
                                            time,
                                            stored.conv_name,
                                            sender,
                                            stored.body_preview
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
                            let connection_id = own_jid_holder
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
                                db.set_mute_by_external_id(&connection_id, &external_id, is_muted)
                            {
                                warn!("Failed to update mute state for {external_id}: {e}");
                            }
                        }
                        Event::HistorySync(history) => {
                            let connection_id = own_jid_holder
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
                                connection_id, sync_type, conv_count, msg_count
                            );
                            if let Err(e) = handle_history_sync(&db, &connection_id, &history) {
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
        debug!(connection_id = %self.config_id, connected, has_session, "WhatsApp health check");
        let message = if connected {
            "connected".into()
        } else if has_session {
            "session found, will connect on sync".into()
        } else {
            "no session found. Run `void setup` to pair.".into()
        };
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
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
        info!(connection_id = %self.config_id, recipient_jid = %jid, "sending WhatsApp message");

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
        debug!(connection_id = %self.config_id, message_id = %msg_id, "WhatsApp message sent");
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
        info!(connection_id = %self.config_id, reply_target = %chat_jid_str, quoted_msg_id = %quoted_msg_id, in_thread, "sending WhatsApp reply");

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
        debug!(connection_id = %self.config_id, message_id = %msg_id, "WhatsApp reply sent");
        Ok(msg_id)
    }
}
