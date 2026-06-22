mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{parse_reply_id, ConnectorType, HealthStatus, MessageContent};

pub struct RedditConnector {
    config_id: String,
    client_id: String,
    client_secret: String,
    refresh_token: Option<String>,
    subreddits: Vec<String>,
    keywords: Vec<String>,
    min_score: u32,
    poll_interval_secs: u64,
}

impl RedditConnector {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connection_id: &str,
        client_id: String,
        client_secret: String,
        refresh_token: Option<String>,
        subreddits: Vec<String>,
        keywords: Vec<String>,
        min_score: u32,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            client_id,
            client_secret,
            refresh_token,
            subreddits: subreddits
                .iter()
                .map(|s| crate::api::sanitize_subreddit(s))
                .filter(|s| !s.is_empty())
                .collect(),
            keywords: keywords.iter().map(|k| k.to_lowercase()).collect(),
            min_score,
            poll_interval_secs,
        }
    }

    fn client(&self) -> crate::api::RedditClient {
        crate::api::RedditClient::with_refresh_token(
            &self.client_id,
            &self.client_secret,
            self.refresh_token.clone(),
        )
    }
}

#[async_trait]
impl Connector for RedditConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(crate::CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let client = self.client();
        let _ = client.subreddit_hot("all", 1).await?;
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            &self.client_id,
            &self.client_secret,
            self.refresh_token.as_deref(),
            &self.subreddits,
            &self.keywords,
            self.min_score,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let client = self.client();
        let ok = client.subreddit_hot("all", 1).await.is_ok();
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
            connector_type: ConnectorType::from_static(crate::CONNECTOR_ID),
            ok,
            message: if ok {
                if self.refresh_token.is_some() {
                    "Reddit OAuth credentials valid (commenting enabled)".to_string()
                } else {
                    "Reddit OAuth credentials valid (read-only)".to_string()
                }
            } else {
                "Reddit OAuth check failed".to_string()
            },
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let text = content.text();
        let post_id = crate::api::extract_post_id(to)?;
        let client = self.client();
        client.post_comment(&format!("t3_{post_id}"), text).await
    }

    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        let (conv_external_id, msg_external_id) = parse_reply_id(message_id)?;
        let text = content.text();
        let client = self.client();

        let thing_id = if msg_external_id.contains("_postbody_") {
            let post_id = crate::api::extract_post_id_from_postbody_external(
                &msg_external_id,
                &self.config_id,
            )?;
            format!("t3_{post_id}")
        } else if msg_external_id.contains("_comment_") {
            let comment_id =
                crate::api::extract_comment_id_from_external(&msg_external_id, &self.config_id)?;
            format!("t1_{comment_id}")
        } else if conv_external_id.contains("_post_") {
            let post_id = crate::api::extract_post_id(&conv_external_id)?;
            format!("t3_{post_id}")
        } else {
            anyhow::bail!(
                "Reddit reply target must be a post thread comment or post body (got {message_id})"
            );
        };

        client.post_comment(&thing_id, text).await
    }
}
