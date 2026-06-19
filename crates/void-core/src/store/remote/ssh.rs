use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::error::ConfigError;

use super::REMOTE_PATH_PREFIX;

#[derive(Debug, Clone)]
pub struct RemoteProxyTargets {
    pub config_path: String,
    pub void_bin: String,
}

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub host: String,
    pub user: Option<String>,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
}

impl SshTarget {
    pub fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    fn base_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-p".into(),
            self.port.to_string(),
        ];
        if let Some(identity) = &self.identity_file {
            args.push("-i".into());
            args.push(identity.to_string_lossy().into_owned());
        }
        args
    }

    /// Resolve `~/…` using the remote host's `$HOME` (for SSH-proxied CLI commands).
    pub fn resolve_path_on_host(&self, path: &str) -> Result<String, ConfigError> {
        if let Some(rest) = path.strip_prefix("~/") {
            let home = self.remote_home_dir()?;
            Ok(home.join(rest).to_string_lossy().into_owned())
        } else if path == "~" {
            Ok(self.remote_home_dir()?.to_string_lossy().into_owned())
        } else {
            Ok(path.to_string())
        }
    }

    /// Resolve absolute config path and `void` binary on the remote host (one SSH round-trip).
    pub fn resolve_proxy_targets(
        &self,
        config_path: &str,
    ) -> Result<RemoteProxyTargets, ConfigError> {
        let output = self.run_remote(&format!(
            "{REMOTE_PATH_PREFIX}; \
             home=$(printf %s \"$HOME\"); \
             bin=$(command -v void); \
             printf '%s\n%s\n' \"$home\" \"$bin\""
        ))?;
        if !output.status.success() {
            return Err(ConfigError::Remote(
                "failed to resolve remote $HOME and void binary".into(),
            ));
        }
        let stdout = String::from_utf8(output.stdout)
            .map_err(|e| ConfigError::Remote(format!("invalid proxy resolve output: {e}")))?;
        let mut lines = stdout.lines();
        let home = lines
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ConfigError::Remote("remote $HOME is empty".into()))?;
        let void_bin = lines
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ConfigError::Remote(
                    "void not found on remote host (install to ~/bin/void or ~/.local/bin/void)"
                        .into(),
                )
            })?;

        let resolved_config = if let Some(rest) = config_path.strip_prefix("~/") {
            format!("{home}/{rest}")
        } else if config_path == "~" {
            home.to_string()
        } else {
            config_path.to_string()
        };

        Ok(RemoteProxyTargets {
            config_path: resolved_config,
            void_bin: void_bin.to_string(),
        })
    }

    pub fn resolve_void_bin(&self) -> Result<String, ConfigError> {
        let output = self.run_remote(&format!("{REMOTE_PATH_PREFIX}; command -v void"))?;
        if !output.status.success() {
            return Err(ConfigError::Remote(
                "void not found on remote host (install to ~/bin/void or ~/.local/bin/void)".into(),
            ));
        }
        let bin = String::from_utf8(output.stdout)
            .map_err(|e| ConfigError::Remote(format!("invalid remote void path: {e}")))?
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if bin.is_empty() {
            return Err(ConfigError::Remote(
                "void not found on remote host (install to ~/bin/void or ~/.local/bin/void)".into(),
            ));
        }
        Ok(bin)
    }

    fn remote_home_dir(&self) -> Result<PathBuf, ConfigError> {
        let output = self.run_remote("printf %s \"$HOME\"")?;
        if !output.status.success() {
            return Err(ConfigError::Remote(
                "failed to resolve remote $HOME for config path".into(),
            ));
        }
        let home = String::from_utf8(output.stdout)
            .map_err(|e| ConfigError::Remote(format!("invalid remote $HOME: {e}")))?
            .trim()
            .to_string();
        if home.is_empty() {
            return Err(ConfigError::Remote("remote $HOME is empty".into()));
        }
        Ok(PathBuf::from(home))
    }

    pub fn run_remote(&self, remote_command: &str) -> Result<Output, ConfigError> {
        let mut cmd = Command::new("ssh");
        cmd.args(self.base_ssh_args());
        cmd.arg(self.destination());
        cmd.arg(remote_command);
        cmd.output()
            .map_err(|e| ConfigError::Remote(format!("ssh failed: {e}")))
    }

    pub fn scp_to(&self, local_path: &Path, remote_path: &str) -> Result<(), ConfigError> {
        // scp does not expand `~` on the remote destination; resolve to an absolute path.
        let resolved_remote = if remote_path.starts_with('~') {
            self.resolve_path_on_host(remote_path)?
        } else {
            remote_path.to_string()
        };
        let remote_spec = format!(
            "{}:{}",
            self.destination(),
            format_remote_scp_path(&resolved_remote)
        );
        let mut cmd = Command::new("scp");
        cmd.args(self.base_scp_args());
        cmd.arg(local_path);
        cmd.arg(&remote_spec);
        let output = cmd
            .output()
            .map_err(|e| ConfigError::Remote(format!("scp failed: {e}")))?;
        if output.status.success() {
            return Ok(());
        }
        Err(ConfigError::Remote(format!(
            "scp failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )))
    }

    pub fn scp_from(&self, remote_path: &str, local_path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = local_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let remote_spec = format!(
            "{}:{}",
            self.destination(),
            format_remote_scp_path(remote_path)
        );
        let mut cmd = Command::new("scp");
        cmd.args(self.base_scp_args());
        cmd.arg(&remote_spec);
        cmd.arg(local_path);
        let output = cmd
            .output()
            .map_err(|e| ConfigError::Remote(format!("scp failed: {e}")))?;
        if output.status.success() {
            return Ok(());
        }
        let scp_err = format!(
            "scp failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        self.fetch_via_ssh_cat(remote_path, local_path)
            .map_err(|ssh_err| ConfigError::Remote(format!("{scp_err}; ssh fallback: {ssh_err}")))
    }

    /// Fallback when `scp` auth fails but `ssh` works (same keys, different subsystem).
    fn fetch_via_ssh_cat(&self, remote_path: &str, local_path: &Path) -> Result<(), ConfigError> {
        let path = self.resolve_path_on_host(remote_path)?;
        let escaped = shell_escape_path(&path);
        let output = self.run_remote(&format!("cat {escaped}"))?;
        if !output.status.success() {
            return Err(ConfigError::Remote(format!(
                "ssh cat failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        std::fs::write(local_path, &output.stdout)
            .map_err(|e| ConfigError::Remote(format!("write cache file: {e}")))?;
        Ok(())
    }

    fn base_scp_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "StrictHostKeyChecking=accept-new".into(),
            "-P".into(),
            self.port.to_string(),
        ];
        if let Some(identity) = &self.identity_file {
            args.push("-i".into());
            args.push(identity.to_string_lossy().into_owned());
        }
        args
    }
}

pub(crate) fn shell_escape_path(path: &str) -> String {
    if path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@'))
    {
        path.to_string()
    } else {
        format!("'{}'", path.replace('\'', "'\\''"))
    }
}

/// Format a remote path for scp. The remote login shell parses this string, so
/// every metacharacter must be quoted to prevent command injection. A leading
/// `~/` is kept unquoted so `$HOME` still expands; only the remainder (which is
/// where untrusted/config-sourced data lives) is escaped.
pub(crate) fn format_remote_scp_path(remote_path: &str) -> String {
    if let Some(rest) = remote_path.strip_prefix("~/") {
        format!("~/{}", shell_escape_path(rest))
    } else if remote_path == "~" {
        "~".to_string()
    } else {
        shell_escape_path(remote_path)
    }
}
