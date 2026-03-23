use std::path::Path;

use void_core::config::{self, VoidConfig};

pub(crate) fn show_configuration(config_path: &Path, cfg: &VoidConfig) {
    eprintln!("Config file: {}", config_path.display());
    eprintln!("Store path:  {}", cfg.store_path().display());
    eprintln!();

    eprintln!("[sync]");
    eprintln!(
        "  gmail_poll_interval_secs    = {}",
        cfg.sync.gmail_poll_interval_secs
    );
    eprintln!(
        "  calendar_poll_interval_secs = {}",
        cfg.sync.calendar_poll_interval_secs
    );
    eprintln!(
        "  hackernews_poll_interval_secs = {}",
        cfg.sync.hackernews_poll_interval_secs
    );
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("No connections configured.");
    } else {
        eprintln!("Connections ({}):", cfg.connections.len());
        for acc in &cfg.connections {
            eprintln!("  - {} ({})", acc.id, acc.connector_type);
            match &acc.settings {
                config::ConnectionSettings::Slack {
                    app_token,
                    user_token,
                    exclude_channels,
                } => {
                    eprintln!("    app_token:  {}", config::redact_token(app_token));
                    eprintln!("    user_token: {}", config::redact_token(user_token));
                    if !exclude_channels.is_empty() {
                        eprintln!("    exclude:    {}", exclude_channels.join(", "));
                    }
                }
                config::ConnectionSettings::Gmail { credentials_file } => {
                    let label = credentials_file.as_deref().unwrap_or("(built-in)");
                    eprintln!("    credentials: {label}");
                }
                config::ConnectionSettings::Calendar {
                    credentials_file,
                    calendar_ids,
                } => {
                    let label = credentials_file.as_deref().unwrap_or("(built-in)");
                    eprintln!("    credentials:  {label}");
                    eprintln!("    calendar_ids: {calendar_ids:?}");
                }
                config::ConnectionSettings::WhatsApp {} => {}
                config::ConnectionSettings::Telegram { api_id, api_hash } => {
                    if let Some(id) = api_id {
                        eprintln!("    api_id:   {id}");
                    }
                    if let Some(hash) = api_hash {
                        eprintln!("    api_hash: {}", config::redact_token(hash));
                    }
                    if api_id.is_none() && api_hash.is_none() {
                        eprintln!("    (using built-in API credentials)");
                    }
                }
                config::ConnectionSettings::HackerNews {
                    keywords,
                    min_score,
                } => {
                    if keywords.is_empty() {
                        eprintln!("    keywords:  (none — all stories)");
                    } else {
                        eprintln!("    keywords:  {}", keywords.join(", "));
                    }
                    eprintln!("    min_score: {min_score}");
                }
            }
        }
    }
}

pub(crate) fn edit_config_file(config_path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    let fallback_editor = "notepad";
    #[cfg(not(windows))]
    let fallback_editor = "vi";

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| fallback_editor.into());
    let status = std::process::Command::new(&editor)
        .arg(config_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("{editor} exited with status {status}");
    }
    Ok(())
}
