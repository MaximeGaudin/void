use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

use std::collections::HashMap;

use super::connection::{settings_set_opt_string, settings_str, ConnectionConfig};
use super::paths::expand_tilde;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoidConfig {
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub connections: Vec<ConnectionConfig>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StoreMode {
    #[default]
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    #[serde(default = "default_store_path")]
    pub path: String,
    #[serde(default)]
    pub mode: StoreMode,
    #[serde(default)]
    pub remote: Option<RemoteStoreConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteStoreConfig {
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default = "default_remote_config_path")]
    pub remote_config_path: String,
    /// Override store path on the remote host. When unset, uses `[store].path` from the remote config.
    #[serde(default)]
    pub remote_store_path: Option<String>,
    #[serde(default = "default_true")]
    pub proxy_writes: bool,
    #[serde(default)]
    pub ssh: RemoteSshConfig,
    #[serde(default)]
    pub cache: RemoteCacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSshConfig {
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub identity_file: Option<String>,
}

impl Default for RemoteSshConfig {
    fn default() -> Self {
        Self {
            port: default_ssh_port(),
            identity_file: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteCacheConfig {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_config_ttl")]
    pub config_ttl_secs: u64,
    #[serde(default = "default_database_ttl")]
    pub database_ttl_secs: u64,
}

impl Default for RemoteCacheConfig {
    fn default() -> Self {
        Self {
            path: None,
            config_ttl_secs: default_config_ttl(),
            database_ttl_secs: default_database_ttl(),
        }
    }
}

fn default_remote_config_path() -> String {
    "~/.config/void/config.toml".to_string()
}

fn default_ssh_port() -> u16 {
    22
}

fn default_config_ttl() -> u64 {
    300
}

fn default_database_ttl() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: default_store_path(),
            mode: StoreMode::Local,
            remote: None,
        }
    }
}

// Remove duplicate Default impl - there was one before for StoreConfig

fn default_store_path() -> String {
    #[cfg(windows)]
    {
        super::paths::preferred_store_dir()
            .to_string_lossy()
            .to_string()
    }
    #[cfg(not(windows))]
    {
        "~/.local/share/void".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(flatten)]
    values: HashMap<String, toml::Value>,
}

impl Default for SyncConfig {
    fn default() -> Self {
        let mut values = HashMap::new();
        values.insert(
            "gmail_poll_interval_secs".into(),
            toml::Value::Integer(default_gmail_poll() as i64),
        );
        values.insert(
            "calendar_poll_interval_secs".into(),
            toml::Value::Integer(default_calendar_poll() as i64),
        );
        values.insert(
            "hackernews_poll_interval_secs".into(),
            toml::Value::Integer(default_hackernews_poll() as i64),
        );
        values.insert(
            "googlenews_poll_interval_secs".into(),
            toml::Value::Integer(default_googlenews_poll() as i64),
        );
        values.insert(
            "linkedin_poll_interval_secs".into(),
            toml::Value::Integer(default_linkedin_poll() as i64),
        );
        values.insert(
            "linkedin_backfill_days".into(),
            toml::Value::Integer(default_linkedin_backfill_days() as i64),
        );
        values.insert(
            "github_poll_interval_secs".into(),
            toml::Value::Integer(default_github_poll() as i64),
        );
        values.insert(
            "reddit_poll_interval_secs".into(),
            toml::Value::Integer(default_reddit_poll() as i64),
        );
        Self { values }
    }
}

impl SyncConfig {
    pub fn poll_interval_secs(&self, connector_id: &str, default: u64) -> u64 {
        let key = format!("{connector_id}_poll_interval_secs");
        self.values
            .get(&key)
            .and_then(|v| v.as_integer())
            .and_then(|i| u64::try_from(i).ok())
            .unwrap_or(default)
    }

    pub fn linkedin_backfill_days(&self) -> u64 {
        self.values
            .get("linkedin_backfill_days")
            .and_then(|v| v.as_integer())
            .and_then(|i| u64::try_from(i).ok())
            .unwrap_or(default_linkedin_backfill_days())
    }

    pub fn gmail_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("gmail", default_gmail_poll())
    }

    pub fn calendar_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("calendar", default_calendar_poll())
    }

    pub fn hackernews_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("hackernews", default_hackernews_poll())
    }

    pub fn googlenews_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("googlenews", default_googlenews_poll())
    }

    pub fn linkedin_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("linkedin", default_linkedin_poll())
    }

    pub fn github_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("github", default_github_poll())
    }

    pub fn reddit_poll_interval_secs(&self) -> u64 {
        self.poll_interval_secs("reddit", default_reddit_poll())
    }

    pub fn iter_values(&self) -> impl Iterator<Item = (&String, &toml::Value)> {
        self.values.iter()
    }
}

fn default_gmail_poll() -> u64 {
    30
}

fn default_calendar_poll() -> u64 {
    60
}

fn default_hackernews_poll() -> u64 {
    3600
}

fn default_googlenews_poll() -> u64 {
    3600
}

fn default_linkedin_poll() -> u64 {
    30 * 60
}

