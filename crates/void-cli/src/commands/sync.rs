use std::process::Stdio;
use std::sync::Arc;

use clap::Args;
#[cfg(unix)]
use sysinfo::Signal;
use sysinfo::{Pid, System};
use tokio_util::sync::CancellationToken;
use tracing::info;

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::hooks::{self, HookRunner};
use void_core::sync::SyncEngine;

use crate::commands::connector_factory;
use crate::output::{resolve_connector_filter, resolve_connector_list};

#[derive(Clone, Debug, Args)]
pub struct SyncArgs {
    /// Sync only specific connectors (comma-separated: whatsapp,telegram,slack,gmail,calendar,hackernews)
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
    /// Clear data for a specific connector before syncing (e.g. whatsapp, telegram, slack, gmail, calendar, hackernews)
    #[arg(long)]
    pub clear_connector: Option<String>,
    /// Stop the running sync daemon
    #[arg(long)]
    pub stop: bool,
    /// Show sync daemon status and per-connector sync info
    #[arg(long)]
    pub status: bool,
    /// Internal: run sync process as detached child.
    #[arg(long, hide = true)]
    pub daemon_inner: bool,
}

/// Output sync daemon status and per-connector sync info as JSON to stdout.
pub fn show_status() -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void setup` first.",
            config_path.display()
        )
    })?;

    let store_path = cfg.store_path();
    let lock_path = store_path.join("LOCK");
    let log_path = store_path.join("void-sync.log");

    let mut daemon = serde_json::json!({ "running": false });
    if lock_path.exists() {
        let content = std::fs::read_to_string(&lock_path).unwrap_or_default();
        if let Ok(pid) = parse_lock_pid(&content) {
            let sys_pid = Pid::from_u32(pid);
            let mut system = System::new_all();
            let alive = refresh_process_exists(&mut system, sys_pid);
            daemon = serde_json::json!({ "running": alive, "pid": pid });
        }
    }

    let mut connections = Vec::new();

    let db = Database::open(&cfg.db_path()).ok();

    let state_map = db
        .as_ref()
        .and_then(|db| db.list_sync_states().ok())
        .unwrap_or_default()
        .into_iter()
        .fold(
            std::collections::HashMap::<String, serde_json::Map<String, serde_json::Value>>::new(),
            |mut map, (conn_id, key, value)| {
                map.entry(conn_id)
                    .or_default()
                    .insert(key, serde_json::Value::String(value));
                map
            },
        );

    for connection in &cfg.connections {
        let conn_id = &connection.id;
        let connector_type = connection.connector_type.to_string();

        let last_message_at = db
            .as_ref()
            .and_then(|db| db.latest_message_timestamp(conn_id, &connector_type).ok())
            .flatten();

        let sync_state = state_map
            .get(conn_id)
            .cloned()
            .unwrap_or_default();

        let mut entry = serde_json::json!({
            "id": conn_id,
            "connector": connector_type,
        });
        if let Some(ts) = last_message_at {
            entry["last_message_at"] = serde_json::json!(ts);
        }
        if !sync_state.is_empty() {
            entry["sync_state"] = serde_json::Value::Object(sync_state);
        }
        connections.push(entry);
    }

    let mut output = serde_json::json!({
        "daemon": daemon,
        "connections": connections,
    });

    if log_path.exists() {
        output["log_file"] = serde_json::json!(log_path.to_string_lossy());
        if let Ok(meta) = std::fs::metadata(&log_path) {
            output["log_file_bytes"] = serde_json::json!(meta.len());
        }
    }

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn parse_lock_pid(content: &str) -> anyhow::Result<u32> {
    let pid_str = content
        .trim()
        .strip_prefix("pid=")
        .unwrap_or(content.trim());
    let pid: u32 = pid_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID in lock file: {content}"))?;
    Ok(pid)
}

fn refresh_process_exists(system: &mut System, pid: Pid) -> bool {
    system.refresh_all();
    system.process(pid).is_some()
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
    let pid = parse_lock_pid(&content)?;
    let sys_pid = Pid::from_u32(pid);
    let mut system = System::new_all();
    let mut process_alive = refresh_process_exists(&mut system, sys_pid);

    if !process_alive {
        eprintln!("Daemon (pid {pid}) is no longer running. Cleaning up stale lock file.");
        std::fs::remove_file(&lock_path).ok();
        return Ok(());
    }

    eprintln!("Stopping sync daemon (pid {pid})...");
    #[cfg(unix)]
    {
        info!(pid, "sending SIGTERM to daemon");
        if let Some(process) = system.process(sys_pid) {
            match process.kill_with(Signal::Term) {
                Some(true) => {}
                Some(false) => anyhow::bail!("Failed to send SIGTERM to daemon (pid {pid})"),
                None => {
                    anyhow::bail!(
                        "Failed to send SIGTERM to daemon (pid {pid}): unsupported signal"
                    );
                }
            }
        }
    }
    #[cfg(windows)]
    {
        info!(pid, "sending termination signal to daemon");
        if let Some(process) = system.process(sys_pid) {
            process.kill();
        }
    }

    const MAX_WAIT: std::time::Duration = std::time::Duration::from_secs(10);
    const POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while process_alive {
        if start.elapsed() > MAX_WAIT {
            #[cfg(unix)]
            {
                eprintln!("Daemon did not exit within {MAX_WAIT:?}, sending SIGKILL...");
                if let Some(process) = system.process(sys_pid) {
                    let _ = process.kill_with(Signal::Kill);
                }
            }
            #[cfg(windows)]
            {
                eprintln!("Daemon did not exit within {MAX_WAIT:?}, forcing termination...");
                if let Some(process) = system.process(sys_pid) {
                    process.kill();
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
        process_alive = refresh_process_exists(&mut system, sys_pid);
    }

    if lock_path.exists() {
        std::fs::remove_file(&lock_path).ok();
    }

    eprintln!("Sync daemon stopped.");
    Ok(())
}

/// Spawn a detached child process that runs sync in daemon mode.
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

    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    if verbose {
        cmd.arg("--verbose");
    }
    cmd.arg("sync").arg("--daemon-inner");
    if let Some(ref connectors) = args.connectors {
        cmd.arg("--connectors").arg(connectors);
    }
    if args.clear {
        cmd.arg("--clear");
    }
    if let Some(ref clear_connector) = args.clear_connector {
        cmd.arg("--clear-connector").arg(clear_connector);
    }

    cmd.current_dir(&store_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Detach from controlling terminal in child process.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    let child = cmd.spawn()?;
    eprintln!("Sync daemon started (pid {}).", child.id());
    Ok(())
}

pub fn run_daemon_inner(args: &SyncArgs, verbose: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let connectors = args.connectors.clone();
    let clear = args.clear;
    let clear_connector = args.clear_connector.clone();

    rt.block_on(async move {
        let log_level = if verbose { "debug" } else { "info" };
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
            )
            .with_writer(std::io::stderr)
            .try_init();

        let inner_args = SyncArgs {
            connectors,
            daemon: false,
            restart: false,
            clear,
            clear_connector,
            stop: false,
            status: false,
            daemon_inner: false,
        };
        info!("daemon child started");
        run(&inner_args).await
    })
}

pub async fn run(args: &SyncArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void setup` first.",
            config_path.display()
        )
    })?;

    if cfg.connections.is_empty() {
        anyhow::bail!("No connections configured. Add connections to your config.toml first.");
    }

    let connector_filter = resolve_connector_list(args.connectors.as_deref())?;

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
        let ct = resolve_connector_filter(Some(connector_type))?.ok_or_else(|| {
            anyhow::anyhow!("internal error: connector type missing after successful parse")
        })?;
        let (msgs, convs, evts, sync_st) = db.clear_connector_data(&ct)?;
        eprintln!(
            "Cleared {ct} data: {msgs} messages, {convs} conversations, {evts} events, {sync_st} sync states"
        );
        info!(
            connector = %ct, msgs, convs, evts, sync_st,
            "connector data cleared"
        );

        if ct == "whatsapp" {
            for connection in &cfg.connections {
                if connection.connector_type.to_string() == "whatsapp" {
                    let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
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

        if ct == "telegram" {
            for connection in &cfg.connections {
                if connection.connector_type.to_string() == "telegram" {
                    let session_file = store_path.join(format!("telegram-{}.json", connection.id));
                    if session_file.exists() {
                        std::fs::remove_file(&session_file)?;
                        eprintln!(
                            "Removed Telegram session: {} (will require re-auth)",
                            session_file.display()
                        );
                    }
                }
            }
        }
    }

    let mut connectors: Vec<Arc<dyn void_core::connector::Connector>> = Vec::new();

    for connection in &cfg.connections {
        if let Some(ref filter) = connector_filter {
            let type_str = connection.connector_type.to_string();
            if !filter.iter().any(|f| type_str.contains(f)) {
                continue;
            }
        }

        match connector_factory::build_connector(connection, &store_path) {
            Ok(conn) => match conn.health_check().await {
                Ok(status) if status.ok => connectors.push(conn),
                Ok(status) => {
                    eprintln!(
                        "[warn] Skipping connection '{}' ({}): {}. Run `void setup` to authenticate.",
                        connection.id, connection.connector_type, status.message
                    );
                }
                Err(e) => {
                    eprintln!(
                        "[warn] Skipping connection '{}' ({}): {e}. Run `void setup` to authenticate.",
                        connection.id, connection.connector_type
                    );
                }
            },
            Err(e) => {
                eprintln!(
                    "[warn] Skipping connection '{}' ({}): {e}",
                    connection.id, connection.connector_type
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

    let hooks_dir = hooks::hooks_dir();
    let loaded_hooks = hooks::load_hooks(&hooks_dir);
    let hook_runner = if loaded_hooks.is_empty() {
        None
    } else {
        let enabled = loaded_hooks.iter().filter(|h| h.enabled).count();
        eprintln!("Loaded {enabled} hook(s) from {}", hooks_dir.display());
        Some(Arc::new(
            HookRunner::new(loaded_hooks).with_db(Arc::clone(&db)),
        ))
    };

    let cancel = CancellationToken::new();
    let engine = SyncEngine::new(connectors, db, &store_path, hook_runner);
    engine.run(cancel).await
}
