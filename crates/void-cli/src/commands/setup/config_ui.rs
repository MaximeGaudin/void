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
    eprintln!(
        "  googlenews_poll_interval_secs = {}",
        cfg.sync.googlenews_poll_interval_secs
    );
    eprintln!(
        "  reddit_poll_interval_secs     = {}",
        cfg.sync.reddit_poll_interval_secs
    );
    eprintln!(
        "  linkedin_poll_interval_secs   = {}",
        cfg.sync.linkedin_poll_interval_secs
    );
    eprintln!(
        "  linkedin_backfill_days        = {}",
        cfg.sync.linkedin_backfill_days
    );
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("No connections configured.");
    } else {
        eprintln!("Connections ({}):", cfg.connections.len());
        for acc in &cfg.connections {
            eprintln!("  - {} ({})", acc.id, acc.connector_type);
            if !acc.ignore_conversations.is_empty() {
                eprintln!("    ignore_conversations: {:?}", acc.ignore_conversations);
            }
            match &acc.settings {
                config::ConnectionSettings::Slack {
                    app_token,
                    user_token,
                    app_id,
                    config_refresh_token,
                } => {
                    eprintln!("    app_token:  {}", config::redact_token(app_token));
                    eprintln!("    user_token: {}", config::redact_token(user_token));
                    if let Some(id) = app_id {
                        eprintln!("    app_id:     {id}");
                    }
                    if let Some(token) = config_refresh_token {
                        eprintln!("    config_refresh_token: {}", config::redact_token(token));
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
                config::ConnectionSettings::GoogleNews {
                    keywords,
                    when,
                    language,
                    country,
                } => {
                    if keywords.is_empty() {
                        eprintln!("    keywords:  (none)");
                    } else {
                        eprintln!("    keywords:  {}", keywords.join(", "));
                    }
                    if when.is_empty() {
                        eprintln!("    when:      (no limit)");
                    } else {
                        eprintln!("    when:      {when}");
                    }
                    eprintln!("    language:  {language}");
                    eprintln!("    country:   {country}");
                }
                config::ConnectionSettings::LinkedIn {
                    api_key,
                    dsn,
                    account_id,
                } => {
                    eprintln!("    api_key:    {}", config::redact_token(api_key));
                    eprintln!("    dsn:        {dsn}");
                    eprintln!("    account_id: {account_id}");
                }
                config::ConnectionSettings::Reddit {
                    client_id,
                    client_secret,
                    refresh_token,
                    subreddits,
                    keywords,
                    min_score,
                } => {
                    eprintln!("    client_id:     {}", config::redact_token(client_id));
                    eprintln!("    client_secret: {}", config::redact_token(client_secret));
                    if refresh_token.is_some() {
                        eprintln!("    refresh_token: (set — commenting enabled)");
                    } else {
                        eprintln!("    refresh_token: (not set — read-only)");
                    }
                    if subreddits.is_empty() {
                        eprintln!("    subreddits:    (none)");
                    } else {
                        eprintln!("    subreddits:    {}", subreddits.join(", "));
                    }
                    if keywords.is_empty() {
                        eprintln!("    keywords:      (none — all posts)");
                    } else {
                        eprintln!("    keywords:      {}", keywords.join(", "));
                    }
                    eprintln!("    min_score:     {min_score}");
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
