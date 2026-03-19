mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

pub struct HackerNewsConnector {
    config_id: String,
    keywords: Vec<String>,
    min_score: u32,
    poll_interval_secs: u64,
}

impl HackerNewsConnector {
    pub fn new(
        connection_id: &str,
        keywords: Vec<String>,
        min_score: u32,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            keywords: keywords.iter().map(|k| k.to_lowercase()).collect(),
            min_score,
            poll_interval_secs,
        }
    }
}

#[async_trait]
impl Connector for HackerNewsConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::HackerNews
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            &self.keywords,
            self.min_score,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
            connector_type: ConnectorType::HackerNews,
            ok: true,
            message: "HN API is public, no auth required".to_string(),
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("Hacker News is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Hacker News is a read-only connector")
    }
}
