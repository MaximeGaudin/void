use std::collections::HashSet;
use std::path::Path;

use clap::Args;
use serde::Serialize;
use tracing::debug;
use void_core::config::{conversation_matches_ignore, VoidConfig};
use void_core::db::Database;
use void_core::models::Conversation;

use crate::output::{resolve_connector_filter, CONNECTOR_FILTER_HELP};

#[derive(Debug, Args)]
pub struct MuteArgs {
    /// Channel/conversation names or IDs to mute (supports partial match)
    pub targets: Vec<String>,
    /// Unmute instead of mute
    #[arg(long)]
    pub unmute: bool,
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    #[arg(long, help = CONNECTOR_FILTER_HELP)]
    pub connector: Option<String>,
    /// List all currently muted conversations
    #[arg(long)]
    pub list: bool,
    /// One-time import of database mutes into config.toml ignore_conversations
    #[arg(long)]
    pub migrate_legacy: bool,
}

pub fn run(args: &MuteArgs) -> anyhow::Result<()> {
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config from {}: {e}", config_path.display()))?;
    let db = crate::context::open_db()?;

    if args.list {
        return list_muted(&cfg, &db, args.connection.as_deref(), connector.as_deref());
    }

    if args.migrate_legacy {
        return migrate_legacy_mutes(&mut cfg, &db, &config_path);
    }

    if args.targets.is_empty() {
        anyhow::bail!(
            "provide at least one channel/conversation name or ID, or use --list or --migrate-legacy"
        );
    }

    let mute = !args.unmute;
    let action = if mute { "muted" } else { "unmuted" };
    let mut results = Vec::new();
    let mut affected_connections = HashSet::new();
    let mut config_changed = false;

    for target in &args.targets {
        debug!(target, mute, "processing mute target");

        let matches = resolve_targets(
            &db,
            target,
            args.connection.as_deref(),
            connector.as_deref(),
        )?;

        if matches.is_empty() {
            eprintln!("no conversation matching \"{target}\" found");
            results.push(serde_json::json!({
                "target": target,
                "error": "not found",
            }));
            continue;
        }

        for conv in matches {
            let changed = if mute {
                cfg.add_ignore_conversation(&conv.connection_id, conv.external_id.clone())
            } else {
                cfg.remove_ignore_conversation(
                    &conv.connection_id,
                    &conv.external_id,
                    conv.name.as_deref(),
                )
            };
            config_changed |= changed;
            affected_connections.insert(conv.connection_id.clone());

            let name = conv.name.as_deref().unwrap_or(&conv.id);
            eprintln!("{action}: {name} [{}] ({})", conv.connector, conv.id);
            results.push(serde_json::json!({
                "id": conv.id,
                "name": name,
                "connector": conv.connector,
                "connection_id": conv.connection_id,
                "external_id": conv.external_id,
                "is_muted": mute,
            }));
        }
    }

    if config_changed {
        cfg.save(&config_path)?;
        for connection_id in &affected_connections {
            if let Some(conn) = cfg.connections.iter().find(|c| c.id == *connection_id) {
                db.sync_ignore_conversations(&conn.id, &conn.ignore_conversations)?;
            }
        }
    }

    println!("{}", serde_json::json!({ "data": results, "error": null }));
    Ok(())
}

