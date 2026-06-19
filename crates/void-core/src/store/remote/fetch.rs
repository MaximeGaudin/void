use std::path::Path;

use crate::error::ConfigError;

use super::ssh::SshTarget;

pub fn fetch_remote_file(
    ssh: &SshTarget,
    remote_path: &str,
    local_path: &Path,
) -> Result<(), ConfigError> {
    ssh.scp_from(remote_path, local_path)
}

pub fn fetch_remote_files_if_present(
    ssh: &SshTarget,
    remote_dir: &str,
    filenames: &[&str],
    cache_dir: &Path,
) -> Result<(), ConfigError> {
    std::fs::create_dir_all(cache_dir)?;
    for name in filenames {
        let remote_path = format!("{remote_dir}/{name}");
        let local_path = cache_dir.join(name);
        if *name == "void.db" {
            fetch_remote_file(ssh, &remote_path, &local_path)?;
        } else if ssh.scp_from(&remote_path, &local_path).is_err() {
            // WAL sidecars may not exist yet on a quiet database.
            let _ = std::fs::remove_file(&local_path);
        }
    }
    Ok(())
}
