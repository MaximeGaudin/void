use std::path::{Path, PathBuf};

use serde::de::Deserializer;

use crate::error::ConfigError;
use crate::models::ConnectorType;
use serde::{Deserialize, Serialize};

const LEGACY_CONFIG_DIR: &str = ".config/void";
const LEGACY_STORE_DIR: &str = ".local/share/void";
const CONFIG_FILENAME: &str = "config.toml";

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
        preferred_store_dir().to_string_lossy().to_string()
    }
    #[cfg(not(windows))]
    {
        format!("~/{LEGACY_STORE_DIR}")
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

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_conversations: Vec<String>,
    #[serde(flatten)]
    pub settings: ConnectionSettings,
}

/// Custom deserializer that uses the `type` field to drive which
/// `ConnectionSettings` variant to parse, avoiding the ambiguity of
/// `#[serde(untagged)]` (Gmail and Calendar share `credentials_file`).
impl<'de> Deserialize<'de> for ConnectionConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: RawConnectionConfig = RawConnectionConfig::deserialize(deserializer)?;
        let settings = match raw.connector_type {
            ConnectorType::Slack => ConnectionSettings::Slack {
                app_token: raw
                    .app_token
                    .ok_or_else(|| serde::de::Error::missing_field("app_token"))?,
                user_token: raw
                    .user_token
                    .ok_or_else(|| serde::de::Error::missing_field("user_token"))?,
                app_id: raw.slack_app_id,
            },
            ConnectorType::Gmail => ConnectionSettings::Gmail {
                credentials_file: raw.credentials_file,
            },
            ConnectorType::Calendar => ConnectionSettings::Calendar {
                credentials_file: raw.credentials_file,
                calendar_ids: raw.calendar_ids.unwrap_or_default(),
            },
            ConnectorType::WhatsApp => ConnectionSettings::WhatsApp {},
            ConnectorType::Telegram => ConnectionSettings::Telegram {
                api_id: raw.api_id,
                api_hash: raw.api_hash,
            },
            ConnectorType::HackerNews => ConnectionSettings::HackerNews {
                keywords: raw.keywords.unwrap_or_default(),
                min_score: raw.min_score.unwrap_or(0),
            },
        };
        Ok(ConnectionConfig {
            id: raw.id,
            connector_type: raw.connector_type,
            ignore_conversations: raw.ignore_conversations.unwrap_or_default(),
            settings,
        })
    }
}

#[derive(Deserialize)]
struct RawConnectionConfig {
    id: String,
    #[serde(rename = "type")]
    connector_type: ConnectorType,
    #[serde(default)]
    app_token: Option<String>,
    #[serde(default)]
    user_token: Option<String>,
    #[serde(default)]
    credentials_file: Option<String>,
    #[serde(default)]
    calendar_ids: Option<Vec<String>>,
    #[serde(default)]
    api_id: Option<i32>,
    #[serde(default)]
    api_hash: Option<String>,
    #[serde(default, rename = "app_id")]
    slack_app_id: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    min_score: Option<u32>,
    #[serde(default)]
    ignore_conversations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConnectionSettings {
    Slack {
        app_token: String,
        user_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
    },
    Gmail {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials_file: Option<String>,
    },
    Calendar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials_file: Option<String>,
        #[serde(default)]
        calendar_ids: Vec<String>,
    },
    WhatsApp {},
    Telegram {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_id: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_hash: Option<String>,
    },
    HackerNews {
        #[serde(default)]
        keywords: Vec<String>,
        #[serde(default)]
        min_score: u32,
    },
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

pub fn default_config_path() -> PathBuf {
    preferred_config_dir().join(CONFIG_FILENAME)
}

pub fn default_config() -> String {
    format!(
        r#"[store]
path = "{}"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60
hackernews_poll_interval_secs = 3600

# Example connections (uncomment and fill in):
#
# [[connections]]
# id = "whatsapp"
# type = "whatsapp"
#
# [[connections]]
# id = "work-slack"
# type = "slack"
# app_token = "xapp-1-..."
# user_token = "xoxp-..."
# # app_id = "A012ABCD0A0"  # optional — enables auto-repair of event subscriptions
#
# [[connections]]
# id = "personal-gmail"
# type = "gmail"
# # credentials_file is optional — built-in Google credentials are used by default
# # credentials_file = "~/.config/void/custom-credentials.json"
#
# [[connections]]
# id = "my-calendar"
# type = "calendar"
# calendar_ids = ["primary"]
#
# [[connections]]
# id = "telegram"
# type = "telegram"
# # Optional: override built-in API credentials
# # api_id = 12345
# # api_hash = "0123456789abcdef0123456789abcdef"
#
# [[connections]]
# id = "hackernews"
# type = "hackernews"
# keywords = ["rust", "ai", "startup"]
# min_score = 100
"#,
        default_store_path_template()
    )
}

/// Expand `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs_home().join(rest)
    } else if path == "~" {
        dirs_home()
    } else {
        PathBuf::from(path)
    }
}

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn legacy_config_dir() -> PathBuf {
    dirs_home().join(LEGACY_CONFIG_DIR)
}

