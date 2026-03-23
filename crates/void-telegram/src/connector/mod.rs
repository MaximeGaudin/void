mod extract;
mod media;
mod send;
mod sync;

use std::sync::Arc;

use crate::session::JsonFileSession;
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use grammers_client::client::Client;
use grammers_client::message::Message as TgMessage;
use grammers_mtsender::{SenderPool, SenderPoolFatHandle};
use grammers_tl_types as tl;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::error::TelegramError;

const DEFAULT_API_ID: i32 = 2040;
const DEFAULT_API_HASH: &str = "b18441a1ff607e10a989891a5462e627";

pub struct TelegramConnector {
    config_id: String,
    session_path: String,
    api_id: i32,
    api_hash: String,
}

impl TelegramConnector {
    pub fn new(
        connection_id: &str,
        session_path: &str,
        api_id: Option<i32>,
        api_hash: Option<&str>,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            session_path: session_path.to_string(),
            api_id: api_id.unwrap_or(DEFAULT_API_ID),
            api_hash: api_hash.unwrap_or(DEFAULT_API_HASH).to_string(),
        }
    }

    fn connect(&self) -> anyhow::Result<(Client, SenderPool)> {
        let session = Arc::new(JsonFileSession::load_or_create(&self.session_path));
        let pool = SenderPool::new(Arc::clone(&session), self.api_id);
        let client = Client::new(pool.handle.clone());

        Ok((client, pool))
    }

    pub async fn download_media(
        &self,
        message_id: i32,
        chat_id: i64,
    ) -> Result<Vec<u8>, TelegramError> {
        let (client, pool) = self
            .connect()
            .map_err(|e| TelegramError::Connection(e.to_string()))?;

        let runner = tokio::spawn(pool.runner.run());

        let results = client
            .search_peer(&chat_id.to_string(), 1)
            .await
            .map_err(|e| TelegramError::Connection(e.to_string()))?;

        let peer = results
            .into_iter()
            .next()
            .ok_or_else(|| TelegramError::Media(format!("peer not found: {chat_id}")))?
            .into_peer();

        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| TelegramError::Media("could not resolve peer ref".into()))?;

        let messages = client
            .get_messages_by_id(peer_ref, &[message_id])
            .await
            .map_err(|e| TelegramError::Media(e.to_string()))?;

        let msg = messages
            .into_iter()
            .flatten()
            .next()
            .ok_or_else(|| TelegramError::Media(format!("message not found: {message_id}")))?;

        let downloadable = msg
            .media()
            .ok_or_else(|| TelegramError::Media("message has no media".into()))?;

        let bytes = media::download_media_to_bytes(&client, &downloadable).await?;

        client.disconnect();
        runner.abort();

        Ok(bytes)
    }
}

