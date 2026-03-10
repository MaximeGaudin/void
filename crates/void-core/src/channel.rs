use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::db::Database;
use crate::models::{ChannelType, HealthStatus, MessageContent};

#[async_trait]
pub trait Channel: Send + Sync {
    fn channel_type(&self) -> ChannelType;
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
}
