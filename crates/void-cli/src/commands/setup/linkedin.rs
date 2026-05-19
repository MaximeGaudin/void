use std::path::Path;

use void_core::config::{ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{authenticate_connection, pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) async fn setup_linkedin(
    cfg: &mut VoidConfig,
    _store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("💼  LINKEDIN (via Unipile)");
    eprintln!();
    eprintln!("Syncs LinkedIn messages through the Unipile API.");
    eprintln!("You need a Unipile account with a connected LinkedIn profile.");
    eprintln!("Find your API key, DSN, and account ID in the Unipile dashboard:");
    eprintln!("  https://dashboard.unipile.com");
    eprintln!();

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::LinkedIn)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("LinkedIn", &existing, cfg);
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
    let api_key = prompt("Unipile API key: ");
    if api_key.trim().is_empty() {
        anyhow::bail!("API key is required.");
    }

    eprintln!();
    eprintln!("DSN is your Unipile API host (from the dashboard).");
    eprintln!("  Examples: api45.unipile.com:17560  or  https://api1.unipile.com:13111");
    let dsn = prompt_default("Unipile DSN", "api1.unipile.com:13111");

    eprintln!();
    eprintln!("Account ID is the Unipile id of your connected LinkedIn account.");
    let account_id = prompt("Unipile account ID: ");
    if account_id.trim().is_empty() {
        anyhow::bail!("Account ID is required.");
    }

    let connection_id = prompt_default("\nConnection name", "linkedin");

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: ConnectorType::LinkedIn,
        ignore_conversations: vec![],
        settings: ConnectionSettings::LinkedIn {
            api_key: api_key.trim().to_string(),
            dsn: dsn.trim().to_string(),
            account_id: account_id.trim().to_string(),
        },
    };

    eprintln!();
    match authenticate_connection(&connection, _store_path).await {
        Ok(()) => eprintln!("  ✓ LinkedIn (Unipile) verified."),
        Err(e) => eprintln!("  ✗ Verification failed: {e}"),
    }

    cfg.connections.push(connection);
    Ok(())
}
