use std::sync::Arc;

use clap::Args;
use tokio_util::sync::CancellationToken;

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::sync::SyncEngine;

use crate::commands::channel_factory;

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Sync only specific channels (comma-separated: whatsapp,slack,gmail,calendar)
    #[arg(long)]
    pub channels: Option<String>,
    /// Run as background daemon
    #[arg(long)]
    pub daemon: bool,
}

pub async fn run(args: &SyncArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void config init` first.",
            config_path.display()
        )
    })?;

    if cfg.accounts.is_empty() {
        anyhow::bail!("No accounts configured. Add accounts to your config.toml first.");
    }

    let channel_filter: Option<Vec<String>> = args
        .channels
        .as_ref()
        .map(|c| c.split(',').map(|s| s.trim().to_lowercase()).collect());

    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    let db = Arc::new(Database::open(&cfg.db_path())?);
    let mut channels: Vec<Arc<dyn void_core::channel::Channel>> = Vec::new();

    for account in &cfg.accounts {
        if let Some(ref filter) = channel_filter {
            let type_str = account.account_type.to_string();
            if !filter.iter().any(|f| type_str.contains(f)) {
                continue;
            }
        }

        match channel_factory::build_channel(account, &store_path) {
            Ok(channel) => channels.push(channel),
            Err(e) => {
                eprintln!(
                    "[warn] Skipping account '{}' ({}): {e}",
                    account.id, account.account_type
                );
            }
        }
    }

    if channels.is_empty() {
        anyhow::bail!("No channels to sync (check your config and --channels filter).");
    }

    eprintln!(
        "Starting sync for {} channel(s)... (Ctrl+C to stop)",
        channels.len()
    );

    let cancel = CancellationToken::new();
    let engine = SyncEngine::new(channels, db, &store_path);
    engine.run(cancel).await
}
