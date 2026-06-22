use std::path::PathBuf;

use crate::models::ConnectorType;

use super::{
    empty_settings, settings_set_opt_string, settings_set_string, settings_str, settings_string,
    settings_string_list, settings_u32, StoreMode, *,
};
fn test_slack_settings(
    app_token: &str,
    user_token: &str,
    app_id: Option<&str>,
    config_refresh_token: Option<&str>,
) -> toml::Table {
    let mut settings = empty_settings();
    settings_set_string(&mut settings, "app_token", app_token);
    settings_set_string(&mut settings, "user_token", user_token);
    settings_set_opt_string(&mut settings, "app_id", app_id.map(|s| s.to_string()));
    settings_set_opt_string(
        &mut settings,
        "config_refresh_token",
        config_refresh_token.map(|s| s.to_string()),
    );
    settings
}

fn test_gmail_settings(credentials_file: Option<&str>) -> toml::Table {
    let mut settings = empty_settings();
    settings_set_opt_string(
        &mut settings,
        "credentials_file",
        credentials_file.map(|s| s.to_string()),
    );
    settings
}

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
    assert_eq!(config.sync.gmail_poll_interval_secs(), 15);
    assert_eq!(config.sync.calendar_poll_interval_secs(), 120);
    assert_eq!(
        config.connections[0].connector_type,
        ConnectorType::from_static("whatsapp")
    );
    assert_eq!(
        config.connections[1].connector_type,
        ConnectorType::from_static("slack")
    );
    assert_eq!(
        config.connections[2].connector_type,
        ConnectorType::from_static("gmail")
    );
    assert_eq!(
        config.connections[3].connector_type,
        ConnectorType::from_static("calendar")
    );
}

#[test]
fn parse_empty_config() {
    let config: VoidConfig = toml::from_str("").unwrap();
    assert!(config.connections.is_empty());
    assert_eq!(config.sync.gmail_poll_interval_secs(), 30);
    assert_eq!(config.sync.calendar_poll_interval_secs(), 60);
    assert_eq!(config.sync.hackernews_poll_interval_secs(), 3600);
}

#[test]
fn parse_defaults() {
    let config = VoidConfig::default();
    #[cfg(windows)]
    assert!(!config.store.path.is_empty());
    #[cfg(not(windows))]
    assert!(config.store.path.contains(".local/share/void"));
    assert_eq!(config.sync.gmail_poll_interval_secs(), 30);
    assert_eq!(config.sync.hackernews_poll_interval_secs(), 3600);
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
                connector_type: ConnectorType::from_static("slack"),
                ignore_conversations: vec![],
                settings: test_slack_settings("xapp", "xoxp", None, None),
            },
            ConnectionConfig {
                id: "personal-gmail".into(),
                connector_type: ConnectorType::from_static("gmail"),
                ignore_conversations: vec![],
                settings: test_gmail_settings(Some("creds.json")),
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
            connector_type: ConnectorType::from_static("gmail"),
            ignore_conversations: vec![],
            settings: test_gmail_settings(Some("creds.json")),
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
            connector_type: ConnectorType::from_static("whatsapp"),
            ignore_conversations: vec![],
            settings: empty_settings(),
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
        ConnectorType::from_static("calendar")
    );
    let settings = &config.connections[0].settings;
    assert_eq!(
        settings_string(settings, "credentials_file"),
        Some("~/.config/void/google-creds.json".to_string())
    );
    assert_eq!(
        settings_string_list(settings, "calendar_ids"),
        vec!["primary".to_string()]
    );
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
        ConnectorType::from_static("calendar")
    );
    assert!(settings_string_list(&config.connections[0].settings, "calendar_ids").is_empty());
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
    let settings = &config.connections[0].settings;
    assert_eq!(settings_str(settings, "app_id"), Some("A0123456"));
    assert_eq!(
        settings_str(settings, "config_refresh_token"),
        Some("xoxe-test-token")
    );
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
            connector_type: ConnectorType::from_static("slack"),
            ignore_conversations: vec![],
            settings: test_slack_settings("xapp", "xoxp", Some("A0123456"), None),
        }],
    };
    config.save(&config_path).unwrap();

    std::fs::write(
        store_dir.join("slack-config-token-work-slack.json"),
        r#"{"refresh_token":"xoxe-from-sidecar"}"#,
    )
    .unwrap();

    let loaded = VoidConfig::load(&config_path).unwrap();
    assert_eq!(
        settings_str(&loaded.connections[0].settings, "config_refresh_token"),
        Some("xoxe-from-sidecar")
    );
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
    assert!(settings_str(&config.connections[0].settings, "app_token").is_some());
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
    assert!(settings_str(&config.connections[0].settings, "app_token").is_some());
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
        ConnectorType::from_static("hackernews")
    );
    let settings = &config.connections[0].settings;
    assert_eq!(
        settings_string_list(settings, "keywords"),
        vec!["rust".to_string(), "ai".to_string(), "startup".to_string()]
    );
    assert_eq!(settings_u32(settings, "min_score"), Some(50));
}

