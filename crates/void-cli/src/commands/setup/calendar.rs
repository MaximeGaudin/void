use std::path::Path;

use void_core::config::{
    empty_settings, settings_set_opt_string, settings_set_string_list, settings_string,
    ConnectionConfig, VoidConfig,
};
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

    let cal_type = ConnectorType::from_static(void_calendar::CONNECTOR_ID);
    let gmail_type = ConnectorType::from_static(void_gmail::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == cal_type)
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

    let existing_custom_creds: Option<String> = cfg.connections.iter().find_map(|a| {
        if a.connector_type == gmail_type || a.connector_type == cal_type {
            settings_string(&a.settings, "credentials_file")
        } else {
            None
        }
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

    let mut settings = empty_settings();
    settings_set_opt_string(&mut settings, "credentials_file", custom_creds);
    settings_set_string_list(&mut settings, "calendar_ids", &calendar_ids);

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: cal_type,
        ignore_conversations: vec![],
        settings,
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
