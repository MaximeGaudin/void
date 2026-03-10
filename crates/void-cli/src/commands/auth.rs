use clap::Args;
use tracing::info;

use void_core::config::{self, VoidConfig};

use crate::commands::channel_factory;

#[derive(Debug, Args)]
pub struct AuthArgs {
    /// Channel type: whatsapp, slack, gmail, calendar
    pub channel_type: String,
    /// Account name (must match id in config.toml)
    pub account_name: Option<String>,
}

pub async fn run(args: &AuthArgs) -> anyhow::Result<()> {
    info!(channel_type = %args.channel_type, account = ?args.account_name, "starting auth");
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path).map_err(|e| {
        anyhow::anyhow!(
            "Cannot load config from {}: {e}\nRun `void config init` first.",
            config_path.display()
        )
    })?;

    let target_type = args.channel_type.to_lowercase();
    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let type_matches = a.account_type.to_string() == target_type;
            let name_matches = args.account_name.as_ref().map_or(true, |n| a.id == *n);
            type_matches && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} account{} found in config.toml",
                args.channel_type,
                args.account_name
                    .as_ref()
                    .map(|n| format!(" named '{n}'"))
                    .unwrap_or_default()
            )
        })?;

    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    let mut channel = channel_factory::build_channel(account, &store_path)?;

    eprintln!(
        "Authenticating {} account '{}'...",
        account.account_type, account.id
    );

    let channel_mut = Arc::get_mut(&mut channel).ok_or_else(|| {
        anyhow::anyhow!("internal error: could not get mutable channel reference")
    })?;
    channel_mut.authenticate().await?;

    eprintln!("Authentication successful for '{}'.", account.id);
    Ok(())
}

use std::sync::Arc;