fn default_linkedin_backfill_days() -> u64 {
    15
}

fn default_github_poll() -> u64 {
    120
}

fn default_reddit_poll() -> u64 {
    3600
}

impl VoidConfig {
    /// Parse config from a string without writing migrations or sidecar changes.
    pub fn parse(content: &str) -> Result<Self, ConfigError> {
        let normalized = if content.contains("[[accounts]]") {
            content.replace("[[accounts]]", "[[connections]]")
        } else {
            content.to_string()
        };
        Ok(toml::from_str(&normalized)?)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        if content.contains("[[accounts]]") {
            let migrated = content.replace("[[accounts]]", "[[connections]]");
            std::fs::write(path, &migrated)?;
            let mut config: Self = toml::from_str(&migrated)?;
            if config.migrate_slack_sidecar_tokens() {
                config.save(path)?;
            }
            return Ok(config);
        }
        let mut config: Self = toml::from_str(&content)?;
        if config.migrate_slack_sidecar_tokens() {
            config.save(path)?;
        }
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = toml::to_string_pretty(self)?;
        // Holds plaintext Slack/LinkedIn/Telegram secrets — keep it owner-only.
        super::write_secure(path, content)?;
        Ok(())
    }

    pub fn store_path(&self) -> PathBuf {
        expand_tilde(&self.store.path)
    }

    pub fn db_path(&self) -> PathBuf {
        self.store_path().join("void.db")
    }

    pub fn is_remote(&self) -> bool {
        self.store.mode == StoreMode::Remote
    }

    /// True when this file is a Mac-style remote client profile, not a full server config.
    pub fn is_remote_client_profile(&self) -> bool {
        self.store.mode == StoreMode::Remote
            && self.connections.is_empty()
            && self.store.remote.is_some()
    }

    pub fn remote(&self) -> Result<&RemoteStoreConfig, ConfigError> {
        match (&self.store.mode, &self.store.remote) {
            (StoreMode::Remote, Some(remote)) => Ok(remote),
            (StoreMode::Remote, None) => Err(ConfigError::Remote(
                "store.mode is \"remote\" but [store.remote] is missing".into(),
            )),
            _ => Err(ConfigError::Remote("store is not in remote mode".into())),
        }
    }

    pub fn find_connection(&self, connection_id: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|a| a.id == connection_id)
    }

    /// Find a config connection by connector type string (e.g. "slack", "gmail", "whatsapp", "telegram").
    pub fn find_connection_by_connector(&self, connector: &str) -> Option<&ConnectionConfig> {
        self.connections
            .iter()
            .find(|a| a.connector_type.as_str() == connector)
    }

    /// Import Slack config refresh tokens from legacy sidecar files in the store.
    /// Returns `true` if any connection was updated.
    fn migrate_slack_sidecar_tokens(&mut self) -> bool {
        let store_path = self.store_path();
        let mut migrated = false;
        for conn in &mut self.connections {
            if conn.connector_type.as_str() != "slack" {
                continue;
            }
            if settings_str(&conn.settings, "config_refresh_token").is_some() {
                continue;
            }
            let sidecar = store_path.join(format!("slack-config-token-{}.json", conn.id));
            if let Ok(content) = std::fs::read_to_string(&sidecar) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                    if let Some(token) = value.get("refresh_token").and_then(|v| v.as_str()) {
                        settings_set_opt_string(
                            &mut conn.settings,
                            "config_refresh_token",
                            Some(token.to_string()),
                        );
                        migrated = true;
                    }
                }
                let _ = std::fs::remove_file(&sidecar);
            }
        }
        migrated
    }

    pub fn set_slack_config_refresh_token(
        &mut self,
        connection_id: &str,
        token: Option<String>,
    ) -> bool {
        let Some(conn) = self.connections.iter_mut().find(|c| c.id == connection_id) else {
            return false;
        };
        if conn.connector_type.as_str() != "slack" {
            return false;
        }
        if token.is_none() {
            conn.settings.remove("config_refresh_token");
        } else {
            settings_set_opt_string(&mut conn.settings, "config_refresh_token", token);
        }
        true
    }

    pub fn add_ignore_conversation(&mut self, connection_id: &str, entry: String) -> bool {
        let Some(conn) = self.connections.iter_mut().find(|c| c.id == connection_id) else {
            return false;
        };
        if conn
            .ignore_conversations
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&entry))
        {
            return false;
        }
        conn.ignore_conversations.push(entry);
        true
    }

    pub fn remove_ignore_conversation(
        &mut self,
        connection_id: &str,
        external_id: &str,
        name: Option<&str>,
    ) -> bool {
        let Some(conn) = self.connections.iter_mut().find(|c| c.id == connection_id) else {
            return false;
        };
        let before = conn.ignore_conversations.len();
        conn.ignore_conversations.retain(|entry| {
            let entry_lower = entry.to_lowercase();
            if entry_lower == external_id.to_lowercase() {
                return false;
            }
            if let Some(name) = name {
                if entry_lower == name.to_lowercase() {
                    return false;
                }
            }
            true
        });
        conn.ignore_conversations.len() < before
    }
}
