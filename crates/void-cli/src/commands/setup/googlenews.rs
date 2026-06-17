use void_core::config::{ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) fn setup_googlenews(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("📰  GOOGLE NEWS");
    eprintln!();
    eprintln!("Monitors Google News for articles matching your keywords.");
    eprintln!("Matching articles appear in your inbox (read-only, no auth needed).");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::GoogleNews)
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

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: ConnectorType::GoogleNews,
        ignore_conversations: vec![],
        settings: ConnectionSettings::GoogleNews {
            keywords,
            when,
            language,
            country,
        },
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Google News configured (no authentication needed).");
    Ok(())
}
