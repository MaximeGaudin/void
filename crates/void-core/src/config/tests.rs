use std::path::PathBuf;

use crate::models::ConnectorType;

use super::{StoreMode, *};

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
                    config_refresh_token: None,
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
            ..Default::default()
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
fn parse_slack_with_config_refresh_token() {
    let toml = r#"
[[connections]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"
app_id = "A0123456"
config_refresh_token = "xoxe-test-token"
"#;
    let config: VoidConfig = toml::from_str(toml).unwrap();
    match &config.connections[0].settings {
        ConnectionSettings::Slack {
            app_id,
            config_refresh_token,
            ..
        } => {
            assert_eq!(app_id.as_deref(), Some("A0123456"));
            assert_eq!(config_refresh_token.as_deref(), Some("xoxe-test-token"));
        }
        other => panic!("expected Slack settings, got {other:?}"),
    }
}

#[test]
fn migrate_slack_sidecar_token_into_config() {
    let dir = std::env::temp_dir().join(format!("void-test-migrate-{}", uuid::Uuid::new_v4()));
    let store_dir = dir.join("store");
    std::fs::create_dir_all(&store_dir).unwrap();
    let config_path = dir.join("config.toml");

    let config = VoidConfig {
        store: StoreConfig {
            path: store_dir.to_string_lossy().into_owned(),
            ..Default::default()
        },
        sync: SyncConfig::default(),
        connections: vec![ConnectionConfig {
            id: "work-slack".into(),
            connector_type: ConnectorType::Slack,
            ignore_conversations: vec![],
            settings: ConnectionSettings::Slack {
                app_token: "xapp".into(),
                user_token: "xoxp".into(),
                app_id: Some("A0123456".into()),
                config_refresh_token: None,
            },
        }],
    };
    config.save(&config_path).unwrap();

    std::fs::write(
        store_dir.join("slack-config-token-work-slack.json"),
        r#"{"refresh_token":"xoxe-from-sidecar"}"#,
    )
    .unwrap();

    let loaded = VoidConfig::load(&config_path).unwrap();
    match &loaded.connections[0].settings {
        ConnectionSettings::Slack {
            config_refresh_token,
            ..
        } => {
            assert_eq!(config_refresh_token.as_deref(), Some("xoxe-from-sidecar"));
        }
        other => panic!("expected Slack settings, got {other:?}"),
    }
    assert!(!store_dir
        .join("slack-config-token-work-slack.json")
        .exists());

    std::fs::remove_dir_all(&dir).ok();
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
fn sync_config_linkedin_defaults() {
    let sync = SyncConfig::default();
    assert_eq!(sync.linkedin_poll_interval_secs, 30 * 60);
    assert_eq!(sync.linkedin_backfill_days, 15);
}

#[test]
fn parse_linkedin_config() {
    let toml = r#"
[[connections]]
id = "linkedin"
type = "linkedin"
api_key = "test-api-key"
dsn = "https://api1.unipile.com:13111"
account_id = "acc-123"
"#;
    let config: VoidConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        config.connections[0].connector_type,
        ConnectorType::LinkedIn
    );
    match &config.connections[0].settings {
        ConnectionSettings::LinkedIn {
            api_key,
            dsn,
            account_id,
        } => {
            assert_eq!(api_key, "test-api-key");
            assert_eq!(dsn, "https://api1.unipile.com:13111");
            assert_eq!(account_id, "acc-123");
        }
        other => panic!("expected LinkedIn settings, got {other:?}"),
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
fn resolve_config_path_expands_tilde() {
    let path = super::resolve_config_path(Some(std::path::Path::new("~/.config/void/config.toml")));
    assert!(path.exists() || !path.to_string_lossy().contains('~'));
    assert!(
        path.ends_with("void/config.toml"),
        "unexpected path: {}",
        path.display()
    );
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
fn add_and_remove_ignore_conversation() {
    let mut cfg = VoidConfig {
        store: StoreConfig::default(),
        sync: SyncConfig::default(),
        connections: vec![ConnectionConfig {
            id: "work-slack".into(),
            connector_type: ConnectorType::Slack,
            ignore_conversations: vec!["random".into()],
            settings: ConnectionSettings::Slack {
                app_token: "xapp".into(),
                user_token: "xoxp".into(),
                app_id: None,
                config_refresh_token: None,
            },
        }],
    };

    assert!(!cfg.add_ignore_conversation("work-slack", "random".into()));
    assert!(cfg.add_ignore_conversation("work-slack", "C123".into()));
    assert_eq!(
        cfg.connections[0].ignore_conversations,
        vec!["random", "C123"]
    );

    assert!(cfg.remove_ignore_conversation("work-slack", "C123", None));
    assert_eq!(cfg.connections[0].ignore_conversations, vec!["random"]);
}

#[test]
fn conversation_matches_ignore_patterns() {
    assert!(conversation_matches_ignore(
        Some("Random Chat"),
        "C123",
        &["random".into()]
    ));
    assert!(conversation_matches_ignore(
        None,
        "noisy-group@g.us",
        &["noisy".into()]
    ));
    assert!(!conversation_matches_ignore(
        Some("Work Updates"),
        "C456",
        &["random".into()]
    ));
}

#[test]
fn conversation_matches_ignore_empty_patterns_returns_false() {
    assert!(!conversation_matches_ignore(
        Some("Random Chat"),
        "C123",
        &[]
    ));
}

#[test]
fn conversation_matches_ignore_case_insensitive_name() {
    assert!(conversation_matches_ignore(
        Some("RANDOM Chat"),
        "C123",
        &["random".into()]
    ));
    assert!(conversation_matches_ignore(
        Some("Work Updates"),
        "C456",
        &["WORK".into()]
    ));
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
fn parse_remote_store_config() {
    let toml = r#"
[store]
mode = "remote"

[store.remote]
host = "homeserver"
user = "alice"
remote_config_path = "~/.config/void/config.toml"

[store.remote.cache]
database_ttl_secs = 15
"#;
    let config: VoidConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.store.mode, StoreMode::Remote);
    let remote = config.store.remote.as_ref().unwrap();
    assert_eq!(remote.host, "homeserver");
    assert_eq!(remote.user.as_deref(), Some("alice"));
    assert_eq!(remote.remote_config_path, "~/.config/void/config.toml");
    assert_eq!(remote.cache.database_ttl_secs, 15);
    assert!(remote.proxy_writes);
    assert!(config.is_remote_client_profile());
}

#[test]
fn server_config_with_connections_is_not_remote_client_profile() {
    let toml = r#"
[store]
path = "~/.local/share/void"

[[connections]]
id = "work-gmail"
type = "gmail"
"#;
    let config: VoidConfig = toml::from_str(toml).unwrap();
    assert!(!config.is_remote_client_profile());
}

#[test]
fn void_config_parse_readonly_does_not_touch_disk() {
    let toml = r#"
[[connections]]
id = "wa"
type = "whatsapp"
"#;
    let config = VoidConfig::parse(toml).unwrap();
    assert_eq!(config.connections.len(), 1);
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
