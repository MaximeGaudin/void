use std::path::Path;

use sysinfo::{Pid, System};
use tracing::info;

/// Simple file-based lock to prevent multiple sync instances.
pub struct FileLock {
    path: std::path::PathBuf,
}

impl FileLock {
    pub fn acquire(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path).unwrap_or_default();
            if let Some(stale) = Self::is_stale_lock(&content) {
                if stale {
                    info!(
                        lock_file = %path.display(),
                        content = content.trim(),
                        "removing stale lock file (process no longer running)"
                    );
                    std::fs::remove_file(path).ok();
                } else {
                    anyhow::bail!(
                        "another sync instance is running (lock file: {}, content: {}). \
                         Stop it with `void sync --stop` first.",
                        path.display(),
                        content.trim()
                    );
                }
            }
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

    /// Check if the PID in the lock file still belongs to a running daemon.
    /// Returns `Some(true)` if stale, `Some(false)` if alive, `None` if unparseable.
    fn is_stale_lock(content: &str) -> Option<bool> {
        let pid_str = content.trim().strip_prefix("pid=")?;
        let pid: u32 = pid_str.parse().ok()?;
        let mut system = System::new_all();
        Some(!refresh_void_daemon_exists(&mut system, Pid::from_u32(pid)))
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// Refresh `system` and report whether `pid` is a live void process.
///
/// PID liveness alone is not enough: PIDs are recycled, so a lock file left
/// behind by a daemon that died weeks ago routinely points at an unrelated
/// process. Treating that as "a sync instance is running" makes `--restart`
/// refuse to start and, worse, makes `void sync --stop` signal a stranger.
pub fn refresh_void_daemon_exists(system: &mut System, pid: Pid) -> bool {
    system.refresh_all();
    system
        .process(pid)
        .is_some_and(|p| is_void_process_name(&p.name().to_string_lossy()))
}

/// Whether a process name belongs to the void binary.
///
/// Matches `void`, `void.exe`, `void-cli` and the `void_*-<hash>` test
/// harnesses, but not unrelated names that merely start with "void".
pub fn is_void_process_name(name: &str) -> bool {
    name.strip_prefix("void")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(['-', '_', '.']))
}
