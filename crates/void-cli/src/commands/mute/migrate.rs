use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use void_core::config::{conversation_matches_ignore, VoidConfig};
use void_core::db::Database;
use void_core::models::Conversation;

#[derive(Serialize)]
pub struct MigratedMute {
    pub connection_id: String,
    pub external_id: String,
    pub name: Option<String>,
}

pub(super) fn migrate_legacy_mutes(
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

pub(super) fn resolve_migration_connection(
    cfg: &VoidConfig,
    conv: &Conversation,
) -> Option<String> {
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
