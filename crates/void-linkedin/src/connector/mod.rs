mod extract;
mod media;
mod posts_sync;
mod profiles;
mod send;
mod sync;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::info;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::api::UnipileClient;
use crate::error::LinkedInError;
use crate::CONNECTOR_ID;

pub struct LinkedInConnector {
    config_id: String,
    api_key: String,
    dsn: String,
    account_id: String,
    poll_interval_secs: u64,
    backfill_days: u64,
}

impl LinkedInConnector {
    pub fn new(
        connection_id: &str,
        api_key: &str,
        dsn: &str,
        account_id: &str,
        poll_interval_secs: u64,
        backfill_days: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            api_key: api_key.to_string(),
            dsn: dsn.to_string(),
            account_id: account_id.to_string(),
            poll_interval_secs,
            backfill_days,
        }
    }

    fn client(&self) -> UnipileClient {
        UnipileClient::new(&self.dsn, &self.api_key)
    }

    pub async fn download_media(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, LinkedInError> {
        media::download_media(&self.client(), message_id, attachment_id).await
    }
}

#[async_trait]
impl Connector for LinkedInConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let client = self.client();
        let account = client
            .get_account(&self.account_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let account_type = account.r#type.as_deref().unwrap_or("unknown");
        info!(
            connection_id = %self.config_id,
            account_id = %self.account_id,
            account_type,
            "LinkedIn (Unipile) authenticated"
        );
        eprintln!("  ✓ LinkedIn account connected via Unipile (type: {account_type})");
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &self.client(),
            &self.account_id,
            &db,
            &self.config_id,
            self.poll_interval_secs,
            self.backfill_days,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        match self.client().get_account(&self.account_id).await {
            Ok(account) => {
                let account_type = account.r#type.as_deref().unwrap_or("unknown");
                Ok(HealthStatus {
                    connection_id: self.config_id.clone(),
                    connector_type: ConnectorType::from_static(CONNECTOR_ID),
                    ok: true,
                    message: format!(
                        "Unipile account {} connected ({account_type})",
                        self.account_id
                    ),
                    last_sync: None,
                    message_count: None,
                })
            }
            Err(e) => Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::from_static(CONNECTOR_ID),
                ok: false,
                message: format!("Unipile health check failed: {e}"),
                last_sync: None,
                message_count: None,
            }),
        }
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let client = self.client();
        let text = content.text();
        let file = send::file_path_for_message_content(&content);

        // LinkedIn provider member IDs (for new 1:1 chats) typically start with ACo/AE.
        let is_new_recipient = to.starts_with("ACo") || to.starts_with("AE");

        if is_new_recipient {
            let msg_id = client
                .start_new_chat(&self.account_id, to, text, file)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            return Ok(msg_id);
        }

        let chat_id = send::chat_id_from_conv_external(&self.config_id, to)
            .unwrap_or_else(|_| to.to_string());
        let msg_id = client
            .send_message_in_chat(&chat_id, text, file)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(msg_id)
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        let (conv_ext_id, msg_ext_id) = void_core::models::parse_reply_id(message_id)?;
        let text = content.text();
        let file = send::file_path_for_message_content(&content);
        if file.is_some() {
            anyhow::bail!("LinkedIn post comment replies do not support file attachments yet");
        }

        if send::is_post_conversation(&conv_ext_id) {
            let post_id = send::post_id_from_conv_external(&self.config_id, &conv_ext_id)?;
            let comment_id = send::comment_id_from_msg_external(&self.config_id, &msg_ext_id)?;
            let post = self
                .client()
                .get_post(&self.account_id, &post_id)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            if post.social_id.is_empty() {
                anyhow::bail!("post {post_id} has no social_id for comment reply");
            }
            let msg_id = self
                .client()
                .send_post_comment(&self.account_id, &post.social_id, text, Some(&comment_id))
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            return Ok(msg_id);
        }

        let chat_id = send::chat_id_from_conv_external(&self.config_id, &conv_ext_id)?;
        let msg_id = self
            .client()
            .send_message_in_chat(&chat_id, text, file)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(msg_id)
    }
}
