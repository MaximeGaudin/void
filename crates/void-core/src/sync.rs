use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::connector::Connector;
use crate::db::Database;

pub struct SyncEngine {
    connectors: Vec<Arc<dyn Connector>>,
    db: Arc<Database>,
    lock_path: std::path::PathBuf,
}

impl SyncEngine {
    pub fn new(connectors: Vec<Arc<dyn Connector>>, db: Arc<Database>, store_path: &Path) -> Self {
        Self {
            connectors,
            db,
            lock_path: store_path.join("LOCK"),
        }
    }

    /// Run all connector syncs concurrently until cancelled or interrupted.
    pub async fn run(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let _lock = self.acquire_lock()?;

        if self.connectors.is_empty() {
            warn!("no connectors configured, nothing to sync");
            return Ok(());
        }

        info!("starting sync for {} connector(s)", self.connectors.len());

        let mut handles = Vec::new();
        for conn in &self.connectors {
            let db = Arc::clone(&self.db);
            let cancel = cancel.clone();
            let conn = Arc::clone(conn);

            let handle = tokio::spawn(async move {
                let account_id = conn.account_id().to_string();
                let connector_type = conn.connector_type();
                info!(%account_id, %connector_type, "starting sync");
                match conn.start_sync(db, cancel).await {
                    Ok(()) => info!(%account_id, %connector_type, "sync stopped"),
                    Err(e) => error!(%account_id, %connector_type, "sync error: {e}"),
                }
            });
            handles.push(handle);
        }

        let cancel_on_signal = cancel.clone();
        tokio::spawn(async move {
            wait_for_shutdown_signal().await;
            eprintln!("\nShutting down gracefully... (press Ctrl+C again to force quit)");
            info!("received shutdown signal, shutting down...");
            cancel_on_signal.cancel();

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nForce exiting.");
                    std::process::exit(1);
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                    eprintln!("Graceful shutdown timed out, force exiting.");
                    std::process::exit(1);
                }
            }
        });

        for handle in handles {
            handle.await.ok();
        }

        info!("all syncs stopped");
        Ok(())
    }

    fn acquire_lock(&self) -> anyhow::Result<FileLock> {
        FileLock::acquire(&self.lock_path)
    }
}

/// Wait for either SIGINT (Ctrl+C) or SIGTERM.
async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl_c");
    }
}

/// Simple file-based lock to prevent multiple sync instances.
struct FileLock {
    path: std::path::PathBuf,
}

impl FileLock {
    fn acquire(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            anyhow::bail!(
                "another sync instance appears to be running (lock file: {}, content: {}). \
                 If this is stale, delete the lock file and retry.",
                path.display(),
                content.trim()
            );
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let pid = std::process::id();
        std::fs::write(path, format!("pid={pid}"))?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_lock_acquire_and_release() {
        let dir = std::env::temp_dir().join(format!("void-lock-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let lock_path = dir.join("LOCK");

        {
            let _lock = FileLock::acquire(&lock_path).unwrap();
            assert!(lock_path.exists());

            let result = FileLock::acquire(&lock_path);
            assert!(result.is_err());
        }

        assert!(!lock_path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
