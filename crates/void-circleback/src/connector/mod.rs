mod sync;

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::api::CirclebackClient;
use crate::CONNECTOR_ID;

pub use sync::{
    build_action_items_message, build_conversation, build_notes_message, build_transcript_messages,
};

pub struct CirclebackConnector {
    config_id: String,
    api_key: String,
    backfill_days: u32,
    include_transcript: bool,
    poll_interval_secs: u64,
}

impl CirclebackConnector {
    pub fn new(
        connection_id: &str,
        api_key: impl Into<String>,
        backfill_days: u32,
        include_transcript: bool,
        poll_interval_secs: u64,
    ) -> Self {
        Self {
            config_id: connection_id.to_string(),
            api_key: api_key.into(),
            backfill_days,
            include_transcript,
            poll_interval_secs,
        }
    }

    fn client(&self) -> CirclebackClient {
        CirclebackClient::new(self.api_key.clone())
    }
}

#[async_trait]
impl Connector for CirclebackConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    async fn authenticate(&mut self) -> anyhow::Result<()> {
        self.client().list_meetings(None).await.map(|_| ())
    }

    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()> {
        sync::run_sync(
            &db,
            &self.config_id,
            self.client(),
            self.backfill_days,
            self.include_transcript,
            self.poll_interval_secs,
            cancel,
        )
        .await
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        let (ok, message) = match self.client().list_meetings(None).await {
            Ok(page) => (
                true,
                format!(
                    "API key valid ({} meetings on the first page)",
                    page.meetings.len()
                ),
            ),
            Err(e) => (false, format!("Circleback API unreachable: {e}")),
        };
        Ok(HealthStatus {
            connection_id: self.config_id.clone(),
            connector_type: ConnectorType::from_static(CONNECTOR_ID),
            ok,
            message,
            last_sync: None,
            message_count: None,
        })
    }

    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("Circleback is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("Circleback is a read-only connector")
    }
}
