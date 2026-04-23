use std::path::PathBuf;

use crate::models::ConnectorType;

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
    assert_eq!(
        config.connections[0].ignore_conversations,
        vec!["noisy-group@g.us", "spam"]
    );
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
    assert_eq!(
        config.connections[0].ignore_conversations,
        vec!["random", "social"]
    );
}
