use void_core::config::{ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) fn setup_hackernews(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("📰  HACKER NEWS");
    eprintln!();
    eprintln!("Monitors Hacker News for stories matching your keywords.");
    eprintln!("Matching stories appear in your inbox (read-only, no auth needed).");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::HackerNews)
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

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: ConnectorType::HackerNews,
        settings: ConnectionSettings::HackerNews {
            keywords,
            min_score,
        },
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Hacker News configured (no authentication needed).");
    Ok(())
}
