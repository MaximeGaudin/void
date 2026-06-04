mod remote;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::{debug, info, warn};

use crate::config::{expand_tilde, resolve_config_path, StoreMode, VoidConfig};
use crate::db::Database;
use crate::error::ConfigError;

pub use remote::{
    cache_is_fresh, default_cache_dir, fetch_remote_file, fetch_remote_files_if_present, now_secs,
    CacheMeta, RemoteProxyTargets, SshTarget, REMOTE_PATH_PREFIX,
};

#[derive(Debug, Clone)]
pub struct ResolvedContext {
    mode: StoreMode,
    config: VoidConfig,
    client_config_path: PathBuf,
    store_override: Option<PathBuf>,
    remote: Option<RemoteHandle>,
}

#[derive(Debug, Clone)]
struct RemoteHandle {
    ssh: SshTarget,
    remote_config_path: String,
    cache_dir: PathBuf,
    meta: CacheMeta,
    proxy_writes: bool,
    /// Effective remote store directory (from remote config or override).
    remote_store_path: String,
    proxy_targets: Arc<Mutex<Option<RemoteProxyTargets>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshPolicy {
    UseCache,
    Force,
    /// Remote proxy commands: do not SCP/refresh cache (SSH proxy only).
    ProxyOnly,
}

impl ResolvedContext {
    pub fn load(
        config_path: Option<&Path>,
        store_override: Option<&str>,
        refresh: RefreshPolicy,
        force_local_store: bool,
    ) -> Result<Self, ConfigError> {
        let client_config_path = resolve_config_path(config_path);
        let store_override = store_override.map(expand_tilde);

        if !client_config_path.exists() {
            return Err(ConfigError::Other(format!(
                "Config file not found: {}",
                client_config_path.display()
            )));
        }

        if force_local_store {
            let mut config = VoidConfig::load(&client_config_path)?;
            if let Some(path) = &store_override {
                config.store.path = path.to_string_lossy().into_owned();
            }
            config.store.mode = StoreMode::Local;
            config.store.remote = None;
            return Ok(Self {
                mode: StoreMode::Local,
                config,
                client_config_path,
                store_override,
                remote: None,
            });
        }

        let client_content = std::fs::read_to_string(&client_config_path)?;
        let client_profile = VoidConfig::parse(&client_content)?;

        match client_profile.store.mode {
            StoreMode::Local => {
                let mut config = VoidConfig::load(&client_config_path)?;
                if let Some(path) = &store_override {
                    config.store.path = path.to_string_lossy().into_owned();
                }
                Ok(Self {
                    mode: StoreMode::Local,
                    config,
                    client_config_path,
                    store_override,
                    remote: None,
                })
            }
            StoreMode::Remote => {
                let remote_settings = client_profile.remote()?;
                let ssh = build_ssh_target(remote_settings);
                let cache_dir = remote_settings
                    .cache
                    .path
                    .as_ref()
                    .map(|p| expand_tilde(p))
                    .unwrap_or_else(|| default_cache_dir(&remote_settings.host));

                let proxy_only = refresh == RefreshPolicy::ProxyOnly;
                let force = refresh == RefreshPolicy::Force;
                let meta = CacheMeta::load(&cache_dir).unwrap_or(CacheMeta {
                    config_fetched_at: 0,
                    database_fetched_at: 0,
                });

                let cached_config_path = cache_dir.join("config.toml");
                let mut meta = meta;
                if !proxy_only
                    && (force
                        || !cache_is_fresh(
                            meta.config_fetched_at,
                            remote_settings.cache.config_ttl_secs,
                        ))
                {
                    match refresh_remote_config(
                        &ssh,
                        &remote_settings.remote_config_path,
                        &cache_dir,
                    ) {
                        Ok(()) => {
                            meta.config_fetched_at = now_secs();
                            meta.save(&cache_dir)?;
                            info!(cache = %cache_dir.display(), "refreshed remote config cache");
                        }
                        Err(e) if cached_config_path.exists() => {
                            warn!(
                                cache = %cache_dir.display(),
                                error = %e,
                                "remote config refresh failed; using stale cache"
                            );
                        }
                        Err(e) => return Err(e),
                    }
                }

                let mut config = if cached_config_path.exists() {
                    let remote_content = std::fs::read_to_string(&cached_config_path)?;
                    VoidConfig::parse(&remote_content)?
                } else if proxy_only {
                    VoidConfig::default()
                } else {
                    return Err(ConfigError::Other(format!(
                        "remote config cache missing at {} — run `void remote refresh` or check SSH",
                        cached_config_path.display()
                    )));
                };

                let remote_store_path = store_override
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .or_else(|| remote_settings.remote_store_path.clone())
                    .or_else(|| {
                        if config.store.path.is_empty() {
                            None
                        } else {
                            Some(config.store.path.clone())
                        }
                    })
                    .unwrap_or_else(|| "~/.local/share/void".to_string());
                config.store.path = remote_store_path.clone();
                config.store.mode = StoreMode::Local;
                config.store.remote = None;

                if !proxy_only
                    && (force
                        || !cache_is_fresh(
                            meta.database_fetched_at,
                            remote_settings.cache.database_ttl_secs,
                        ))
                {
                    match refresh_remote_database(&ssh, &remote_store_path, &cache_dir) {
                        Ok(()) => {
                            meta.database_fetched_at = now_secs();
                            meta.save(&cache_dir)?;
                            debug!(
                                cache = %cache_dir.display(),
                                "refreshed remote database snapshot"
                            );
                        }
                        Err(e) if cache_dir.join("void.db").exists() => {
                            warn!(
                                cache = %cache_dir.display(),
                                error = %e,
                                "remote database refresh failed; using stale snapshot"
                            );
                        }
                        Err(e) => return Err(e),
                    }
                }

                let remote = RemoteHandle {
                    ssh,
                    remote_config_path: remote_settings.remote_config_path.clone(),
                    cache_dir,
                    meta,
                    proxy_writes: remote_settings.proxy_writes,
                    remote_store_path,
                    proxy_targets: Arc::new(Mutex::new(None)),
                };

                Ok(Self {
                    mode: StoreMode::Remote,
                    config,
                    client_config_path,
                    store_override,
                    remote: Some(remote),
                })
            }
        }
    }