#[async_trait]
impl Connector for TelegramConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Telegram
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let (client, pool) = self.connect()?;
        let handle = pool.handle.clone();
        let runner = tokio::spawn(pool.runner.run());

        if client.is_authorized().await? {
            info!(connection_id = %self.config_id, "already authenticated");
            client.disconnect();
            runner.abort();
            return Ok(());
        }

        eprintln!("Scan this QR code with Telegram on your phone:");
        eprintln!("  Open Telegram > Settings > Devices > Link Desktop Device\n");

        let result = qr_login_loop(&client, &handle, &self.api_hash, self.api_id).await;

        client.disconnect();
        runner.abort();

        result
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        let (client, pool) = self.connect()?;
        let runner = tokio::spawn(pool.runner.run());

        if !client.is_authorized().await? {
            anyhow::bail!(
                "Telegram connection '{}' is not authenticated. Run `void setup` first.",
                self.config_id
            );
        }

        info!(connection_id = %self.config_id, "starting telegram sync");

        let result = sync::run_sync(&client, pool.updates, &db, &self.config_id, &cancel).await;

        client.disconnect();
        runner.abort();

        result
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let session_exists = std::path::Path::new(&self.session_path).exists();

        if !session_exists {
            return Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::Telegram,
                ok: false,
                message: "Session file not found".to_string(),
                last_sync: None,
                message_count: None,
            });
        }

        match self.connect() {
            Ok((client, pool)) => {
                let runner = tokio::spawn(pool.runner.run());
                let authorized = client.is_authorized().await.unwrap_or(false);
                client.disconnect();
                runner.abort();

                if authorized {
                    Ok(HealthStatus {
                        connection_id: self.config_id.clone(),
                        connector_type: ConnectorType::Telegram,
                        ok: true,
                        message: "Connected and authenticated".to_string(),
                        last_sync: None,
                        message_count: None,
                    })
                } else {
                    Ok(HealthStatus {
                        connection_id: self.config_id.clone(),
                        connector_type: ConnectorType::Telegram,
                        ok: false,
                        message: "Session exists but not authorized. Run `void setup`.".to_string(),
                        last_sync: None,
                        message_count: None,
                    })
                }
            }
            Err(e) => Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::Telegram,
                ok: false,
                message: format!("Connection failed: {e}"),
                last_sync: None,
                message_count: None,
            }),
        }
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let (client, pool) = self.connect()?;
        let runner = tokio::spawn(pool.runner.run());

        let peer = send::resolve_peer(&client, to).await?;
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("could not resolve peer ref"))?;

        let msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                media::upload_and_build_media_message(
                    &client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                )
                .await?
            }
            _ => send::build_input_message(&content),
        };

        let sent: TgMessage = client.send_message(peer_ref, msg).await?;
        let msg_id = sent.id().to_string();

        debug!(msg_id, to, "telegram message sent");

        client.disconnect();
        runner.abort();

        Ok(msg_id)
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        let (conv_ext_id, msg_ext_id) = send::parse_reply_id(message_id)?;

        let raw_msg_id: i32 = msg_ext_id
            .strip_prefix(&format!("telegram_{}_", self.config_id))
            .unwrap_or(&msg_ext_id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid message ID: {msg_ext_id}"))?;

        let raw_chat_id: i64 = conv_ext_id
            .strip_prefix(&format!("telegram_{}_", self.config_id))
            .unwrap_or(&conv_ext_id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid conversation ID: {conv_ext_id}"))?;

        let (client, pool) = self.connect()?;
        let runner = tokio::spawn(pool.runner.run());

        let results = client.search_peer(&raw_chat_id.to_string(), 1).await?;
        let peer = results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("could not resolve chat {raw_chat_id}"))?
            .into_peer();
        let peer_ref = peer
            .to_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("could not resolve peer ref"))?;

        let mut msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                media::upload_and_build_media_message(
                    &client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                )
                .await?
            }
            _ => send::build_input_message(&content),
        };
        msg = msg.reply_to(Some(raw_msg_id));

        let sent: TgMessage = client.send_message(peer_ref, msg).await?;
        let sent_id = sent.id().to_string();

        debug!(sent_id, reply_to = raw_msg_id, "telegram reply sent");

        client.disconnect();
        runner.abort();

        Ok(sent_id)
    }

    async fn mark_read(
        &self,
        _external_id: &str,
        conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        let raw_chat_id: i64 = conversation_external_id
            .strip_prefix(&format!("telegram_{}_", self.config_id))
            .unwrap_or(conversation_external_id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid conversation ID: {conversation_external_id}"))?;

        let (client, pool) = self.connect()?;
        let runner = tokio::spawn(pool.runner.run());

        let results = client.search_peer(&raw_chat_id.to_string(), 1).await?;
        if let Some(item) = results.into_iter().next() {
            let peer = item.into_peer();
            if let Some(peer_ref) = peer.to_ref().await {
                client.mark_as_read(peer_ref).await?;
            }
        }

        client.disconnect();
        runner.abort();

        Ok(())
    }

    async fn forward(
        &self,
        external_id: &str,
        conversation_external_id: &str,
        to: &str,
        _comment: Option<&str>,
    ) -> anyhow::Result<String> {
        let raw_msg_id: i32 = external_id
            .strip_prefix(&format!("telegram_{}_", self.config_id))
            .unwrap_or(external_id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid message ID: {external_id}"))?;

        let raw_chat_id: i64 = conversation_external_id
            .strip_prefix(&format!("telegram_{}_", self.config_id))
            .unwrap_or(conversation_external_id)
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid conversation ID: {conversation_external_id}"))?;

        let (client, pool) = self.connect()?;
        let runner = tokio::spawn(pool.runner.run());

        let source_results = client.search_peer(&raw_chat_id.to_string(), 1).await?;
        let source = source_results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("could not resolve source chat {raw_chat_id}"))?
            .into_peer();
        let source_ref = source
            .to_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("could not resolve source peer ref"))?;

        let dest = send::resolve_peer(&client, to).await?;
        let dest_ref = dest
            .to_ref()
            .await
            .ok_or_else(|| anyhow::anyhow!("could not resolve destination peer ref"))?;

        let forwarded: Vec<Option<TgMessage>> = client
            .forward_messages(dest_ref, &[raw_msg_id], source_ref)
            .await?;

        let fwd_id = forwarded
            .into_iter()
            .flatten()
            .next()
            .map(|m| m.id().to_string())
            .unwrap_or_else(|| "forwarded".to_string());

        client.disconnect();
        runner.abort();

        Ok(fwd_id)
    }
}

