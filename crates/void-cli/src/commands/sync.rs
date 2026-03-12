use std::sync::Arc;

use clap::Args;
use tokio_util::sync::CancellationToken;
use tracing::info;

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::sync::SyncEngine;

use crate::commands::connector_factory;

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Sync only specific connectors (comma-separated: whatsapp,slack,gmail,calendar)
    #[arg(long)]
    pub connectors: Option<String>,
    /// Detach and run as a background daemon
    #[arg(long)]
    pub daemon: bool,
    /// Stop any existing sync before starting this one
    #[arg(long)]
    pub restart: bool,
    /// Clear the database before syncing (fresh start)
    #[arg(long)]
    pub clear: bool,
    /// Clear data for a specific connector before syncing (e.g. whatsapp, slack, gmail, calendar)
    #[arg(long)]
    pub clear_connector: Option<String>,
    /// Stop the running sync daemon
    #[arg(long)]
    pub stop: bool,
}

/// Stop a running sync daemon by reading its PID from the lock file, sending
/// SIGTERM, waiting for it to exit, and cleaning up the lock file.
pub fn stop_daemon() -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)?;
    let lock_path = cfg.store_path().join("LOCK");

    if !lock_path.exists() {
        eprintln!("No sync daemon is running.");
        return Ok(());
    }

    let content = std::fs::read_to_string(&lock_path)?;
    let pid_str = content
        .trim()
        .strip_prefix("pid=")
        .unwrap_or(content.trim());
    let pid: i32 = pid_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID in lock file: {content}"))?;

    let process_alive = |p: i32| -> bool { unsafe { libc::kill(p, 0) == 0 } };

    if !process_alive(pid) {
        eprintln!("Daemon (pid {pid}) is no longer running. Cleaning up stale lock file.");
        std::fs::remove_file(&lock_path).ok();
        return Ok(());
    }

    eprintln!("Stopping sync daemon (pid {pid})...");
    info!(pid, "sending SIGTERM to daemon");
    unsafe {
        if libc::kill(pid, libc::SIGTERM) != 0 {
            let err = std::io::Error::last_os_error();
            anyhow::bail!("Failed to send SIGTERM to daemon (pid {pid}): {err}");
        }
    }

    const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while process_alive(pid) {
        if start.elapsed() > MAX_WAIT {
            eprintln!("Daemon did not exit within {MAX_WAIT:?}, sending SIGKILL...");
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    if lock_path.exists() {
        std::fs::remove_file(&lock_path).ok();
    }

    eprintln!("Sync daemon stopped.");
    Ok(())
}

/// Fork into a background daemon, then run sync in the child process.
/// Must be called *before* any tokio runtime is created.
pub fn daemonize(args: &SyncArgs, verbose: bool) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void setup` first.",
            config_path.display()
        )
    })?;

    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    let log_path = store_path.join("void-sync.log");
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| anyhow::anyhow!("Cannot open log file {}: {e}", log_path.display()))?;
    let log_err = log_file
        .try_clone()
        .map_err(|e| anyhow::anyhow!("Cannot clone log file handle: {e}"))?;

    let lock_path = store_path.join("LOCK");
    if lock_path.exists() {
        if args.restart {
            stop_daemon().ok();
        } else {
            let content = std::fs::read_to_string(&lock_path).unwrap_or_default();
            anyhow::bail!(
                "Sync daemon already running ({}).\nUse --restart to stop it and start a new one, or `void sync --stop` to stop it.",
                content.trim()
            );
        }
    }

    eprintln!("Starting sync daemon... logs at {}", log_path.display());

    let daemon = daemonize::Daemonize::new()
        .working_directory(&store_path)
        .stdout(log_file)
        .stderr(log_err);

    daemon
        .start()
        .map_err(|e| anyhow::anyhow!("Failed to daemonize: {e}"))?;

    // We're now in the detached child process -- build a fresh tokio runtime
    let rt = tokio::runtime::Runtime::new()?;
    let connectors_clone = args.connectors.clone();
    let clear = args.clear;
    let clear_connector = args.clear_connector.clone();
    rt.block_on(async move {
        let log_level = if verbose { "debug" } else { "info" };
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
            )
            .with_writer(std::io::stderr)
            .init();

        info!(log_path = %log_path.display(), "daemon started");

        let inner_args = SyncArgs {
            connectors: connectors_clone,
            daemon: false,
            restart: false,
            clear,
            clear_connector,
            stop: false,
        };
        if let Err(e) = run(&inner_args).await {
            tracing::error!("sync daemon error: {e}");
        }
    });
    Ok(())
}

pub async fn run(args: &SyncArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void setup` first.",
            config_path.display()
        )
    })?;

    if cfg.accounts.is_empty() {
        anyhow::bail!("No accounts configured. Add accounts to your config.toml first.");
    }

    let connector_filter: Option<Vec<String>> = args
        .connectors
        .as_ref()
        .map(|c| c.split(',').map(|s| s.trim().to_lowercase()).collect());

    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    if args.restart {
        let lock_path = store_path.join("LOCK");
        if lock_path.exists() {
            stop_daemon().ok();
        }
    }

    if args.clear {
        let db_path = cfg.db_path();
        if db_path.exists() {
            std::fs::remove_file(&db_path)?;
            eprintln!("Database cleared: {}", db_path.display());
            info!(path = %db_path.display(), "database cleared");
        }
    }

    let db = Arc::new(Database::open(&cfg.db_path())?);

    if let Some(ref connector_type) = args.clear_connector {
        let ct = connector_type.trim().to_lowercase();
        let (msgs, convs, evts, sync_st) = db.clear_connector_data(&ct)?;
        eprintln!(
            "Cleared {ct} data: {msgs} messages, {convs} conversations, {evts} events, {sync_st} sync states"
        );
        info!(
            connector = %ct, msgs, convs, evts, sync_st,
            "connector data cleared"
        );

        if ct == "whatsapp" {
            for account in &cfg.accounts {
                if account.account_type.to_string() == "whatsapp" {
                    let session_db = store_path.join(format!("whatsapp-{}.db", account.id));
                    if session_db.exists() {
                        std::fs::remove_file(&session_db)?;
                        eprintln!(
                            "Removed WhatsApp session: {} (will require re-pairing)",
                            session_db.display()
                        );
                    }
                }
            }
        }
    }

    let mut connectors: Vec<Arc<dyn void_core::connector::Connector>> = Vec::new();

    for account in &cfg.accounts {
        if let Some(ref filter) = connector_filter {
            let type_str = account.account_type.to_string();
            if !filter.iter().any(|f| type_str.contains(f)) {
                continue;
            }
        }

        match connector_factory::build_connector(account, &store_path) {
            Ok(conn) => match conn.health_check().await {
                Ok(status) if status.ok => connectors.push(conn),
                Ok(status) => {
                    eprintln!(
                        "[warn] Skipping account '{}' ({}): {}. Run `void setup` to authenticate.",
                        account.id, account.account_type, status.message
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[warn] Skipping account '{}' ({}): {e}. Run `void setup` to authenticate.",
                        account.id, account.account_type
                    );
                }
            },
            Err(e) => {
                eprintln!(
                    "[warn] Skipping account '{}' ({}): {e}",
                    account.id, account.account_type
                );
            }
        }
    }

    if connectors.is_empty() {
        anyhow::bail!("No connectors to sync (check your config and --connectors filter).");
    }

    eprintln!(
        "Starting sync for {} connector(s)... (Ctrl+C to stop)",
        connectors.len()
    );

    let cancel = CancellationToken::new();
    let engine = SyncEngine::new(connectors, db, &store_path);
    engine.run(cancel).await
}
