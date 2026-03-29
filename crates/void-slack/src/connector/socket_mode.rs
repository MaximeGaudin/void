//! Socket Mode: WebSocket connection, event handling, conversation creation.

use std::collections::HashMap;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use void_core::db::Database;
use void_core::models::Message;

use crate::connector::mapping::{map_conversation, parse_ts};
use crate::connector::SlackConnector;

impl SlackConnector {
    pub(crate) async fn run_socket_mode(
        &self,
        db: &Database,
        cancel: &CancellationToken,
    ) -> anyhow::Result<()> {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let user_cache = self.prefetch_users().await.unwrap_or_default();

        loop {
            if cancel.is_cancelled() {
                info!(connection_id = %self.connection_id, "Slack sync cancelled");
                return Ok(());
            }

            let wss_url = match self.api.connections_open(&self.app_token).await {
                Ok(resp) => resp.url,
                Err(e) => {
                    error!(connection_id = %self.connection_id, error = %e, "failed to open Socket Mode connection");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            info!(connection_id = %self.connection_id, "connecting to Slack Socket Mode");

            let (ws_stream, _) = match tokio_tungstenite::connect_async(&wss_url).await {
                Ok(conn) => conn,
                Err(e) => {
                    error!(connection_id = %self.connection_id, error = %e, "WebSocket connect failed");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
            };

            eprintln!("[slack:{}] Socket Mode connected", self.connection_id);
            let (mut ws_tx, mut ws_rx) = ws_stream.split();

            let disconnect = loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!(connection_id = %self.connection_id, "Slack sync cancelled");
                        return Ok(());
                    }
                    frame = ws_rx.next() => {
                        match frame {
                            Some(Ok(tungstenite::Message::Text(text))) => {
                                let envelope: serde_json::Value = match serde_json::from_str(&text) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        eprintln!("[slack:{}] failed to parse frame: {}", self.connection_id, e);
                                        continue;
                                    }
                                };

                                let msg_type = envelope.get("type").and_then(|v| v.as_str()).unwrap_or("");

                                if let Some(envelope_id) = envelope.get("envelope_id").and_then(|v| v.as_str()) {
                                    let ack = serde_json::json!({"envelope_id": envelope_id});
                                    if let Err(e) = ws_tx.send(tungstenite::Message::Text(ack.to_string().into())).await {
                                        eprintln!("[slack:{}] failed to send ack: {}", self.connection_id, e);
                                    }
                                }

                                match msg_type {
                                    "hello" => {
                                        eprintln!("[slack:{}] Socket Mode handshake OK", self.connection_id);
                                    }
                                    "disconnect" => {
                                        let reason = envelope.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown");
                                        eprintln!("[slack:{}] disconnect requested: {}", self.connection_id, reason);
                                        break true;
                                    }
                                    "events_api" => {
                                        if let Some(payload) = envelope.get("payload") {
                                            self.handle_socket_event(payload, db, &user_cache).await;
                                        }
                                    }
                                    other => {
                                        eprintln!("[slack:{}] unhandled envelope type: {}", self.connection_id, other);
                                    }
                                }
                            }
                            Some(Ok(tungstenite::Message::Ping(_data))) => {
                                let _ = ws_tx.send(tungstenite::Message::Pong(_data)).await;
                            }
                            Some(Ok(tungstenite::Message::Close(reason))) => {
                                eprintln!("[slack:{}] WebSocket closed by server: {:?}", self.connection_id, reason);
                                break true;
                            }
                            Some(Err(e)) => {
                                eprintln!("[slack:{}] WebSocket error: {}", self.connection_id, e);
                                break true;
                            }
                            None => {
                                eprintln!("[slack:{}] WebSocket stream ended", self.connection_id);
                                break true;
                            }
                            _ => {}
                        }
                    }
                }
            };

            if !disconnect || cancel.is_cancelled() {
                return Ok(());
            }

