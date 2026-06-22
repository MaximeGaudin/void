use void_core::config::{empty_settings, settings_set_string, ConnectionConfig, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) async fn setup_github(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("🐙  GITHUB");
    eprintln!();
    eprintln!("Syncs GitHub activity into your inbox:");
    eprintln!("  • Open PRs requesting your review");
    eprintln!("  • Comments on your pull requests");
    eprintln!("  • @mentions of your handle");
    eprintln!();
    eprintln!("Create a Personal Access Token with at least the `notifications` scope.");
    eprintln!("For private repositories, also grant `repo` (classic) or Pull requests read access (fine-grained).");

    let gh_type = ConnectorType::from_static(void_github::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == gh_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("GitHub", &existing, cfg);
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
    let token = prompt("GitHub Personal Access Token: ");
    if token.trim().is_empty() {
        anyhow::bail!("GitHub token is required");
    }

    let client = void_github::api::GitHubClient::new(token.trim());
    let user = client.current_user().await?;
    eprintln!("  ✓ Token valid for @{}", user.login);

    let username = prompt_default("GitHub username", &user.login);

    let connection_id = prompt_default("\nAccount name", "github");

    let mut settings = empty_settings();
    settings_set_string(&mut settings, "token", token.trim());
    settings_set_string(&mut settings, "username", &username);

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: gh_type,
        ignore_conversations: vec![],
        settings,
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ GitHub configured.");
    eprintln!("  Mute repositories with: void mute <owner/repo>");
    Ok(())
}
