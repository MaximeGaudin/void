use void_core::config::{
    empty_settings, settings_set_string, settings_set_string_list, ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) fn setup_googlenews(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("📰  GOOGLE NEWS");
    eprintln!();
    eprintln!("Monitors Google News for articles matching your keywords.");
    eprintln!("Matching articles appear in your inbox (read-only, no auth needed).");

    let gn_type = ConnectorType::from_static(void_googlenews::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == gn_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Google News", &existing, cfg);
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
    eprintln!("Enter keywords to watch (comma-separated).");
    eprintln!(
        "Each keyword triggers its own Google News search; matching articles land in your inbox."
    );
    let kw_input = prompt("Keywords: ");
    let keywords: Vec<String> = kw_input
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    eprintln!();
    eprintln!("Recency window — only ingest articles published within this window.");
    eprintln!("Examples: 24h, 7d. Leave empty for no limit.");
    let when = prompt_default("Recency", "7d").trim().to_lowercase();

    eprintln!();
    eprintln!("Edition — UI language (hl) and country (gl), e.g. fr/FR or en/US.");
    let language = prompt_default("Language", "fr").trim().to_lowercase();
    let country = prompt_default("Country", "FR").trim().to_uppercase();

    let connection_id = prompt_default("\nAccount name", "googlenews");

    let mut settings = empty_settings();
    settings_set_string_list(&mut settings, "keywords", &keywords);
    settings_set_string(&mut settings, "when", &when);
    settings_set_string(&mut settings, "language", &language);
    settings_set_string(&mut settings, "country", &country);

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: gn_type,
        ignore_conversations: vec![],
        settings,
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Google News configured (no authentication needed).");
    Ok(())
}
