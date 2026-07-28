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
    let mut system = System::new_all();
    refresh_void_daemon_exists(&mut system, Pid::from_u32(pid))
}