#[test]
fn sync_config_linkedin_defaults() {
    let sync = SyncConfig::default();
    assert_eq!(sync.linkedin_poll_interval_secs(), 30 * 60);
    assert_eq!(sync.linkedin_backfill_days(), 15);
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
        ConnectorType::from_static("linkedin")
    );
    let settings = &config.connections[0].settings;
    assert_eq!(settings_str(settings, "api_key"), Some("test-api-key"));
    assert_eq!(
        settings_str(settings, "dsn"),
        Some("https://api1.unipile.com:13111")
    );
    assert_eq!(settings_str(settings, "account_id"), Some("acc-123"));
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
        ConnectorType::from_static("hackernews")
    );
    let settings = &config.connections[0].settings;
    assert!(settings_string_list(settings, "keywords").is_empty());
    assert_eq!(settings_u32(settings, "min_score"), None);
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
            connector_type: ConnectorType::from_static("slack"),
            ignore_conversations: vec!["random".into()],
            settings: test_slack_settings("xapp", "xoxp", None, None),
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

// ---- Area F: legacy [[accounts]] -> [[connections]] migration ----

#[test]
fn parse_migrates_legacy_accounts_table_to_connections() {
    // Old configs used [[accounts]]; parse() rewrites it to [[connections]].
    let toml = r#"
[[accounts]]
id = "work-slack"
type = "slack"
app_token = "xapp-1-test"
user_token = "xoxp-test"

[[accounts]]
id = "wa"
type = "whatsapp"
"#;
    let config = VoidConfig::parse(toml).unwrap();
    assert_eq!(
        config.connections.len(),
        2,
        "accounts surfaced as connections"
    );
    assert_eq!(config.connections[0].id, "work-slack");
    assert_eq!(
        config.connections[0].connector_type,
        ConnectorType::from_static("slack")
    );
    assert_eq!(
        config.connections[1].connector_type,
        ConnectorType::from_static("whatsapp")
    );
}

#[test]
fn load_migrates_accounts_table_and_rewrites_file_on_disk() {
    let dir = std::env::temp_dir().join(format!("void-cfg-accounts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        r#"
[store]
path = "/tmp/void-test-store"

[[accounts]]
id = "wa"
type = "whatsapp"
"#,
    )
    .unwrap();

    let config = VoidConfig::load(&path).unwrap();
    assert_eq!(config.connections.len(), 1);
    assert_eq!(
        config.connections[0].connector_type,
        ConnectorType::from_static("whatsapp")
    );

    // The on-disk file is migrated in place to the new table name.
    let rewritten = std::fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("[[connections]]"),
        "file rewritten to [[connections]]: {rewritten}"
    );
    assert!(
        !rewritten.contains("[[accounts]]"),
        "legacy table name removed: {rewritten}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

// ---- Area F: unknown connector type parses (validated at runtime via registry) ----

#[test]
fn parse_unknown_connector_type_is_accepted_at_load() {
    let toml = r#"
[[connections]]
id = "mystery"
type = "myspace"
"#;
    let config = VoidConfig::parse(toml).expect("unknown types are stored as strings");
    assert_eq!(config.connections[0].connector_type.as_str(), "myspace");
}

#[test]
fn raw_toml_unknown_connector_type_deserializes() {
    let toml = r#"
[[connections]]
id = "x"
type = "definitely-not-a-connector"
"#;
    let config: VoidConfig = toml::from_str(toml).expect("deserialize");
    assert_eq!(
        config.connections[0].connector_type.as_str(),
        "definitely-not-a-connector"
    );
}
