use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::connector::Connector;
use crate::db::Database;
use crate::hooks::HookRunner;

use super::lock::FileLock;

const MAX_CONSECUTIVE_FAILURES: u32 = 10;
const STABLE_THRESHOLD: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

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
    /// Connectors that fail are **not** restarted — use [`run_supervised`] for
    /// automatic restart with backoff.
    pub async fn run(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        self.run_inner(cancel, false, None).await
    }

    /// Like [`run`], but each connector is automatically restarted on failure
    /// with exponential backoff (5 s → 300 s, reset after 60 s of stable
    /// uptime, give up after 10 consecutive failures).
    pub async fn run_supervised(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        self.run_inner(cancel, true, None).await
    }

    /// Like [`run_supervised`], but reusing a lock the caller already holds.
    ///
    /// Callers that set up process-wide resources before syncing (the WhatsApp
    /// RPC endpoint, database wipes) must take the lock with [`acquire_lock`]
    /// *first*, so a second `void sync` bails out before touching anything the
    /// running daemon owns.
    pub async fn run_supervised_with_lock(
        &self,
        cancel: CancellationToken,
        lock: FileLock,
    ) -> anyhow::Result<()> {
        self.run_inner(cancel, true, Some(lock)).await
    }

    async fn run_inner(
        &self,
        cancel: CancellationToken,
        supervised: bool,
        lock: Option<FileLock>,
    ) -> anyhow::Result<()> {
        let _lock = match lock {
            Some(lock) => lock,
            None => self.acquire_lock()?,
        };

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

        info!(
            "starting {} sync for {} connector(s)",
            if supervised { "supervised" } else { "one-shot" },
            self.connectors.len()
        );

        let mut handles = Vec::new();
        for conn in &self.connectors {
            let db = Arc::clone(&self.db);
            let cancel = cancel.clone();
            let conn = Arc::clone(conn);

            let handle = if supervised {
                tokio::spawn(async move {
                    supervise_connector(conn, db, cancel).await;
                })
            } else {
                tokio::spawn(async move {
                    let connection_id = conn.connection_id().to_string();
                    let connector_type = conn.connector_type();
                    info!(%connection_id, %connector_type, "starting sync");
                    match conn.start_sync(db, cancel).await {
                        Ok(()) => info!(%connection_id, %connector_type, "sync stopped"),
                        Err(e) => error!(%connection_id, %connector_type, "sync error: {e}"),
                    }
                })
            };
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

    /// Take the single-instance lock without starting any sync.
    pub fn acquire_lock(&self) -> anyhow::Result<FileLock> {
        FileLock::acquire(&self.lock_path)
    }
}

async fn supervise_connector(
    conn: Arc<dyn Connector>,
    db: Arc<Database>,
    cancel: CancellationToken,
) {
    let connection_id = conn.connection_id().to_string();
    let connector_type = conn.connector_type();
    let mut failures = 0u32;

    loop {
        if cancel.is_cancelled() {
            break;
        }

        info!(%connection_id, %connector_type, attempt = failures + 1, "starting sync");
        let started = Instant::now();
        match conn.start_sync(Arc::clone(&db), cancel.clone()).await {
            Ok(()) => {
                info!(%connection_id, %connector_type, "sync stopped cleanly");
                break;
            }
            Err(e) => {
                if cancel.is_cancelled() {
                    info!(%connection_id, %connector_type, "sync cancelled");
                    break;
                }

                if started.elapsed() > STABLE_THRESHOLD {
                    failures = 0;
                }
                failures += 1;

                if failures >= MAX_CONSECUTIVE_FAILURES {
                    error!(
                        %connection_id, %connector_type,
                        "sync failed {failures} consecutive times, giving up: {e}"
                    );
                    break;
                }

                let delay = backoff_delay(failures);
                warn!(
                    %connection_id, %connector_type, attempt = failures,
                    "sync error: {e} — restarting in {delay:?}"
                );
                eprintln!(
                    "[{connector_type}:{connection_id}] sync error, restarting in {delay:?}: {e}"
                );

                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        }
    }
}

fn backoff_delay(attempt: u32) -> Duration {
    Duration::from_secs(5u64.saturating_mul(2u64.saturating_pow(attempt.saturating_sub(1).min(6))))
        .min(MAX_BACKOFF)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_delay_grows_exponentially() {
        assert_eq!(backoff_delay(1), Duration::from_secs(5));
        assert_eq!(backoff_delay(2), Duration::from_secs(10));
        assert_eq!(backoff_delay(3), Duration::from_secs(20));
        assert_eq!(backoff_delay(4), Duration::from_secs(40));
    }

    #[test]
    fn backoff_delay_caps_at_max() {
        assert!(backoff_delay(10) <= MAX_BACKOFF);
        assert!(backoff_delay(20) <= MAX_BACKOFF);
    }
}