            eprintln!(
                "[slack:{}] reconnecting Socket Mode in 2s...",
                self.connection_id
            );
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    }

    async fn handle_socket_event(
        &self,
        payload: &serde_json::Value,
        db: &Database,
        user_cache: &HashMap<String, String>,
    ) {
        let event = match payload.get("event") {
            Some(e) => e,
            None => {
                eprintln!(
                    "[slack:{}] event payload has no 'event' field",
                    self.connection_id
                );
                return;
            }
        };

        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if event_type != "message" {
            eprintln!(
                "[slack:{}] event type '{}' (not message, skipping)",
                self.connection_id, event_type
            );
            return;
        }

        let subtype = event.get("subtype").and_then(|v| v.as_str());
        match subtype {
            None | Some("file_share") | Some("me_message") | Some("thread_broadcast") => {}
            Some(st) => {
                debug!(subtype = st, "ignoring message subtype");
                return;
            }
        }

        let channel_id = match event.get("channel").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => return,
        };
        let ts = match event.get("ts").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return,
        };
        let user_id = event
            .get("user")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let text = event.get("text").and_then(|v| v.as_str()).unwrap_or("");

        let (file_summary, file_metadata, media_type) = if subtype == Some("file_share") {
            let raw_files = event
                .get("files")
                .and_then(|f| f.as_array())
                .cloned()
                .unwrap_or_default();

            let summary: Option<String> = if raw_files.is_empty() {
                None
            } else {
                let descs: Vec<String> = raw_files
                    .iter()
                    .map(|f| {
                        let name = f
                            .get("name")
                            .or_else(|| f.get("title"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("file");
                        let icon = match f.get("mimetype").and_then(|v| v.as_str()) {
                            Some(m) if m.starts_with("image/") => "🖼️",
                            Some(m) if m.starts_with("video/") => "🎬",
                            Some(m) if m.starts_with("audio/") => "🎵",
                            _ => "📎",
                        };
                        format!("{icon} {name}")
                    })
                    .collect();
                Some(descs.join(", ")).filter(|s| !s.is_empty())
            };

            let mtype = raw_files.first().and_then(|f| {
                f.get("mimetype").and_then(|v| v.as_str()).map(|m| {
                    if m.starts_with("image/") {
                        "image"
                    } else if m.starts_with("video/") {
                        "video"
                    } else if m.starts_with("audio/") {
                        "audio"
                    } else {
                        "file"
                    }
                    .to_string()
                })
            });

            let files_json: Vec<serde_json::Value> = raw_files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "id": f.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": f.get("name").and_then(|v| v.as_str()),
                        "title": f.get("title").and_then(|v| v.as_str()),
                        "mimetype": f.get("mimetype").and_then(|v| v.as_str()),
                        "filetype": f.get("filetype").and_then(|v| v.as_str()),
                        "size": f.get("size").and_then(|v| v.as_u64()),
                        "url_private": f.get("url_private").and_then(|v| v.as_str()),
                        "permalink": f.get("permalink").and_then(|v| v.as_str()),
                    })
                })
                .collect();

            (summary, Some(files_json), mtype)
        } else {
            (None, None, None)
        };

        if text.is_empty() && file_summary.is_none() {
            return;
        }

        let sender_name = user_cache
            .get(user_id)
            .cloned()
            .unwrap_or_else(|| user_id.to_string());

        let conv_id = format!("{}-{}", self.connection_id, channel_id);

        if self
            .ensure_conversation_exists(db, channel_id, &conv_id, user_cache)
            .await
            .is_err()
        {
            return;
        }

        let thread_ts = event.get("thread_ts").and_then(|v| v.as_str());
        let context_id = thread_ts.map(|tts| format!("{}-thread-{tts}", self.connection_id));

        let body = match (&file_summary, text.is_empty()) {
            (Some(files), true) => files.clone(),
            (Some(files), false) => format!("{text}\n{files}"),
            _ => text.to_string(),
        };

        let timestamp = parse_ts(ts).unwrap_or(0);

        let metadata = file_metadata.map(|files| {
            serde_json::json!({
                "channel_id": channel_id,
                "files": files,
            })
        });

        let mut message = Message {
            id: format!("{}-{}", self.connection_id, ts),
            conversation_id: conv_id.clone(),
            connection_id: self.connection_id.clone(),
            connector: "slack".into(),
            external_id: ts.to_string(),
            sender: user_id.to_string(),
            sender_name: Some(sender_name.clone()),
            body: Some(body),
            timestamp,
            synced_at: None,
            is_archived: false,
            reply_to_id: thread_ts.map(|tts| format!("{}-{tts}", self.connection_id)),
            media_type,
            metadata,
            context_id,
            context: None,
        };

        self.download_message_files(std::slice::from_mut(&mut message))
            .await;

        match db.upsert_message(&message) {
            Ok(_) => {
                let conv_name = db
                    .get_conversation(&conv_id)
                    .ok()
                    .flatten()
                    .and_then(|c| c.name)
                    .unwrap_or_else(|| channel_id.to_string());
                let time = chrono::DateTime::from_timestamp(timestamp, 0)
                    .map(|utc| utc.with_timezone(&chrono::Local))
                    .map(|local| local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
                    .unwrap_or_default();
                let preview: String = message
                    .body
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect();
                eprintln!(
                    "[slack:{}] {} {} — {}: {}",
                    self.connection_id, time, conv_name, sender_name, preview
                );
            }
            Err(e) => {
                eprintln!(
                    "[slack:{}] error storing message {}: {}",
                    self.connection_id, ts, e
                );
            }
        }
    }

    async fn ensure_conversation_exists(
        &self,
        db: &Database,
        channel_id: &str,
        conv_id: &str,
        user_cache: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        if db.get_conversation(conv_id)?.is_some() {
            return Ok(());
        }

        debug!(
            channel_id,
            "conversation not in DB, fetching via conversations.info"
        );
        match self.api.conversations_info(channel_id).await {
            Ok(slack_conv) => {
                let conversation = map_conversation(&slack_conv, &self.connection_id, user_cache);
                db.upsert_conversation(&conversation)?;
                debug!(conv_id, "created conversation from Socket Mode event");
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "[slack:{}] failed to fetch conversation {}: {}",
                    self.connection_id, channel_id, e
                );
                Err(e.into())
            }
        }
    }
}
