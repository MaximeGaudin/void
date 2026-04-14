use std::path::Path;

use void_core::config::{ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{authenticate_connection, pick_connector_action, ConnectorAction};
use super::prompt::{confirm, confirm_default_yes, prompt, prompt_default};

pub(crate) async fn setup_calendar(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📅  GOOGLE CALENDAR");
    eprintln!();
    eprintln!("Syncs your Google Calendar events. Lets you view today's agenda,");
    eprintln!("this week's schedule, and upcoming events from the CLI.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Calendar)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Google Calendar", &existing, cfg);
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

    let existing_custom_creds: Option<String> =
        cfg.connections
            .iter()
            .find_map(|a| match (&a.connector_type, &a.settings) {
                (ConnectorType::Gmail, ConnectionSettings::Gmail { credentials_file }) => {
                    credentials_file.clone()
                }
                (
                    ConnectorType::Calendar,
                    ConnectionSettings::Calendar {
                        credentials_file, ..
                    },
                ) => credentials_file.clone(),
                _ => None,
            });

    let custom_creds = if let Some(ref existing_path) = existing_custom_creds {
        eprintln!("You have a custom credentials file configured: {existing_path}");
        eprintln!();
        if confirm_default_yes("Reuse this credentials file?") {
            Some(existing_path.clone())
        } else if confirm("Use built-in credentials instead?") {
            None
        } else {
            let path = prompt("Path to Google Cloud credentials JSON: ");
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        }
    } else {
        None
    };

    eprintln!();
    eprintln!("Which calendars should Void sync?");
    eprintln!("Enter calendar IDs separated by commas.");
    eprintln!("Use \"primary\" for your main calendar.");
    let cal_input = prompt_default("Calendar IDs", "primary");
    let calendar_ids: Vec<String> = cal_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let connection_id = prompt_default("Connection name", "calendar");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Calendar,
        ignore_conversations: vec![],
        settings: ConnectionSettings::Calendar {
            credentials_file: custom_creds,
            calendar_ids,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Calendar authenticated successfully."),
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
