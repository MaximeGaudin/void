use std::path::Path;

use void_core::config::{self, ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{authenticate_connection, pick_connector_action, ConnectorAction};
use super::prompt::{confirm, confirm_default_yes, prompt, prompt_default};

pub(crate) async fn setup_gmail(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📧  GMAIL");
    eprintln!();
    eprintln!("Connects your Gmail inbox. Void syncs your recent emails and");
    eprintln!("lets you search, read, reply, and archive from the CLI.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Gmail)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Gmail", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();
    eprintln!("Void includes built-in Google OAuth credentials.");
    eprintln!("You can use your own credentials file, or use the built-in ones.");
    eprintln!();

    let custom_creds = if confirm_default_yes("Use built-in credentials? (recommended)") {
        None
    } else {
        let path = prompt("Path to Google Cloud credentials JSON: ");
        if path.is_empty() {
            eprintln!("  Skipped (no path provided).");
            return Ok(());
        }
        let expanded = config::expand_tilde(&path);
        if !expanded.exists() {
            eprintln!("  Warning: file not found at {}", expanded.display());
            if !confirm("  Continue anyway?") {
                return Ok(());
            }
        }
        Some(path)
    };

    let connection_id = prompt_default("Connection name", "gmail");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Gmail,
        settings: ConnectionSettings::Gmail {
            credentials_file: custom_creds,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Gmail authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void setup");
    }

    cfg.connections.push(connection);
    Ok(())
}
