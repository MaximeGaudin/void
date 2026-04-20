#[cfg(unix)]
use sysinfo::Signal;
use sysinfo::{Pid, System};
use tracing::info;
use void_core::config::{self, VoidConfig};

use super::lock::{parse_lock_pid, refresh_process_exists};

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
pub fn daemonize(args: &super::SyncArgs, verbose: bool) -> anyhow::Result<()> {
    use std::process::Stdio;

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

pub fn run_daemon_inner(args: &super::SyncArgs, verbose: bool) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let connectors = args.connectors.clone();
    let clear = args.clear;
    let clear_connector = args.clear_connector.clone();

    rt.block_on(async move {
        let log_level = if verbose { "debug" } else { "info" };
        let filter = format!("{log_level},html5ever=error");
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&filter)),
            )
            .with_writer(std::io::stderr)
            .try_init();

        let inner_args = super::SyncArgs {
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
        super::engine::run(&inner_args).await
    })
}