fn resolve_targets(
    db: &Database,
    target: &str,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<Vec<Conversation>> {
    if let Some(conv) = db.get_conversation(target)? {
        if connection_filter.is_some_and(|filter| !conv.connection_id.contains(filter)) {
            return Ok(vec![]);
        }
        if connector_filter.is_some_and(|filter| conv.connector != filter) {
            return Ok(vec![]);
        }
        return Ok(vec![conv]);
    }

    let matches = db.list_channels(connection_filter, connector_filter, Some(target), 100, true)?;
    let dm_matches = find_conversations_by_name(db, target, connection_filter, connector_filter)?;
    let mut seen = HashSet::new();
    Ok(matches
        .into_iter()
        .chain(dm_matches)
        .filter(|conv| seen.insert(conv.id.clone()))
        .collect())
}

fn list_muted(
    cfg: &VoidConfig,
    db: &Database,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<()> {
    let mut items = Vec::new();

    for conn in &cfg.connections {
        if connection_filter.is_some_and(|filter| !conn.id.contains(filter)) {
            continue;
        }
        if connector_filter.is_some_and(|filter| conn.connector_type.to_string() != filter) {
            continue;
        }
        if conn.ignore_conversations.is_empty() {
            continue;
        }

        let conversations = db.list_conversations(Some(&conn.id), None, 10_000, true)?;

        for pattern in &conn.ignore_conversations {
            let matches: Vec<_> = conversations
                .iter()
                .filter(|c| {
                    conversation_matches_ignore(
                        c.name.as_deref(),
                        &c.external_id,
                        std::slice::from_ref(pattern),
                    )
                })
                .collect();

            if matches.is_empty() {
                items.push(serde_json::json!({
                    "connection_id": conn.id,
                    "connector": conn.connector_type.to_string(),
                    "pattern": pattern,
                }));
                continue;
            }

            for conv in matches {
                items.push(serde_json::json!({
                    "id": conv.id,
                    "name": conv.name,
                    "connector": conv.connector,
                    "connection_id": conv.connection_id,
                    "pattern": pattern,
                }));
            }
        }
    }

    println!("{}", serde_json::json!({ "data": items, "error": null }));
    Ok(())
}

fn find_conversations_by_name(
    db: &Database,
    search: &str,
    connection_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<Vec<Conversation>> {
    let all = db.list_conversations(connection_filter, connector_filter, 500, true)?;
    let lower = search.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|c| {
            c.name
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        })
        .collect())
}

#[derive(Serialize)]
pub struct MigratedMute {
    pub connection_id: String,
    pub external_id: String,
    pub name: Option<String>,
}

fn migrate_legacy_mutes(
    cfg: &mut VoidConfig,
    db: &Database,
    config_path: &Path,
) -> anyhow::Result<()> {
    let migrated = migrate_db_mutes_to_config(cfg, db, config_path)?;
    for conn in &cfg.connections {
        db.sync_ignore_conversations(&conn.id, &conn.ignore_conversations)?;
    }

    if migrated.is_empty() {
        eprintln!("No database mutes needed importing — config.toml is already up to date.");
    } else {
        eprintln!(
            "Imported {} muted conversation(s) into config.toml:",
            migrated.len()
        );
        for item in &migrated {
            let label = item.name.as_deref().unwrap_or(&item.external_id);
            eprintln!(
                "  [{}] {} ({})",
                item.connection_id, label, item.external_id
            );
        }
        eprintln!("Saved {}", config_path.display());
    }

    println!("{}", serde_json::json!({ "data": migrated, "error": null }));
    Ok(())
}

pub(crate) fn run_one_time_legacy_mute_migration(
    cfg: &mut VoidConfig,
    db: &Database,
    config_path: &Path,
) -> anyhow::Result<usize> {
    if db
        .get_sync_state("_void", "legacy_mutes_migrated")?
        .is_some()
    {
        return Ok(0);
    }

    let migrated = migrate_db_mutes_to_config(cfg, db, config_path)?;
    for conn in &cfg.connections {
        db.sync_ignore_conversations(&conn.id, &conn.ignore_conversations)?;
    }
    db.set_sync_state("_void", "legacy_mutes_migrated", "1")?;
    Ok(migrated.len())
}

fn resolve_migration_connection(cfg: &VoidConfig, conv: &Conversation) -> Option<String> {
    if cfg.connections.iter().any(|c| c.id == conv.connection_id) {
        return Some(conv.connection_id.clone());
    }

    let matching: Vec<_> = cfg
        .connections
        .iter()
        .filter(|c| c.connector_type.to_string() == conv.connector)
        .collect();
    if matching.len() == 1 {
        return Some(matching[0].id.clone());
    }

    None
}

pub(crate) fn migrate_db_mutes_to_config(
    cfg: &mut VoidConfig,
    db: &Database,
    config_path: &Path,
) -> anyhow::Result<Vec<MigratedMute>> {
    let mut migrated = Vec::new();
    let mut seen = HashSet::new();

    for conv in db.list_muted_conversations()? {
        let Some(connection_id) = resolve_migration_connection(cfg, &conv) else {
            continue;
        };
        if !seen.insert((connection_id.clone(), conv.external_id.clone())) {
            continue;
        }

        let patterns = cfg
            .connections
            .iter()
            .find(|c| c.id == connection_id)
            .map(|c| c.ignore_conversations.clone())
            .unwrap_or_default();
        if conversation_matches_ignore(conv.name.as_deref(), &conv.external_id, &patterns) {
            continue;
        }
        if cfg.add_ignore_conversation(&connection_id, conv.external_id.clone()) {
            migrated.push(MigratedMute {
                connection_id,
                external_id: conv.external_id.clone(),
                name: conv.name.clone(),
            });
        }
    }

    if !migrated.is_empty() {
        cfg.save(config_path)?;
    }
    Ok(migrated)
}

#[cfg(test)]
mod tests {
    use super::{migrate_db_mutes_to_config, resolve_migration_connection, MigratedMute};
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
        let dir =
            std::env::temp_dir().join(format!("void-mute-migrate-skip-{}", uuid::Uuid::new_v4()));
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
}
