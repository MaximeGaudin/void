use clap::Args;
use void_core::config::VoidConfig;

use crate::output::{resolve_connector_filter, CONNECTOR_FILTER_HELP};
use crate::service::writes::{self, MuteParams};

mod list;
mod migrate;
pub(crate) mod resolve;

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

    let params = MuteParams {
        targets: &args.targets,
        unmute: args.unmute,
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
    };

    let value = writes::mute(&db, &mut cfg, &config_path, params)?;
    println!("{value}");
    Ok(())
}
