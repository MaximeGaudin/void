use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::db::Database;
use crate::models::{ConnectorType, HealthStatus, MessageContent};

#[async_trait]
pub trait Connector: Send + Sync {
    fn connector_type(&self) -> ConnectorType;
    fn account_id(&self) -> &str;

    /// Run the interactive authentication flow.
    async fn authenticate(&mut self) -> anyhow::Result<()>;

    /// Start continuous sync. Runs until the cancellation token is triggered.
    async fn start_sync(&self, db: Arc<Database>, cancel: CancellationToken) -> anyhow::Result<()>;

    /// Check connectivity and auth status.
    async fn health_check(&self) -> anyhow::Result<HealthStatus>;

    /// Send a new message to a recipient.
    async fn send_message(&self, to: &str, content: MessageContent) -> anyhow::Result<String>;

    /// Reply to an existing message. If `in_thread` is true, reply in-thread
    /// (Slack thread, WhatsApp quote).
    async fn reply(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String>;

    /// Mark a message as read on the remote service.
    /// `external_id` is the platform-specific message identifier.
    /// `conversation_external_id` is the platform-specific conversation/channel ID.
    async fn mark_read(
        &self,
        _external_id: &str,
        _conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Archive a message on the remote service (e.g., remove from inbox).
    /// `external_id` is the platform-specific message identifier.
    /// `conversation_external_id` is the platform-specific conversation/channel ID.
    async fn archive(
        &self,
        _external_id: &str,
        _conversation_external_id: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Forward a message to another recipient.
    /// `external_id` is the platform-specific message identifier.
    /// `conversation_external_id` is the platform-specific conversation/channel ID.
    /// `to` is the recipient (email address, channel ID, etc.).
    async fn forward(
        &self,
        _external_id: &str,
        _conversation_external_id: &str,
        _to: &str,
        _comment: Option<&str>,
    ) -> anyhow::Result<String> {
        anyhow::bail!(
            "Forward is not supported for {}",
            self.connector_type()
        )
    }
}