    pub fn mode(&self) -> StoreMode {
        self.mode
    }

    pub fn is_remote(&self) -> bool {
        self.mode == StoreMode::Remote
    }

    pub fn config(&self) -> &VoidConfig {
        &self.config
    }

    pub fn client_config_path(&self) -> &Path {
        &self.client_config_path
    }

    pub fn store_path(&self) -> PathBuf {
        if self.is_remote() {
            self.remote
                .as_ref()
                .map(|r| r.cache_dir.clone())
                .unwrap_or_else(|| self.config.store_path())
        } else {
            self.config.store_path()
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.store_path().join("void.db")
    }

    pub fn open_database(&self) -> Result<Database, crate::error::DbError> {
        if self.is_remote() {
            Database::open_readonly(&self.db_path())
        } else {
            Database::open(&self.db_path())
        }
    }

    pub fn open_database_writable(&self) -> Result<Database, crate::error::DbError> {
        if self.is_remote() {
            Err(crate::error::DbError::Other(
                "cannot open remote database for writing locally; writes are proxied to the remote host"
                    .into(),
            ))
        } else {
            Database::open(&self.db_path())
        }
    }

    pub fn refresh_cache(&mut self) -> Result<(), ConfigError> {
        let store_override = self
            .store_override
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned());
        *self = Self::load(
            Some(&self.client_config_path),
            store_override.as_deref(),
            RefreshPolicy::Force,
            false,
        )?;
        Ok(())
    }

    pub fn remote_status(&self) -> Result<serde_json::Value, ConfigError> {
        let remote = self.remote.as_ref().ok_or_else(|| {
            ConfigError::Remote("remote status requires store.mode = \"remote\"".into())
        })?;

        let ssh_check = remote
            .ssh
            .run_remote("echo void-remote-ok")
            .map(|output| output.status.success())
            .unwrap_or(false);

        let daemon_running = remote_daemon_running(&remote.ssh, &remote.remote_store_path);

        Ok(serde_json::json!({
            "mode": "remote",
            "host": remote.ssh.host,
            "user": remote.ssh.user,
            "remote_config_path": remote.remote_config_path,
            "remote_store_path": remote.remote_store_path,
            "cache_dir": remote.cache_dir,
            "config_age_secs": now_secs().saturating_sub(remote.meta.config_fetched_at),
            "database_age_secs": now_secs().saturating_sub(remote.meta.database_fetched_at),
            "ssh_reachable": ssh_check,
            "remote_daemon_running": daemon_running,
            "proxy_writes": remote.proxy_writes,
        }))
    }

