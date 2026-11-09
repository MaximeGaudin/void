use std::path::Path;

use sysinfo::{Pid, System};

use super::lock::refresh_void_daemon_exists;

/// Returns true when a sync daemon lock file exists and its PID is a live
/// void process.
pub fn is_daemon_running(store_path: &Path) -> bool {
    let lock_path = store_path.join("LOCK");
    if !lock_path.exists() {
        return false;
    }
    let content = match std::fs::read_to_string(&lock_path) {
        Ok(content) => content,
        Err(_) => return false,
    };
    let pid_str = match content.trim().strip_prefix("pid=") {
        Some(pid) => pid,
        None => return false,
    };
    let pid: u32 = match pid_str.parse() {
        Ok(pid) => pid,
        Err(_) => return false,
    };
    // Same answer as before sysinfo 0.40 when the process table can't be read:
    // no process found.
    match System::new_all() {
        Ok(mut system) => refresh_void_daemon_exists(&mut system, Pid::from_u32(pid)),
        Err(e) => {
            tracing::warn!(error = %e, "cannot read the process table; assuming no daemon");
            false
        }
    }
}
