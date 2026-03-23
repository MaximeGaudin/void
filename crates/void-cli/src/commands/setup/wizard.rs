use std::path::Path;

use void_core::config::VoidConfig;

use super::prompt::{confirm_default_yes, separator};
use super::{calendar, gdrive, gmail, hackernews, slack, telegram, whatsapp};
use crate::commands::sync;

pub(crate) async fn run_full_wizard(
    cfg: &mut VoidConfig,
    store_path: &Path,
    config_path: &Path,
) -> anyhow::Result<()> {
    eprintln!();
    eprintln!("This wizard will guide you through connecting your");
    eprintln!("communication services (Gmail, Slack, WhatsApp, Telegram,");
    eprintln!("Google Calendar, Google Drive, Hacker News) to Void.");
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
    gdrive::setup_gdrive(cfg, store_path).await?;
    separator();
    hackernews::setup_hackernews(cfg, false)?;
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
                stop: false,
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
