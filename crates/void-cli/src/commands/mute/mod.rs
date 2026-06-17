use std::collections::HashSet;

use clap::Args;
use tracing::debug;
use void_core::config::VoidConfig;

use crate::output::{resolve_connector_filter, CONNECTOR_FILTER_HELP};

mod list;
mod migrate;
mod resolve;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use migrate::MigratedMute;

pub(crate) use migrate::run_one_time_legacy_mute_migration;

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
        return list::list_muted(&cfg, &db, args.connection.as_deref(), connector.as_deref());
    }

    if args.migrate_legacy {
        return migrate::migrate_legacy_mutes(&mut cfg, &db, &config_path);
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

        let matches = resolve::resolve_targets(
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