#[cfg(windows)]
fn legacy_store_dir() -> PathBuf {
    dirs_home().join(LEGACY_STORE_DIR)
}

fn preferred_config_dir() -> PathBuf {
    let legacy = legacy_config_dir();
    if legacy.exists() {
        return legacy;
    }

    dirs::config_dir()
        .map(|path| path.join("void"))
        .unwrap_or(legacy)
}

#[cfg(windows)]
fn preferred_store_dir() -> PathBuf {
    let legacy = legacy_store_dir();
    if legacy.exists() {
        return legacy;
    }

    dirs::data_dir()
        .map(|path| path.join("void"))
        .unwrap_or(legacy)
}

#[cfg(windows)]
fn default_store_path_template() -> String {
    // TOML basic strings need escaped backslashes on Windows paths.
    preferred_store_dir()
        .to_string_lossy()
        .replace('\\', "\\\\")
}

#[cfg(not(windows))]
fn default_store_path_template() -> String {
    format!("~/{LEGACY_STORE_DIR}")
}

/// Redact a token for display: show first 8 chars + "..."
pub fn redact_token(token: &str) -> String {
    if token.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}...", &token[..8])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_config() {
        let toml = r#"
[store]
path = "~/.local/share/void"

[sync]
gmail_poll_interval_secs = 15
calendar_poll_interval_secs = 120

[[connections]]
id = "whatsapp"
type = "whatsapp"