async fn qr_login_loop(
    client: &Client,
    handle: &SenderPoolFatHandle,
    api_hash: &str,
    api_id: i32,
) -> anyhow::Result<()> {
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
    const MAX_ATTEMPTS: usize = 60;

    for attempt in 0..MAX_ATTEMPTS {
        let export = tl::functions::auth::ExportLoginToken {
            api_id,
            api_hash: api_hash.to_string(),
            except_ids: vec![],
        };

        let response = client.invoke(&export).await?;

        match response {
            tl::enums::auth::LoginToken::Token(token) => {
                if attempt % 5 == 0 {
                    let encoded = URL_SAFE_NO_PAD.encode(&token.token);
                    let url = format!("tg://login?token={encoded}");
                    render_qr(&url);
                }
                debug!(attempt, "polling for QR code scan");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            tl::enums::auth::LoginToken::MigrateTo(migrate) => {
                let old_dc = handle.session.home_dc_id();
                debug!(
                    old_dc,
                    new_dc = migrate.dc_id,
                    "QR scan detected, migrating home DC"
                );
                handle.thin.disconnect_from_dc(old_dc);
                handle.session.set_home_dc_id(migrate.dc_id).await;

                let import = tl::functions::auth::ImportLoginToken {
                    token: migrate.token,
                };
                match client.invoke_in_dc(migrate.dc_id, &import).await {
                    Ok(tl::enums::auth::LoginToken::Success(success)) => {
                        log_auth_success(&success);
                        return Ok(());
                    }
                    Ok(_) => anyhow::bail!("unexpected response after DC migration"),
                    Err(e) => {
                        warn!(error = %e, "DC migration import failed, retrying with fresh token");
                        continue;
                    }
                }
            }
            tl::enums::auth::LoginToken::Success(success) => {
                log_auth_success(&success);
                return Ok(());
            }
        }
    }
    anyhow::bail!("QR login timed out after {MAX_ATTEMPTS} attempts — please retry")
}

fn log_auth_success(success: &tl::types::auth::LoginTokenSuccess) {
    match &success.authorization {
        tl::enums::auth::Authorization::Authorization(auth) => {
            if let tl::enums::User::User(user) = &auth.user {
                let name = user.first_name.as_deref().unwrap_or("Unknown");
                info!(user = %name, "telegram QR sign-in successful");
                eprintln!("\nSigned in as {name}");
            }
        }
        tl::enums::auth::Authorization::SignUpRequired(_) => {
            warn!("QR login returned sign-up required (unexpected)");
        }
    }
}

fn render_qr(url: &str) {
    if let Err(e) = qr2term::print_qr(url) {
        eprintln!("Could not render QR code: {e}");
        eprintln!("Open this URL in a QR reader: {url}");
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramConnector;
    use void_core::connector::Connector;
    use void_core::models::ConnectorType;

    #[test]
    fn telegram_connector_new_sets_ids() {
        let session_path = std::env::temp_dir().join("tg.json");
        let c = TelegramConnector::new("conn-a", &session_path.to_string_lossy(), None, None);
        assert_eq!(c.connection_id(), "conn-a");
        assert_eq!(c.connector_type(), ConnectorType::Telegram);
    }
}
