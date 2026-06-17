use std::path::Path;

use sysinfo::{Pid, System};
use tracing::info;

/// Simple file-based lock to prevent multiple sync instances.
pub(crate) struct FileLock {
    path: std::path::PathBuf,
}

impl FileLock {
    pub(crate) fn acquire(path: &Path) -> anyhow::Result<Self> {
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

    /// Check if the PID in the lock file is still alive.
    /// Returns `Some(true)` if stale, `Some(false)` if alive, `None` if unparseable.
    fn is_stale_lock(content: &str) -> Option<bool> {
        let pid_str = content.trim().strip_prefix("pid=")?;
        let pid: u32 = pid_str.parse().ok()?;
        let mut system = System::new_all();
        system.refresh_all();
        let alive = system.process(Pid::from_u32(pid)).is_some();
        Some(!alive)
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}
