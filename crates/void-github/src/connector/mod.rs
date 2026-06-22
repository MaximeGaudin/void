mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::CONNECTOR_ID;

pub struct GitHubConnector {
    config_id: String,
    token: String,
    username: String,
    poll_interval_secs: u64,
}

impl GitHubConnector {
    pub fn new(
        connection_id: &str,
        token: String,
        username: String,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            token,
            username,
            poll_interval_secs,
        }
    }
}

#[async_trait]
impl Connector for GitHubConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let client = crate::api::GitHubClient::new(&self.token);
        let user = client.current_user().await?;
        if !self.username.eq_ignore_ascii_case(&user.login) {
            tracing::warn!(
                configured = %self.username,
                actual = %user.login,
                "GitHub username differs from configured value; using configured username"
            );
        }
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            &self.token,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let client = crate::api::GitHubClient::new(&self.token);
        match client.current_user().await {
            Ok(user) => Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::from_static(CONNECTOR_ID),
                ok: true,
                message: format!("Authenticated as @{}", user.login),
                last_sync: None,
                message_count: None,
            }),
            Err(e) => Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::from_static(CONNECTOR_ID),
                ok: false,
                message: e.to_string(),
                last_sync: None,
                message_count: None,
            }),
        }
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("GitHub is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("GitHub is a read-only connector")
    }
}
