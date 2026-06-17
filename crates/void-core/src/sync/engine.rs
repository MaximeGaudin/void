use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::connector::Connector;
use crate::db::Database;
use crate::hooks::HookRunner;

use super::lock::FileLock;

pub struct SyncEngine {
    connectors: Vec<Arc<dyn Connector>>,
    db: Arc<Database>,
    hook_runner: Option<Arc<HookRunner>>,
    lock_path: std::path::PathBuf,
}

impl SyncEngine {
    pub fn new(
        connectors: Vec<Arc<dyn Connector>>,
        db: Arc<Database>,
        store_path: &Path,
        hook_runner: Option<Arc<HookRunner>>,
    ) -> Self {
        Self {
            connectors,
            db,
            hook_runner,
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

        if let Some(ref runner) = self.hook_runner {
            self.db.set_hook_runner(Arc::clone(runner));
            runner.start_schedules(cancel.clone());
            let n_hooks = runner.hooks().len();
            info!(n_hooks, "hook runner attached ({n_hooks} hook(s) loaded)");
        }

        info!("starting sync for {} connector(s)", self.connectors.len());

        let mut handles = Vec::new();
        for conn in &self.connectors {
            let db = Arc::clone(&self.db);
            let cancel = cancel.clone();
            let conn = Arc::clone(conn);

            let handle = tokio::spawn(async move {
                let connection_id = conn.connection_id().to_string();
                let connector_type = conn.connector_type();
                info!(%connection_id, %connector_type, "starting sync");
                match conn.start_sync(db, cancel).await {
                    Ok(()) => info!(%connection_id, %connector_type, "sync stopped"),
                    Err(e) => error!(%connection_id, %connector_type, "sync error: {e}"),
                }
            });
            handles.push(handle);
        }

        let (shutdown_done_tx, shutdown_done_rx) = tokio::sync::oneshot::channel::<()>();

        let cancel_on_signal = cancel.clone();
        tokio::spawn(async move {
            let signal = wait_for_shutdown_signal().await;
            eprintln!("\nShutting down gracefully... (press Ctrl+C again to force quit)");
            info!(signal, "received shutdown signal, shutting down...");
            cancel_on_signal.cancel();

            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    eprintln!("\nForce exiting.");
                    std::process::exit(1);
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {
                    eprintln!("Graceful shutdown timed out, force exiting.");
                    std::process::exit(1);
                }
                _ = shutdown_done_rx => {}
            }
        });

        for handle in handles {
            handle.await.ok();
        }

        drop(shutdown_done_tx);

        info!("all syncs stopped");
        Ok(())
    }

    fn acquire_lock(&self) -> anyhow::Result<FileLock> {
        FileLock::acquire(&self.lock_path)
    }
}

/// Wait for either SIGINT (Ctrl+C) or SIGTERM and return which signal fired.
async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "SIGINT (Ctrl+C)",
            _ = sigterm.recv() => "SIGTERM",
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for ctrl_c");
        "SIGINT (Ctrl+C)"
    }
}
