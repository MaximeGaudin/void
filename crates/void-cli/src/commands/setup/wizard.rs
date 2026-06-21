use std::path::Path;

use void_core::config::VoidConfig;

use super::prompt::{confirm_default_yes, separator};
use super::{calendar, gmail, googlenews, hackernews, linkedin, reddit, slack, telegram, whatsapp};
use crate::commands::sync;

pub(crate) async fn run_full_wizard(
    cfg: &mut VoidConfig,
    store_path: &Path,
    config_path: &Path,
) -> anyhow::Result<()> {
    eprintln!();
    eprintln!("This wizard will guide you through connecting your");
    eprintln!("communication services (Gmail, Slack, WhatsApp, Telegram,");
    eprintln!("Google Calendar, Hacker News, Google News, LinkedIn, Reddit) to Void.");
    eprintln!();

    separator();
    gmail::setup_gmail(cfg, store_path, false).await?;
    separator();
    slack::setup_slack(cfg, store_path, false).await?;
    separator();
    whatsapp::setup_whatsapp(cfg, store_path, false).await?;
    separator();
    telegram::setup_telegram(cfg, store_path, false).await?;
    separator();
    calendar::setup_calendar(cfg, store_path, false).await?;
    separator();
    hackernews::setup_hackernews(cfg, false)?;
    separator();
    googlenews::setup_googlenews(cfg, false)?;
    separator();
    linkedin::setup_linkedin(cfg, store_path, false).await?;
    separator();
    reddit::setup_reddit(cfg, false).await?;
    separator();

    cfg.save(config_path)?;
    eprintln!("Configuration saved to {}", config_path.display());
    Ok(())
}

pub(crate) async fn exit_setup(cfg: &VoidConfig) -> anyhow::Result<()> {
    eprintln!("Setup complete.");

    if cfg.connections.is_empty() {
        eprintln!("No connectors configured. Run `void setup` again when ready.");
    } else {
        eprintln!();
        eprintln!("Configured connections:");
        for acc in &cfg.connections {
            eprintln!("  • {} ({})", acc.id, acc.connector_type);
        }
        eprintln!();
        if confirm_default_yes("Start syncing now? (`void sync --daemon`)") {
            eprintln!();
            let args = sync::SyncArgs {
                connectors: None,
                daemon: true,
                restart: false,
                clear: false,
                clear_connector: None,
                allow_broken: false,
                stop: false,
                status: false,
                daemon_inner: false,
            };
            sync::daemonize(&args, false)?;
        } else {
            eprintln!();
            eprintln!("You can start syncing later with: void sync --daemon");
        }
    }

    eprintln!();
    Ok(())
}
