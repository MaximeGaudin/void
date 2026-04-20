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

pub(super) fn refresh_process_exists(system: &mut System, pid: Pid) -> bool {
    system.refresh_all();
    system.process(pid).is_some()
}
