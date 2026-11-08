//! Hooks in remote store mode.
//!
//! The sync daemon runs on the remote host and loads hooks from the `hooks/`
//! directory next to the *remote* config. Hook files on the client are never
//! read there, so these helpers inspect and populate the remote directory.

use std::path::PathBuf;

use crate::error::ConfigError;
use crate::hooks::Hook;

use super::remote::REMOTE_PATH_PREFIX;
use super::{shell_escape, ResolvedContext};

/// `hooks/` directory next to a remote config file path.
pub(crate) fn remote_hooks_dir(remote_config_path: &str) -> String {
    match remote_config_path.rsplit_once('/') {
        Some(("", _)) => "/hooks".to_string(),
        Some((parent, _)) => format!("{parent}/hooks"),
        None => "hooks".to_string(),
    }
}

impl ResolvedContext {
    /// Absolute path of the hooks directory the remote daemon loads.
    pub fn remote_hooks_dir(&self) -> Result<String, ConfigError> {
        let remote = self.remote.as_ref().ok_or_else(|| {
            ConfigError::Remote("remote hooks require store.mode = \"remote\"".into())
        })?;
        let targets = remote.cached_proxy_targets()?;
        Ok(remote_hooks_dir(&targets.config_path))
    }

    /// Hooks defined on the remote host (`void hook list` run over SSH).
    ///
    /// Read-only, so it works even when `store.remote.proxy_writes = false`.
    pub fn remote_hook_list(&self) -> Result<Vec<Hook>, ConfigError> {
        let remote = self.remote.as_ref().ok_or_else(|| {
            ConfigError::Remote("remote hooks require store.mode = \"remote\"".into())
        })?;
        let targets = remote.cached_proxy_targets()?;
        let store_path = remote.ssh.resolve_path_on_host(&remote.remote_store_path)?;
        let parts = [
            targets.void_bin.as_str(),
            "--config",
            targets.config_path.as_str(),
            "--local-store",
            "--store",
            store_path.as_str(),
            "hook",
            "list",
        ];
        let escaped = parts
            .iter()
            .map(|part| shell_escape(part))
            .collect::<Vec<_>>()
            .join(" ");
        let output = remote
            .ssh
            .run_remote(&format!("{REMOTE_PATH_PREFIX} {escaped}"))?;
        if !output.status.success() {
            return Err(ConfigError::Remote(format!(
                "remote `void hook list` failed ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        parse_hook_list(&output.stdout)
    }

    /// Copy local hook files into the remote hooks directory.
    ///
    /// Returns the remote directory. The remote daemon only loads hooks at
    /// startup, so it must be restarted to pick them up.
    pub fn push_hooks(&self, files: &[PathBuf]) -> Result<String, ConfigError> {
        let remote = self.remote.as_ref().ok_or_else(|| {
            ConfigError::Remote("pushing hooks requires store.mode = \"remote\"".into())
        })?;
        if !remote.proxy_writes {
            return Err(ConfigError::Remote(
                "remote write proxy is disabled (store.remote.proxy_writes = false)".into(),
            ));
        }
        let dir = self.remote_hooks_dir()?;
        let output = remote
            .ssh
            .run_remote(&format!("mkdir -p {}", shell_escape(&dir)))?;
        if !output.status.success() {
            return Err(ConfigError::Remote(format!(
                "failed to create remote hooks directory {dir}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        for file in files {
            let name = file.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
                ConfigError::Remote(format!("invalid hook file name: {}", file.display()))
            })?;
            remote.ssh.scp_to(file, &format!("{dir}/{name}"))?;
        }
        Ok(dir)
    }
}

/// Parse `void hook list` JSON output (`{"data": [...]}`).
pub(crate) fn parse_hook_list(stdout: &[u8]) -> Result<Vec<Hook>, ConfigError> {
    #[derive(serde::Deserialize)]
    struct HookList {
        data: Vec<Hook>,
    }
    serde_json::from_slice::<HookList>(stdout)
        .map(|list| list.data)
        .map_err(|e| ConfigError::Remote(format!("invalid remote `void hook list` output: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_hooks_dir_is_next_to_config() {
        assert_eq!(
            remote_hooks_dir("/home/max/.config/void/config.toml"),
            "/home/max/.config/void/hooks"
        );
        assert_eq!(remote_hooks_dir("/config.toml"), "/hooks");
        assert_eq!(remote_hooks_dir("config.toml"), "hooks");
    }

    #[test]
    fn parses_hook_list_output() {
        let hook = Hook {
            name: "slack-triage".into(),
            enabled: true,
            max_turns: 3,
            agent: "claude".into(),
            extra_args: Vec::new(),
            active_window: None,
            trigger: crate::hooks::Trigger::NewMessage {
                connector: Some("slack".into()),
            },
            prompt: crate::hooks::PromptConfig {
                text: "triage".into(),
            },
        };
        let out = serde_json::to_vec(&serde_json::json!({ "data": [hook] })).unwrap();
        let hooks = parse_hook_list(&out).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "slack-triage");

        assert!(parse_hook_list(br#"{"data": []}"#).unwrap().is_empty());
        assert!(parse_hook_list(b"not json").is_err());
    }
}
