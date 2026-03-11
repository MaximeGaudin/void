use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG_DIR: &str = ".config/void";
const DEFAULT_STORE_DIR: &str = ".local/share/void";
const CONFIG_FILENAME: &str = "config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoidConfig {
    #[serde(default)]
    pub store: StoreConfig,
    #[serde(default)]
    pub sync: SyncConfig,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
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
    format!("~/{DEFAULT_STORE_DIR}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_gmail_poll")]
    pub gmail_poll_interval_secs: u64,
    #[serde(default = "default_calendar_poll")]
    pub calendar_poll_interval_secs: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            gmail_poll_interval_secs: default_gmail_poll(),
            calendar_poll_interval_secs: default_calendar_poll(),
        }
    }
}

fn default_gmail_poll() -> u64 {
    30
}

fn default_calendar_poll() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    #[serde(flatten)]
    pub settings: AccountSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    WhatsApp,
    Slack,
    Gmail,
    Calendar,
}

impl std::fmt::Display for AccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WhatsApp => write!(f, "whatsapp"),
            Self::Slack => write!(f, "slack"),
            Self::Gmail => write!(f, "gmail"),
            Self::Calendar => write!(f, "calendar"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AccountSettings {
    Slack {
        app_token: String,
        user_token: String,
        #[serde(default)]
        exclude_channels: Vec<String>,
    },
    Gmail {
        credentials_file: String,
    },
    Calendar {
        credentials_file: String,
        #[serde(default)]
        calendar_ids: Vec<String>,
    },
    WhatsApp {},
}

impl VoidConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
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

    pub fn find_account(&self, account_id: &str) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| a.id == account_id)
    }

    /// Find a config account by connector type string ("slack", "gmail", "whatsapp", "calendar").
    pub fn find_account_by_connector(&self, connector: &str) -> Option<&AccountConfig> {
        let target = match connector {
            "whatsapp" => AccountType::WhatsApp,
            "slack" => AccountType::Slack,
            "gmail" => AccountType::Gmail,
            "calendar" => AccountType::Calendar,
            _ => return None,
        };
        self.accounts.iter().find(|a| a.account_type == target)
    }
}

pub fn default_config_path() -> PathBuf {
    let home = dirs_home();
    home.join(DEFAULT_CONFIG_DIR).join(CONFIG_FILENAME)
}

pub fn default_config() -> String {
    r#"[store]
path = "~/.local/share/void"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60

# Example accounts (uncomment and fill in):
#
# [[accounts]]
# id = "whatsapp"
# type = "whatsapp"
#
# [[accounts]]
# id = "work-slack"
# type = "slack"
# app_token = "xapp-1-..."
# user_token = "xoxp-..."
# exclude_channels = ["random", "social"]
#
# [[accounts]]
# id = "personal-gmail"
# type = "gmail"
# credentials_file = "~/.config/void/gmail-personal.json"
#
# [[accounts]]
# id = "my-calendar"
# type = "calendar"
# credentials_file = "~/.config/void/calendar.json"
# calendar_ids = ["primary"]
"#
    .to_string()
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
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
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

[[accounts]]
id = "whatsapp"
type = "whatsapp"

[[accounts]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"

[[accounts]]
id = "personal-gmail"
type = "gmail"
credentials_file = "~/.config/void/gmail.json"

[[accounts]]
id = "my-calendar"
type = "calendar"
credentials_file = "~/.config/void/calendar.json"
calendar_ids = ["primary", "holidays"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.accounts.len(), 4);
        assert_eq!(config.sync.gmail_poll_interval_secs, 15);
        assert_eq!(config.sync.calendar_poll_interval_secs, 120);
        assert_eq!(config.accounts[0].account_type, AccountType::WhatsApp);
        assert_eq!(config.accounts[1].account_type, AccountType::Slack);
        assert_eq!(config.accounts[2].account_type, AccountType::Gmail);
        assert_eq!(config.accounts[3].account_type, AccountType::Calendar);
    }

    #[test]
    fn parse_empty_config() {
        let config: VoidConfig = toml::from_str("").unwrap();
        assert!(config.accounts.is_empty());
        assert_eq!(config.sync.gmail_poll_interval_secs, 30);
        assert_eq!(config.sync.calendar_poll_interval_secs, 60);
    }

    #[test]
    fn parse_defaults() {
        let config = VoidConfig::default();
        assert!(config.store.path.contains(".local/share/void"));
        assert_eq!(config.sync.gmail_poll_interval_secs, 30);
    }

    #[test]
    fn expand_tilde_works() {
        let expanded = expand_tilde("~/foo/bar");
        assert!(expanded.to_str().unwrap().ends_with("/foo/bar"));
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
    fn find_account_returns_match() {
        let config = VoidConfig {
            store: StoreConfig::default(),
            sync: SyncConfig::default(),
            accounts: vec![
                AccountConfig {
                    id: "work-slack".into(),
                    account_type: AccountType::Slack,
                    settings: AccountSettings::Slack {
                        app_token: "xapp".into(),
                        user_token: "xoxp".into(),
                        exclude_channels: vec![],
                    },
                },
                AccountConfig {
                    id: "personal-gmail".into(),
                    account_type: AccountType::Gmail,
                    settings: AccountSettings::Gmail {
                        credentials_file: "creds.json".into(),
                    },
                },
            ],
        };
        assert!(config.find_account("work-slack").is_some());
        assert_eq!(config.find_account("work-slack").unwrap().id, "work-slack");
        assert!(config.find_account("nonexistent").is_none());
    }

    #[test]
    fn find_account_by_connector_returns_match() {
        let config = VoidConfig {
            store: StoreConfig::default(),
            sync: SyncConfig::default(),
            accounts: vec![AccountConfig {
                id: "gmail-1".into(),
                account_type: AccountType::Gmail,
                settings: AccountSettings::Gmail {
                    credentials_file: "creds.json".into(),
                },
            }],
        };
        assert!(config.find_account_by_connector("gmail").is_some());
        assert_eq!(
            config.find_account_by_connector("gmail").unwrap().id,
            "gmail-1"
        );
        assert!(config.find_account_by_connector("unknown").is_none());
    }

    #[test]
    fn redact_works() {
        assert_eq!(redact_token("xoxp-12345678-rest"), "xoxp-123...");
        assert_eq!(redact_token("short"), "***");
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
            accounts: vec![AccountConfig {
                id: "wa".to_string(),
                account_type: AccountType::WhatsApp,
                settings: AccountSettings::WhatsApp {},
            }],
        };

        config.save(&path).unwrap();
        let loaded = VoidConfig::load(&path).unwrap();
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.store.path, "~/test-store");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_slack_exclude_channels() {
        let toml = r#"
[[accounts]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
exclude_channels = ["random", "social", "C07ABC123"]
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.accounts.len(), 1);
        match &config.accounts[0].settings {
            AccountSettings::Slack {
                exclude_channels, ..
            } => {
                assert_eq!(exclude_channels.len(), 3);
                assert_eq!(exclude_channels[0], "random");
                assert_eq!(exclude_channels[2], "C07ABC123");
            }
            _ => panic!("expected Slack settings"),
        }
    }

    #[test]
    fn parse_slack_without_exclude_channels_defaults_empty() {
        let toml = r#"
[[accounts]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
"#;
        let config: VoidConfig = toml::from_str(toml).unwrap();
        match &config.accounts[0].settings {
            AccountSettings::Slack {
                exclude_channels, ..
            } => {
                assert!(exclude_channels.is_empty());
            }
            _ => panic!("expected Slack settings"),
        }
    }
}
