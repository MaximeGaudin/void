use super::migrate::{migrate_db_mutes_to_config, resolve_migration_connection, MigratedMute};
use void_core::config::{
    ConnectionConfig, ConnectionSettings, StoreConfig, SyncConfig, VoidConfig,
};
use void_core::db::Database;
use void_core::models::{ConnectorType, Conversation, ConversationKind};

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

fn make_config(connection_id: &str, connector: ConnectorType) -> VoidConfig {
    let settings = match connector {
        ConnectorType::Slack => ConnectionSettings::Slack {
            app_token: "xapp".into(),
            user_token: "xoxp".into(),
            app_id: None,
            config_refresh_token: None,
        },
        ConnectorType::WhatsApp => ConnectionSettings::WhatsApp {},
        _ => ConnectionSettings::WhatsApp {},
    };
    VoidConfig {
        store: StoreConfig::default(),
        sync: SyncConfig::default(),
        connections: vec![ConnectionConfig {
            id: connection_id.into(),
            connector_type: connector,
            ignore_conversations: vec![],
            settings,
        }],
    }
}

fn make_muted_conversation(
    id: &str,
    connection_id: &str,
    connector: &str,
    external_id: &str,
    name: Option<&str>,
) -> Conversation {
    Conversation {
        id: id.into(),
        connection_id: connection_id.into(),
        connector: connector.into(),
        external_id: external_id.into(),
        name: name.map(|s| s.to_string()),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: true,
        metadata: None,
    }
}

#[test]
fn resolve_migration_connection_matches_by_connection_id() {
    let cfg = make_config("work-slack", ConnectorType::Slack);
    let conv = make_muted_conversation("c1", "work-slack", "slack", "C123", Some("random"));
    assert_eq!(
        resolve_migration_connection(&cfg, &conv).as_deref(),
        Some("work-slack")
    );
}

#[test]
fn resolve_migration_connection_falls_back_to_single_connector_match() {
    let cfg = make_config("my-slack", ConnectorType::Slack);
    let conv = make_muted_conversation("c1", "legacy-id", "slack", "C123", Some("random"));
    assert_eq!(
        resolve_migration_connection(&cfg, &conv).as_deref(),
        Some("my-slack")
    );
}

#[test]
fn resolve_migration_connection_ambiguous_connector_returns_none() {
    let mut cfg = make_config("slack-a", ConnectorType::Slack);
    cfg.connections.push(ConnectionConfig {
        id: "slack-b".into(),
        connector_type: ConnectorType::Slack,
        ignore_conversations: vec![],
        settings: ConnectionSettings::Slack {
            app_token: "xapp".into(),
            user_token: "xoxp".into(),
            app_id: None,
            config_refresh_token: None,
        },
    });
    let conv = make_muted_conversation("c1", "unknown", "slack", "C123", None);
    assert!(resolve_migration_connection(&cfg, &conv).is_none());
}

#[test]
fn migrate_db_mutes_to_config_imports_muted_conversations() {
    let dir = std::env::temp_dir().join(format!("void-mute-migrate-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");

    let mut cfg = make_config("work-slack", ConnectorType::Slack);
    let db = test_db();
    let conv = make_muted_conversation("c1", "work-slack", "slack", "C-noisy", Some("noisy"));
    db.upsert_conversation(&conv).unwrap();
    db.update_conversation_mute("c1", true).unwrap();

    let migrated = migrate_db_mutes_to_config(&mut cfg, &db, &config_path).unwrap();
    assert_eq!(migrated.len(), 1);
    assert_eq!(migrated[0].external_id, "C-noisy");
    assert_eq!(cfg.connections[0].ignore_conversations, vec!["C-noisy"]);
    assert!(config_path.exists());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn migrate_db_mutes_to_config_skips_already_ignored() {
    let dir = std::env::temp_dir().join(format!("void-mute-migrate-skip-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");

    let mut cfg = make_config("work-slack", ConnectorType::Slack);
    cfg.connections[0]
        .ignore_conversations
        .push("C-noisy".into());

    let db = test_db();
    let conv = make_muted_conversation("c1", "work-slack", "slack", "C-noisy", Some("noisy"));
    db.upsert_conversation(&conv).unwrap();
    db.update_conversation_mute("c1", true).unwrap();

    let migrated: Vec<MigratedMute> =
        migrate_db_mutes_to_config(&mut cfg, &db, &config_path).unwrap();
    assert!(migrated.is_empty());
    assert!(!config_path.exists());

    std::fs::remove_dir_all(&dir).ok();
}
