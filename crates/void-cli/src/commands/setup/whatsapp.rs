use std::path::Path;

use void_core::config::{empty_settings, ConnectionConfig, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{authenticate_connection, pick_connector_action, ConnectorAction};
use super::prompt::{confirm_default_yes, prompt_default};

pub(crate) async fn setup_whatsapp(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📱  WHATSAPP");
    eprintln!();
    eprintln!("Connects WhatsApp via QR code (like WhatsApp Web).");
    eprintln!("No credentials or API keys needed.");

    let wa_type = ConnectorType::from_static(void_whatsapp::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == wa_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("WhatsApp", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    let connection_id = prompt_default("\nAccount name", "whatsapp");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: wa_type,
        ignore_conversations: vec![],
        settings: empty_settings(),
    };

    eprintln!();
    eprintln!("WhatsApp authentication requires scanning a QR code.");
    eprintln!("When you proceed, a QR code will appear in this terminal.");
    eprintln!("Open WhatsApp on your phone > Settings > Linked Devices > Link a Device,");
    eprintln!("then scan the code.");
    eprintln!();

    if confirm_default_yes("Pair now?") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ WhatsApp paired successfully."),
            Err(e) => {
                eprintln!("  ✗ Pairing failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can pair later with: void setup");
    }

    cfg.connections.push(connection);
    Ok(())
}
