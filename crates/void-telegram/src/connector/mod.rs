mod connector_trait;
mod extract;
mod media;
mod send;
mod sync;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::session::JsonFileSession;
use grammers_client::client::Client;
use grammers_mtsender::SenderPool;
use tracing::info;

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

        info!(
            connection_id = %self.config_id,
            message_id,
            chat_id,
            "downloading Telegram media"
        );

        let bytes = media::download_media_to_bytes(&client, &downloadable).await?;

        client.disconnect();
        runner.abort();

        Ok(bytes)
    }
}
