use void_core::config::{
    empty_settings, settings_set_string_list, settings_set_u32, ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) fn setup_hackernews(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("📰  HACKER NEWS");
    eprintln!();
    eprintln!("Monitors Hacker News for stories matching your keywords.");
    eprintln!("Matching stories appear in your inbox (read-only, no auth needed).");

    let hn_type = ConnectorType::from_static(void_hackernews::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == hn_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Hacker News", &existing, cfg);
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
    eprintln!("Stories whose title contains any of these keywords will land in your inbox.");
    eprintln!("Leave empty to get all stories above the minimum score.");
    let kw_input = prompt("Keywords: ");
    let keywords: Vec<String> = kw_input
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    eprintln!();
    eprintln!("Minimum score for a story to appear in your inbox.");
    let min_score_input = prompt_default("Minimum score", "100");
    let min_score: u32 = min_score_input.parse().unwrap_or(100);

    let connection_id = prompt_default("\nAccount name", "hackernews");

    let mut settings = empty_settings();
    settings_set_string_list(&mut settings, "keywords", &keywords);
    settings_set_u32(&mut settings, "min_score", min_score);

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: hn_type,
        ignore_conversations: vec![],
        settings,
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Hacker News configured (no authentication needed).");
    Ok(())
}
