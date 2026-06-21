use std::path::Path;

use void_core::config::{
    settings_set_opt_string, settings_set_string, settings_string, VoidConfig,
};

use crate::connectors::{self, SetupCtx};

use super::auth::authenticate_connection;
use super::prompt::{prompt, select, separator};

pub(crate) async fn add_connection(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    let plugins = connectors::all();
    let labels: Vec<&str> = plugins.iter().map(|p| p.menu_label).collect();
    let choice = select("Which connector type?", &labels);

    separator();
    let plugin = plugins[choice];
    let ctx = SetupCtx {
        cfg,
        store_path,
        add_only: true,
    };
    (plugin.setup)(ctx).await?;
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
    let connector_type = cfg.connections[choice].connector_type;

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

    if let Some(plugin) = connectors::by_id(connector_type.as_str()) {
        for old_path in (plugin.session_files)(store_path, &old_name) {
            if let Some(file_name) = old_path.file_name() {
                let new_path = old_path
                    .with_file_name(file_name.to_string_lossy().replace(&old_name, &new_name));
                if old_path.exists() {
                    std::fs::rename(&old_path, &new_path)?;
                    eprintln!(
                        "  Renamed session: {} → {}",
                        old_path.display(),
                        new_path.display()
                    );
                }
            }
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
    let plugin = connectors::by_id(connection.connector_type.as_str());

    if plugin.is_some_and(|p| p.prompt_token_reauth) {
        eprintln!("  You need to provide your Slack tokens again.");
        eprintln!("  (Press Enter to keep the existing value)");

        let current_app_token =
            settings_string(&connection.settings, "app_token").unwrap_or_default();
        let current_user_token =
            settings_string(&connection.settings, "user_token").unwrap_or_default();
        let current_app_id = settings_string(&connection.settings, "app_id").unwrap_or_default();
        let current_refresh_token =
            settings_string(&connection.settings, "config_refresh_token").unwrap_or_default();

        let user_token =
            super::prompt::prompt_default("User OAuth Token (xoxp-...)", &current_user_token);
        let app_token =
            super::prompt::prompt_default("App-Level Token  (xapp-...)", &current_app_token);
        let app_id =
            super::prompt::prompt_default("App ID (optional, e.g. A012ABCD0A0)", &current_app_id);
        let refresh_token = super::prompt::prompt_default(
            "Config Refresh Token (optional, xoxe-...)",
            &current_refresh_token,
        );

        if !app_token.trim().is_empty() {
            settings_set_string(
                &mut cfg.connections[choice].settings,
                "app_token",
                app_token.trim(),
            );
        }
        if !user_token.trim().is_empty() {
            settings_set_string(
                &mut cfg.connections[choice].settings,
                "user_token",
                user_token.trim(),
            );
        }
        settings_set_opt_string(
            &mut cfg.connections[choice].settings,
            "app_id",
            if app_id.trim().is_empty() {
                None
            } else {
                Some(app_id.trim().to_string())
            },
        );
        settings_set_opt_string(
            &mut cfg.connections[choice].settings,
            "config_refresh_token",
            if refresh_token.trim().is_empty() {
                None
            } else {
                Some(refresh_token.trim().to_string())
            },
        );

        cfg.save(config_path)?;

        let mut conn = crate::commands::connector_factory::build_connector(
            &cfg.connections[choice],
            store_path,
        )?;
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
