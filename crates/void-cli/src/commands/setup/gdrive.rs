use std::path::Path;

use void_core::config::{self, ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::prompt::{confirm, confirm_default_yes, select};

pub(crate) async fn setup_gdrive(cfg: &VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("📁  GOOGLE DRIVE");
    eprintln!();
    eprintln!("Enables downloading files from Google Drive, Docs, Sheets, and Slides.");
    eprintln!("This adds Drive read-only access to an existing Google connection.");

    let google_connections: Vec<(usize, &ConnectionConfig)> = cfg
        .connections
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.connector_type == ConnectorType::Gmail || a.connector_type == ConnectorType::Calendar
        })
        .collect();

    if google_connections.is_empty() {
        eprintln!();
        eprintln!("  No Google connections configured (Gmail or Calendar).");
        eprintln!("  Set up Gmail or Calendar first, then enable Drive access.");
        return Ok(());
    }

    if !confirm_default_yes("Enable Google Drive access?") {
        return Ok(());
    }

    let connection = if google_connections.len() == 1 {
        let (_, acc) = google_connections[0];
        eprintln!("  Using connection: {} ({})", acc.id, acc.connector_type);
        acc
    } else {
        let options: Vec<String> = google_connections
            .iter()
            .map(|(_, a)| format!("{} ({})", a.id, a.connector_type))
            .collect();
        let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
        let pick = select("Which Google connection should Drive use?", &options_refs);
        google_connections[pick].1
    };

    let drive_token = void_gdrive::api::drive_token_cache_path(store_path, &connection.id);
    if drive_token.exists() {
        eprintln!("  Drive is already authorized for \"{}\".", connection.id);
        if !confirm("  Re-authorize?") {
            return Ok(());
        }
    }

    let credentials_file = match &connection.settings {
        ConnectionSettings::Gmail { credentials_file } => credentials_file.clone(),
        ConnectionSettings::Calendar {
            credentials_file, ..
        } => credentials_file.clone(),
        _ => None,
    };
    let cred_path = credentials_file.as_ref().map(|f| config::expand_tilde(f));

    match void_gdrive::api::authenticate_drive(
        store_path,
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
    )
    .await
    {
        Ok(()) => eprintln!("  ✓ Google Drive authorized for \"{}\".", connection.id),
        Err(e) => {
            eprintln!("  ✗ Authorization failed: {e}");
            eprintln!(
                "    You can retry later with: void drive auth --connection {}",
                connection.id
            );
        }
    }
    Ok(())
}