    pub fn proxy_command(&self, args: &[String]) -> Result<i32, ConfigError> {
        let remote = self.remote.as_ref().ok_or_else(|| {
            ConfigError::Remote("cannot proxy commands in local store mode".into())
        })?;
        if !remote.proxy_writes {
            return Err(ConfigError::Remote(
                "remote write proxy is disabled (store.remote.proxy_writes = false)".into(),
            ));
        }

        let targets = {
            let mut cache = remote
                .proxy_targets
                .lock()
                .map_err(|e| ConfigError::Remote(format!("proxy cache lock poisoned: {e}")))?;
            if let Some(targets) = cache.as_ref() {
                targets.clone()
            } else {
                let targets = remote
                    .ssh
                    .resolve_proxy_targets(&remote.remote_config_path)?;
                *cache = Some(targets.clone());
                targets
            }
        };

        let store_path = remote.ssh.resolve_path_on_host(&remote.remote_store_path)?;

        let mut parts = vec![targets.void_bin.clone(), "--config".to_string()];
        parts.push(targets.config_path.clone());
        parts.push("--local-store".to_string());
        parts.push("--store".to_string());
        parts.push(store_path);
        parts.extend(args.iter().cloned());

        let escaped = parts
            .iter()
            .map(|part| shell_escape(part))
            .collect::<Vec<_>>()
            .join(" ");
        let remote_command = format!("{REMOTE_PATH_PREFIX} {escaped}");
        let output = remote.ssh.run_remote(&remote_command)?;
        if !output.stdout.is_empty() {
            print!("{}", String::from_utf8_lossy(&output.stdout));
        }
        if !output.stderr.is_empty() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }

        Ok(output.status.code().unwrap_or(1))
    }

    pub fn ensure_local_sync_allowed(&self) -> Result<(), ConfigError> {
        if self.is_remote() {
            Err(ConfigError::Remote(
                "sync runs on the remote host when store.mode = \"remote\". \
                 Use `ssh <host> void sync --daemon` on the server, or `void remote status`."
                    .into(),
            ))
        } else {
            Ok(())
        }
    }

    pub fn ensure_local_setup_allowed(&self) -> Result<(), ConfigError> {
        if self.is_remote() {
            Err(ConfigError::Remote(
                "setup must run on the remote host when store.mode = \"remote\" \
                 (connections and credentials live in the remote config)."
                    .into(),
            ))
        } else {
            Ok(())
        }
    }
}

fn build_ssh_target(remote: &crate::config::RemoteStoreConfig) -> SshTarget {
    SshTarget {
        host: remote.host.clone(),
        user: remote.user.clone(),
        port: remote.ssh.port,
        identity_file: remote.ssh.identity_file.as_ref().map(|p| expand_tilde(p)),
    }
}

fn refresh_remote_config(
    ssh: &SshTarget,
    remote_config_path: &str,
    cache_dir: &Path,
) -> Result<(), ConfigError> {
    std::fs::create_dir_all(cache_dir)?;
    let local_path = cache_dir.join("config.toml");
    fetch_remote_file(ssh, remote_config_path, &local_path)?;
    Ok(())
}

fn refresh_remote_database(
    ssh: &SshTarget,
    remote_store_path: &str,
    cache_dir: &Path,
) -> Result<(), ConfigError> {
    fetch_remote_files_if_present(
        ssh,
        remote_store_path,
        &["void.db", "void.db-wal", "void.db-shm"],
        cache_dir,
    )
}

fn remote_daemon_running(ssh: &SshTarget, remote_store_path: &str) -> bool {
    let lock_path = format!("{remote_store_path}/LOCK");
    let cmd = if lock_path.starts_with('~') {
        format!("test -f {lock_path} && echo running || echo stopped")
    } else {
        format!("test -f '{lock_path}' && echo running || echo stopped")
    };
    ssh.run_remote(&cmd)
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|s| s.trim() == "running")
        .unwrap_or(false)
}

fn shell_escape(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@'))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_safe_strings() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn shell_escape_absolute_paths() {
        let path = "/Users/me/.config/void/config.toml";
        assert_eq!(shell_escape(path), path);
        let bin = "/Users/me/bin/void";
        assert_eq!(shell_escape(bin), bin);
    }
}
