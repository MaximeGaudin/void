mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

pub struct RedditConnector {
    config_id: String,
    client_id: String,
    client_secret: String,
    subreddits: Vec<String>,
    keywords: Vec<String>,
    min_score: u32,
    poll_interval_secs: u64,
}

impl RedditConnector {
    pub fn new(
        connection_id: &str,
        client_id: String,
        client_secret: String,
        subreddits: Vec<String>,
        keywords: Vec<String>,
        min_score: u32,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            client_id,
            client_secret,
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
}

#[async_trait]
impl Connector for RedditConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::Reddit
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let client = crate::api::RedditClient::new(&self.client_id, &self.client_secret);
        let _ = client.subreddit_hot("all", 1).await?;
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            &self.client_id,
            &self.client_secret,
            &self.subreddits,
            &self.keywords,
            self.min_score,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let client = crate::api::RedditClient::new(&self.client_id, &self.client_secret);
        let ok = client.subreddit_hot("all", 1).await.is_ok();
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
            connector_type: ConnectorType::Reddit,
            ok,
            message: if ok {
                "Reddit OAuth credentials valid".to_string()
            } else {
                "Reddit OAuth check failed".to_string()
            },
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("Reddit is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Reddit is a read-only connector")
    }
}
