use void_core::config::{empty_settings, settings_set_string, ConnectionConfig, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::prompt_default;

pub(crate) fn setup_imessage(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("💬  iMESSAGE (macOS)");
    eprintln!();
    eprintln!("Indexes the local Messages history (iMessage and SMS) for search.");
    eprintln!("Can also send: void never writes to the Messages database directly,");
    eprintln!("but drives Messages.app via AppleScript and confirms sends against it.");

    if !cfg!(target_os = "macos") {
        eprintln!();
        eprintln!("This connector only works on macOS. Skipping.");
        return Ok(());
    }

    let im_type = ConnectorType::from_static(void_imessage::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == im_type)
            .map(|(i, _)| i)
            .collect();

        match pick_connector_action("iMessage", &existing, cfg) {
            ConnectorAction::Skip | ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();
    eprintln!("Reading the Messages database requires Full Disk Access.");
    eprintln!("System Settings > Privacy & Security > Full Disk Access, then add");
    eprintln!("the binary that runs void (your terminal, or the daemon's launcher).");
    eprintln!();
    eprintln!("Without it, void sees the file but macOS refuses the read, and");
    eprintln!("`void health` will say so explicitly.");

    let default_path = void_imessage::connector_default_db_path()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "~/Library/Messages/chat.db".to_string());
    let db_path = prompt_default("\nMessages database path", &default_path);

    // Report the permission state now rather than letting the first sync fail
    // in the background where nobody is watching.
    match std::fs::metadata(&db_path) {
        Ok(_) => eprintln!("\n✓ Found the Messages database."),
        Err(e) => eprintln!("\n⚠️  Could not stat {db_path}: {e}"),
    }

    let connection_id = prompt_default("\nAccount name", "imessage");

    let mut settings = empty_settings();
    settings_set_string(&mut settings, "db_path", &db_path);

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: im_type,
        ignore_conversations: Vec::new(),
        settings,
    };
    cfg.connections.push(connection);
    Ok(())
}
