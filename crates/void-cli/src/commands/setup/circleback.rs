use void_core::config::{
    empty_settings, settings_set_string, settings_set_u32, ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{confirm_default_yes, prompt, prompt_default};
use crate::connectors::circleback::DEFAULT_BACKFILL_DAYS;

pub(crate) async fn setup_circleback(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("🎙️  CIRCLEBACK");
    eprintln!();
    eprintln!("Syncs your Circleback meetings (read-only):");
    eprintln!("  • AI notes and action items of every meeting");
    eprintln!("  • The transcript, one message per speaker turn");
    eprintln!();
    eprintln!(
        "Create an API key at https://circleback.ai → Settings → API (keys start with `cb_`)."
    );

    let cb_type = ConnectorType::from_static(void_circleback::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == cb_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Circleback", &existing, cfg);
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
    let api_key = prompt("Circleback API key: ");
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        anyhow::bail!("Circleback API key is required");
    }

    let client = void_circleback::api::CirclebackClient::new(api_key.clone());
    let page = client.list_meetings(None).await?;
    eprintln!(
        "  ✓ Key valid ({} meetings on the first page)",
        page.meetings.len()
    );

    eprintln!();
    eprintln!("How far back to import on the first sync (0 = everything).");
    let backfill_days: u32 = prompt_default("Backfill days", &DEFAULT_BACKFILL_DAYS.to_string())
        .trim()
        .parse()
        .unwrap_or(DEFAULT_BACKFILL_DAYS);
    let include_transcript =
        confirm_default_yes("Import full transcripts (one message per speaker turn)?");

    let connection_id = prompt_default("\nAccount name", "circleback");

    let mut settings = empty_settings();
    settings_set_string(&mut settings, "api_key", &api_key);
    settings_set_u32(&mut settings, "backfill_days", backfill_days);
    settings.insert(
        "include_transcript".to_string(),
        toml::Value::Boolean(include_transcript),
    );

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: cb_type,
        ignore_conversations: vec![],
        settings,
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Circleback configured.");
    Ok(())
}
