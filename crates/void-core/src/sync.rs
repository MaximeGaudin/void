use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::channel::Channel;
use crate::db::Database;

pub struct SyncEngine {
    channels: Vec<Arc<dyn Channel>>,
    db: Arc<Database>,
    lock_path: std::path::PathBuf,
}

impl SyncEngine {
    pub fn new(channels: Vec<Arc<dyn Channel>>, db: Arc<Database>, store_path: &Path) -> Self {
        Self {
            channels,
            db,
            lock_path: store_path.join("LOCK"),
        }
    }

    /// Run all channel syncs concurrently until cancelled or interrupted.
    pub async fn run(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let _lock = self.acquire_lock()?;

        if self.channels.is_empty() {
            warn!("no channels configured, nothing to sync");
            return Ok(());
        }

        info!("starting sync for {} channel(s)", self.channels.len());

        let mut handles = Vec::new();
        for channel in &self.channels {
            let db = Arc::clone(&self.db);
            let cancel = cancel.clone();
            let channel = Arc::clone(channel);

            let handle = tokio::spawn(async move {
                let account_id = channel.account_id().to_string();
                let channel_type = channel.channel_type();
                info!(%account_id, %channel_type, "starting sync");
                match channel.start_sync(db, cancel).await {
                    Ok(()) => info!(%account_id, %channel_type, "sync stopped"),
                    Err(e) => error!(%account_id, %channel_type, "sync error: {e}"),
                }
            });
            handles.push(handle);
        }

        let cancel_on_signal = cancel.clone();
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("received interrupt, shutting down...");
            cancel_on_signal.cancel();
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