[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"

[[connections]]
id = "personal-gmail"
type = "gmail"
credentials_file = "~/.config/void/gmail.json"

[[connections]]
id = "my-calendar"
type = "calendar"
credentials_file = "~/.config/void/calendar.json"
calendar_ids = ["primary", "holidays"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.connections.len(), 4);
        assert_eq!(config.sync.gmail_poll_interval_secs, 15);
        assert_eq!(config.sync.calendar_poll_interval_secs, 120);
        assert_eq!(
            config.connections[0].connector_type,
            ConnectorType::WhatsApp
        );
        assert_eq!(config.connections[1].connector_type, ConnectorType::Slack);
        assert_eq!(config.connections[2].connector_type, ConnectorType::Gmail);
        assert_eq!(
            config.connections[3].connector_type,
            ConnectorType::Calendar
        );
    }

    #[test]
    fn parse_empty_config() {
        let config: VoidConfig = toml::from_str("").unwrap();
        assert!(config.connections.is_empty());
        assert_eq!(config.sync.gmail_poll_interval_secs, 30);
        assert_eq!(config.sync.calendar_poll_interval_secs, 60);
        assert_eq!(config.sync.hackernews_poll_interval_secs, 3600);
    }

    #[test]
    fn parse_defaults() {
        let config = VoidConfig::default();
        #[cfg(windows)]
        assert!(!config.store.path.is_empty());
        #[cfg(not(windows))]
        assert!(config.store.path.contains(".local/share/void"));
        assert_eq!(config.sync.gmail_poll_interval_secs, 30);
        assert_eq!(config.sync.hackernews_poll_interval_secs, 3600);
    }

    #[test]
    fn expand_tilde_works() {
        let expanded = expand_tilde("~/foo/bar");
        assert!(expanded.ends_with("foo/bar"));
        assert!(!expanded.to_str().unwrap().starts_with('~'));

        let no_tilde = expand_tilde("/absolute/path");
        assert_eq!(no_tilde, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn expand_tilde_bare_tilde() {
        let expanded = expand_tilde("~");
        assert!(!expanded.to_str().unwrap().starts_with('~'));
        assert!(expanded.is_absolute());
    }

    #[test]
    fn expand_tilde_other_user_prefix_unchanged() {
        // Only "~/..." and exactly "~" expand; "~alice/..." is not POSIX home syntax here.
        assert_eq!(
            expand_tilde("~alice/projects"),
            PathBuf::from("~alice/projects")
        );
    }

    #[test]
    fn find_connection_returns_match() {
        let config = VoidConfig {
            store: StoreConfig::default(),
            sync: SyncConfig::default(),
            connections: vec![
                ConnectionConfig {
                    id: "work-slack".into(),
                    connector_type: ConnectorType::Slack,
                    ignore_conversations: vec![],
                    settings: ConnectionSettings::Slack {
                        app_token: "xapp".into(),
                        user_token: "xoxp".into(),
                        app_id: None,
                    },
                },
                ConnectionConfig {
                    id: "personal-gmail".into(),
                    connector_type: ConnectorType::Gmail,
                    ignore_conversations: vec![],
                    settings: ConnectionSettings::Gmail {
                        credentials_file: Some("creds.json".into()),
                    },
                },
            ],
        };
        assert!(config.find_connection("work-slack").is_some());
        assert_eq!(
            config.find_connection("work-slack").unwrap().id,
            "work-slack"
        );
        assert!(config.find_connection("nonexistent").is_none());
    }

    #[test]
    fn find_connection_by_connector_returns_match() {
        let config = VoidConfig {
            store: StoreConfig::default(),
            sync: SyncConfig::default(),
            connections: vec![ConnectionConfig {
                id: "gmail-1".into(),
                connector_type: ConnectorType::Gmail,
                ignore_conversations: vec![],
                settings: ConnectionSettings::Gmail {
                    credentials_file: Some("creds.json".into()),
                },
            }],
        };
        assert!(config.find_connection_by_connector("gmail").is_some());
        assert_eq!(
            config.find_connection_by_connector("gmail").unwrap().id,
            "gmail-1"
        );
        assert!(config.find_connection_by_connector("unknown").is_none());
    }

    #[test]
    fn redact_works() {
        assert_eq!(redact_token("xoxp-12345678-rest"), "xoxp-123...");
        assert_eq!(redact_token("short"), "***");
    }

    #[test]
    fn redact_token_exactly_eight_chars() {
        assert_eq!(redact_token("12345678"), "***");
    }

    #[test]
    fn redact_token_nine_chars_shows_prefix() {
        assert_eq!(redact_token("123456789"), "12345678...");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("void-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");

        let config = VoidConfig {
            store: StoreConfig {
                path: "~/test-store".to_string(),
            },
            sync: SyncConfig::default(),
            connections: vec![ConnectionConfig {
                id: "wa".to_string(),
                connector_type: ConnectorType::WhatsApp,
                ignore_conversations: vec![],
                settings: ConnectionSettings::WhatsApp {},
            }],
        };

        config.save(&path).unwrap();
        let loaded = VoidConfig::load(&path).unwrap();
        assert_eq!(loaded.connections.len(), 1);
        assert_eq!(loaded.store.path, "~/test-store");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_calendar_not_confused_with_gmail() {
        let toml = r#"
[[connections]]
id = "my-calendar"
type = "calendar"
credentials_file = "~/.config/void/google-creds.json"
calendar_ids = ["primary"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.connections[0].connector_type,
            ConnectorType::Calendar
        );
        match &config.connections[0].settings {
            ConnectionSettings::Calendar {
                credentials_file,
                calendar_ids,
            } => {
                assert_eq!(
                    credentials_file.as_deref(),
                    Some("~/.config/void/google-creds.json")
                );
                assert_eq!(calendar_ids, &["primary"]);
            }
            other => panic!("expected Calendar settings, got {other:?}"),
        }
    }

    #[test]
    fn parse_calendar_without_calendar_ids() {
        let toml = r#"
[[connections]]
id = "cal"
type = "calendar"
credentials_file = "creds.json"
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.connections[0].connector_type,
            ConnectorType::Calendar
        );
        match &config.connections[0].settings {
            ConnectionSettings::Calendar { calendar_ids, .. } => {
                assert!(calendar_ids.is_empty());
            }
            other => panic!("expected Calendar settings, got {other:?}"),
        }
    }

    #[test]
    fn parse_slack_config() {
        let toml = r#"
[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.connections.len(), 1);
        match &config.connections[0].settings {
            ConnectionSettings::Slack { .. } => {}
            _ => panic!("expected Slack settings"),
        }
    }

    #[test]
    fn parse_slack_with_legacy_exclude_channels_is_accepted() {
        let toml = r#"
[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
exclude_channels = ["random", "social", "C07ABC123"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        match &config.connections[0].settings {
            ConnectionSettings::Slack { .. } => {}
            _ => panic!("expected Slack settings"),
        }
    }

    #[test]
    fn parse_hackernews_config() {
        let toml = r#"
[[connections]]
id = "hackernews"
type = "hackernews"
keywords = ["rust", "ai", "startup"]
min_score = 50
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.connections[0].connector_type,
            ConnectorType::HackerNews
        );
        match &config.connections[0].settings {
            ConnectionSettings::HackerNews {
                keywords,
                min_score,
            } => {
                assert_eq!(keywords, &["rust", "ai", "startup"]);
                assert_eq!(*min_score, 50);
            }
            other => panic!("expected HackerNews settings, got {other:?}"),
        }
    }

    #[test]
    fn parse_hackernews_without_optional_fields() {
        let toml = r#"
[[connections]]
id = "hn"
type = "hackernews"
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(
            config.connections[0].connector_type,
            ConnectorType::HackerNews
        );
        match &config.connections[0].settings {
            ConnectionSettings::HackerNews {
                keywords,
                min_score,
            } => {
                assert!(keywords.is_empty());
                assert_eq!(*min_score, 0);
            }
            other => panic!("expected HackerNews settings, got {other:?}"),
        }
    }

    #[test]
    fn default_config_path_returns_config_toml_under_void_dir() {
        let path = default_config_path();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("config.toml")
        );
        assert!(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                == Some("void")
        );
    }

    #[test]
    fn default_config_contains_store_section() {
        let config_str = default_config();
        assert!(config_str.contains("[store]"));
        assert!(config_str.contains("path"));
        assert!(config_str.contains("[sync]"));
    }

    #[test]
    fn parse_ignore_conversations() {
        let toml = r#"
[[connections]]
id = "my-whatsapp"
type = "whatsapp"
ignore_conversations = ["noisy-group@g.us", "spam"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.connections[0].ignore_conversations, vec!["noisy-group@g.us", "spam"]);
    }

    #[test]
    fn parse_ignore_conversations_absent_defaults_empty() {
        let toml = r#"
[[connections]]
id = "my-whatsapp"
type = "whatsapp"
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert!(config.connections[0].ignore_conversations.is_empty());
    }

    #[test]
    fn parse_ignore_conversations_works_for_any_connector() {
        let toml = r#"
[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
ignore_conversations = ["random", "social"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.connections[0].ignore_conversations, vec!["random", "social"]);
    }
}
