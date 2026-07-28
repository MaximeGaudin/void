use sysinfo::{Pid, System};

pub(super) fn parse_lock_pid(content: &str) -> anyhow::Result<u32> {
    let pid_str = content
        .trim()
        .strip_prefix("pid=")
        .unwrap_or(content.trim());
    let pid: u32 = pid_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID in lock file: {content}"))?;
    Ok(pid)
}

/// Whether the lock's PID still belongs to a running void daemon.
///
/// Delegates to void-core so the "is this really our process?" rule lives in
/// one place: a recycled PID must never be mistaken for a live daemon, or
/// `void sync --stop` would signal an unrelated process.
pub(super) fn refresh_process_exists(system: &mut System, pid: Pid) -> bool {
    void_core::sync::refresh_void_daemon_exists(system, pid)
}
