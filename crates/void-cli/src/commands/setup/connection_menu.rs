use std::path::Path;

use void_core::config::VoidConfig;

use super::auth::authenticate_connection;
use super::prompt::{prompt, select, separator};
use super::{calendar, gdrive, gmail, hackernews, slack, telegram, whatsapp};

pub(crate) async fn add_connection(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    let choice = select(
        "Which connector type?",
        &[
            "Gmail",
            "Slack",
            "WhatsApp",
            "Telegram",
            "Google Calendar",
            "Google Drive",
            "Hacker News",
        ],
    );

    separator();
    match choice {
        0 => gmail::setup_gmail(cfg, store_path, true).await?,
        1 => slack::setup_slack(cfg, store_path, true).await?,
        2 => whatsapp::setup_whatsapp(cfg, store_path, true).await?,
        3 => telegram::setup_telegram(cfg, store_path, true).await?,
        4 => calendar::setup_calendar(cfg, store_path, true).await?,
        5 => gdrive::setup_gdrive(cfg, store_path).await?,
        6 => hackernews::setup_hackernews(cfg, true)?,
        _ => {}
    }
    Ok(())
}

pub(crate) fn remove_connection(cfg: &mut VoidConfig) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to remove.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which connection would you like to remove?", &options_refs);
    cfg.connections.remove(choice);
    Ok(())
}

pub(crate) fn rename_connection(
    cfg: &mut VoidConfig,
    store_path: &std::path::Path,
) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to rename.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which connection would you like to rename?", &options_refs);
    let new_name = prompt("New connection name: ");
    if new_name.is_empty() {
        return Ok(());
    }

    let old_name = cfg.connections[choice].id.clone();
    let connector_type = &cfg.connections[choice].connector_type;

    // Rename token files (Gmail / Calendar)
    let old_token = store_path.join(format!("{old_name}-token.json"));
    let new_token = store_path.join(format!("{new_name}-token.json"));
    if old_token.exists() {
        std::fs::rename(&old_token, &new_token)?;
        eprintln!(
            "  Renamed token: {} → {}",
            old_token.display(),
            new_token.display()
        );
    }

    // Rename Drive token file if present
    let old_drive_token = store_path.join(format!("{old_name}-drive-token.json"));
    let new_drive_token = store_path.join(format!("{new_name}-drive-token.json"));
    if old_drive_token.exists() {
        std::fs::rename(&old_drive_token, &new_drive_token)?;
        eprintln!(
            "  Renamed drive token: {} → {}",
            old_drive_token.display(),
            new_drive_token.display()
        );
    }

    // Rename WhatsApp session DB
    if connector_type.to_string() == "whatsapp" {
        let old_wa = store_path.join(format!("whatsapp-{old_name}.db"));
        let new_wa = store_path.join(format!("whatsapp-{new_name}.db"));
        if old_wa.exists() {
            std::fs::rename(&old_wa, &new_wa)?;
            eprintln!(
                "  Renamed session: {} → {}",
                old_wa.display(),
                new_wa.display()
            );
        }
    }

    // Rename Telegram session file
    if connector_type.to_string() == "telegram" {
        let old_tg = store_path.join(format!("telegram-{old_name}.json"));
        let new_tg = store_path.join(format!("telegram-{new_name}.json"));
        if old_tg.exists() {
            std::fs::rename(&old_tg, &new_tg)?;
            eprintln!(
                "  Renamed session: {} → {}",
                old_tg.display(),
                new_tg.display()
            );
        }
    }

    // Update DB references (sync_state, conversations, messages)
    let db_path = cfg.db_path();
    if db_path.exists() {
        let db = void_core::db::Database::open(&db_path)?;
        db.rename_connection(&old_name, &new_name)?;
        eprintln!("  Updated database references.");
    }

    cfg.connections[choice].id = new_name;
    Ok(())
}

pub(crate) async fn reauthenticate_connection(
    cfg: &mut VoidConfig,
    config_path: &Path,
    store_path: &Path,
) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to re-authenticate.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select(
        "Which connection would you like to re-authenticate?",
        &options_refs,
    );

    reauthenticate_specific_connection(cfg, config_path, store_path, choice).await
}

pub(crate) async fn reauthenticate_specific_connection(
    cfg: &mut VoidConfig,
    config_path: &Path,
    store_path: &Path,
    choice: usize,
) -> anyhow::Result<()> {
    let connection = cfg.connections[choice].clone();

    if connection.connector_type == void_core::models::ConnectorType::Slack {
        eprintln!("  You need to provide your Slack tokens again.");
        eprintln!("  (Press Enter to keep the existing value)");
        let current_app_token = if let void_core::config::ConnectionSettings::Slack { ref app_token, .. } = connection.settings {
            app_token.clone()
        } else {
            String::new()
        };

        let current_user_token = if let void_core::config::ConnectionSettings::Slack { ref user_token, .. } = connection.settings {
            user_token.clone()
        } else {
            String::new()
        };

        let current_app_id = if let void_core::config::ConnectionSettings::Slack { ref app_id, .. } = connection.settings {
            app_id.clone().unwrap_or_default()
        } else {
            String::new()
        };

        let user_token = super::prompt::prompt_default("User OAuth Token (xoxp-...)", &current_user_token);
        let app_token = super::prompt::prompt_default("App-Level Token  (xapp-...)", &current_app_token);
        let app_id = super::prompt::prompt_default("App ID (optional, e.g. A012ABCD0A0)", &current_app_id);
        
        let refresh_token = prompt("Config Refresh Token (optional, xoxe-...): ");

        if let void_core::config::ConnectionSettings::Slack {
            app_token: ref mut at,
            user_token: ref mut ut,
            app_id: ref mut aid,
        } = cfg.connections[choice].settings
        {
            if !app_token.trim().is_empty() {
                *at = app_token.trim().to_string();
            }
            if !user_token.trim().is_empty() {
                *ut = user_token.trim().to_string();
            }
            *aid = if app_id.trim().is_empty() { None } else { Some(app_id.trim().to_string()) };
        }

        if !refresh_token.trim().is_empty() {
            let token_path = store_path.join(format!("slack-config-token-{}.json", connection.id));
            if let Err(e) = void_slack::manifest::save_refresh_token(&token_path, refresh_token.trim()) {
                eprintln!("  ✗ Failed to save refresh token to {}: {e}", token_path.display());
            } else {
                eprintln!("  ✓ Refresh token saved to {}", token_path.display());
            }
        }

        cfg.save(config_path)?;
        
        // Also verify the tokens
        let mut conn = crate::commands::connector_factory::build_connector(&cfg.connections[choice], store_path)?;
        if let Some(conn_mut) = std::sync::Arc::get_mut(&mut conn) {
            match conn_mut.authenticate().await {
                Ok(()) => eprintln!("  ✓ Re-authentication successful. Configuration saved."),
                Err(e) => eprintln!("  ✗ Verification failed: {e}. (Tokens saved anyway)"),
            }
        }
    } else {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Re-authentication successful."),
            Err(e) => eprintln!("  ✗ Re-authentication failed: {e}"),
        }
    }
    Ok(())
}
