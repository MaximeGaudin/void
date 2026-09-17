#[cfg(target_os = "macos")]
pub mod store;
#[cfg(target_os = "macos")]
mod sync;
pub mod typedstream;

#[cfg(all(test, target_os = "macos"))]
mod store_tests;
#[cfg(test)]
mod typedstream_tests;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use void_core::connector::Connector;
use void_core::db::Database;
use void_core::models::{ConnectorType, HealthStatus, MessageContent};

use crate::CONNECTOR_ID;

/// Off macOS the two store fields are carried but never read: every code path
/// that touches them is `#[cfg(target_os = "macos")]`, so `-D warnings` turns
/// the resulting dead-code lint into a build error on Linux and Windows. The
/// fields stay in the struct rather than being cfg-gated themselves so that
/// `new()` keeps one signature on every platform and the caller in void-cli
/// does not need its own gating.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct ImessageConnector {
    config_id: String,
    db_path: PathBuf,
    poll_interval_secs: u64,
}

impl ImessageConnector {
    pub fn new(connection_id: &str, db_path: PathBuf, poll_interval_secs: u64) -> Self {
        Self {
            config_id: connection_id.to_string(),
            db_path,
            poll_interval_secs,
        }
    }
}

#[async_trait]
impl Connector for ImessageConnector {
    fn connector_type(&self) -> ConnectorType {
        ConnectorType::from_static(CONNECTOR_ID)
    }

    fn connection_id(&self) -> &str {
        &self.config_id
    }

    /// Nothing to authenticate: access is granted by macOS, not by a token.
    /// The interactive part is a system permission, so report what is actually
    /// blocking rather than pretending to log in.
    async fn authenticate(&mut self) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            store::open_read_only(&self.db_path)?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            anyhow::bail!("iMessage is only available on macOS")
        }
    }

    async fn start_sync(
        &self,
        _db: Arc<Database>,
        _cancel: CancellationToken,
    ) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            sync::run_sync(
                &_db,
                &self.config_id,
                &self.db_path,
                self.poll_interval_secs,
                _cancel,
            )
            .await
        }
        #[cfg(not(target_os = "macos"))]
        {
            anyhow::bail!("iMessage is only available on macOS")
        }
    }

    async fn health_check(&self) -> anyhow::Result<HealthStatus> {
        #[cfg(target_os = "macos")]
        {
            // A real bounded read, not an existence check: the file's metadata
            // is readable without Full Disk Access, so `exists()` reports
            // healthy on a store we cannot actually read.
            let (ok, message) = match store::open_read_only(&self.db_path) {
                Ok(_) => (
                    true,
                    format!("Messages store readable at {:?}", self.db_path),
                ),
                Err(err) => (false, err.to_string()),
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
        #[cfg(not(target_os = "macos"))]
        {
            Ok(HealthStatus {
                connection_id: self.config_id.clone(),
                connector_type: ConnectorType::from_static(CONNECTOR_ID),
                ok: false,
                message: "iMessage is only available on macOS".to_string(),
                last_sync: None,
                message_count: None,
            })
        }
    }

    /// Sending is deliberately absent in this connector.
    ///
    /// The only route is driving Messages.app over AppleScript, which returns
    /// success as soon as the app accepts the event, long before anything
    /// reaches Apple. Reporting that as sent would repeat the bug fixed in
    /// 0380e6e for WhatsApp. `chat.db` does expose `date_delivered`, so a real
    /// confirmation loop is possible, but it belongs in its own change.
    async fn send_message(&self, _to: &str, _content: MessageContent) -> anyhow::Result<String> {
        anyhow::bail!("iMessage is a read-only connector")
    }

    async fn reply(
        &self,
        _message_id: &str,
        _content: MessageContent,
        _in_thread: bool,
    ) -> anyhow::Result<String> {
        anyhow::bail!("iMessage is a read-only connector")
    }
}
