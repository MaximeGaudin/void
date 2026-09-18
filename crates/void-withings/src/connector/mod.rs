mod sync;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::api::WithingsClient;
use crate::auth::token_cache_path;
use crate::{Stream, CONNECTOR_ID};

pub struct WithingsConnector {
    config_id: String,
    client_id: String,
    client_secret: String,
    token_path: PathBuf,
    streams: Vec<Stream>,
    backfill_days: u32,
    poll_interval_secs: u64,
}

impl WithingsConnector {
    pub fn new(
        connection_id: &str,
        client_id: String,
        client_secret: String,
        store_path: &Path,
        streams: Vec<Stream>,
        backfill_days: u32,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            client_id,
            client_secret,
            token_path: token_cache_path(store_path, connection_id),
            streams,
            backfill_days,
            poll_interval_secs,
        }
    }

    pub fn client(&self) -> WithingsClient {
        WithingsClient::new(&self.client_id, &self.client_secret, &self.token_path)
    }

    pub fn token_path(&self) -> &Path {
        &self.token_path
    }
}

#[async_trait]
impl Connector for WithingsConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    /// Re-run the browser flow. Withings rotates the refresh token on every
    /// refresh, so a dead grant can only be replaced by a fresh authorization.
    async fn authenticate(&mut self) -> anyhow::Result<()> {
        let client = self.client();
        client.authorize_interactive().await?;
        client.probe().await?;
        Ok(())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            self.client(),
            self.streams.clone(),
            self.backfill_days,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let message = match self.client().probe().await {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
            connector_type: ConnectorType::from_static(CONNECTOR_ID),
            ok: message.is_none(),
            message: message.unwrap_or_else(|| {
                format!(
                    "Withings credentials valid ({})",
                    self.streams
                        .iter()
                        .map(|s| s.id())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }),
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("Withings is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Withings is a read-only connector")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALL_STREAMS;

    fn connector(store: &Path) -> WithingsConnector {
        WithingsConnector::new(
            "health",
            "cid".into(),
            "secret".into(),
            store,
            ALL_STREAMS.to_vec(),
            365,
            3_600,
        )
    }

    #[test]
    fn token_path_is_scoped_to_the_connection() {
        let dir = tempfile::tempdir().unwrap();
        let connector = connector(dir.path());
        assert!(connector
            .token_path()
            .ends_with("health-withings-token.json"));
        assert_eq!(connector.connector_type().as_str(), "withings");
        assert_eq!(connector.connection_id(), "health");
    }

    #[tokio::test]
    async fn sending_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let connector = connector(dir.path());
        let err = connector
            .send_message("anyone", MessageContent::from_text("hi"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("read-only"));
    }

    #[tokio::test]
    async fn health_check_without_a_token_explains_itself() {
        let dir = tempfile::tempdir().unwrap();
        let health = connector(dir.path()).health_check().await.unwrap();
        assert!(!health.ok);
        assert!(health.message.contains("void setup"));
    }
}
