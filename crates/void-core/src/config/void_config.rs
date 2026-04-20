use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::models::ConnectorType;

use super::connection::ConnectionConfig;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreConfig {
    #[serde(default = "default_store_path")]
    pub path: String,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            path: default_store_path(),
        }
    }
}

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
    #[serde(default = "default_gmail_poll")]
    pub gmail_poll_interval_secs: u64,
    #[serde(default = "default_calendar_poll")]
    pub calendar_poll_interval_secs: u64,
    #[serde(default = "default_hackernews_poll")]
    pub hackernews_poll_interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            gmail_poll_interval_secs: default_gmail_poll(),
            calendar_poll_interval_secs: default_calendar_poll(),
            hackernews_poll_interval_secs: default_hackernews_poll(),
        }
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

impl VoidConfig {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        if content.contains("[[accounts]]") {
            let migrated = content.replace("[[accounts]]", "[[connections]]");
            std::fs::write(path, &migrated)?;
            let config: Self = toml::from_str(&migrated)?;
            return Ok(config);
        }
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn store_path(&self) -> PathBuf {
        expand_tilde(&self.store.path)
    }

    pub fn db_path(&self) -> PathBuf {
        self.store_path().join("void.db")
    }

    pub fn find_connection(&self, connection_id: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|a| a.id == connection_id)
    }

    /// Find a config connection by connector type string (e.g. "slack", "gmail", "whatsapp", "telegram").
    pub fn find_connection_by_connector(&self, connector: &str) -> Option<&ConnectionConfig> {
        let target = match connector {
            "whatsapp" => ConnectorType::WhatsApp,
            "slack" => ConnectorType::Slack,
            "gmail" => ConnectorType::Gmail,
            "calendar" => ConnectorType::Calendar,
            "telegram" => ConnectorType::Telegram,
            "hackernews" => ConnectorType::HackerNews,
            _ => return None,
        };
        self.connections.iter().find(|a| a.connector_type == target)
    }
}
