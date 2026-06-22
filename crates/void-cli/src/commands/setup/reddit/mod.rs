mod oauth;

use void_core::config::{
    empty_settings, settings_set_opt_string, settings_set_string, settings_set_string_list,
    settings_set_u32, ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{confirm_default_yes, prompt, prompt_default};

pub(crate) async fn setup_reddit(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
    eprintln!("🔶  REDDIT");
    eprintln!();
    eprintln!("Monitors Reddit subreddits for posts matching your keywords.");
    eprintln!("Matching posts appear in your inbox. Enable commenting to sync");
    eprintln!("thread comments and reply from the CLI.");
    eprintln!();
    eprintln!("Create a Reddit \"web\" app at https://www.reddit.com/prefs/apps");
    eprintln!(
        "with redirect URI {oauth_uri}.",
        oauth_uri = oauth::REDIRECT_URI
    );

    let reddit_type = ConnectorType::from_static(void_reddit::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == reddit_type)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Reddit", &existing, cfg);
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
    let client_id = prompt("Reddit client ID: ");
    if client_id.trim().is_empty() {
        anyhow::bail!("Reddit client ID is required");
    }

    eprintln!();
    let client_secret = prompt("Reddit client secret: ");
    if client_secret.trim().is_empty() {
        anyhow::bail!("Reddit client secret is required");
    }

    eprintln!();
    eprintln!("Enter subreddits to watch (comma-separated, without r/ prefix).");
    eprintln!("Example: rust, programming, startups");
    let sub_input = prompt("Subreddits: ");
    let subreddits: Vec<String> = sub_input
        .split(',')
        .map(|s| s.trim().trim_start_matches("r/").to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if subreddits.is_empty() {
        anyhow::bail!("At least one subreddit is required");
    }

    eprintln!();
    eprintln!("Enter keywords to watch (comma-separated).");
    eprintln!("Posts whose title contains any of these keywords will land in your inbox.");
    eprintln!("Leave empty to get all posts above the minimum score.");
    let kw_input = prompt("Keywords: ");
    let keywords: Vec<String> = kw_input
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    eprintln!();
    eprintln!("Minimum score (upvotes) for a post to appear in your inbox.");
    let min_score_input = prompt_default("Minimum score", "50");
    let min_score: u32 = min_score_input.parse().unwrap_or(50);

    let refresh_token = if confirm_default_yes(
        "Enable commenting? (opens browser for Reddit authorization; stores refresh token)",
    ) {
        match oauth::obtain_refresh_token(&client_id, &client_secret).await {
            Ok(token) => {
                eprintln!("  ✓ Reddit commenting authorized.");
                Some(token)
            }
            Err(e) => {
                eprintln!("  ✗ Reddit authorization failed: {e}");
                eprintln!("    Continuing in read-only mode.");
                None
            }
        }
    } else {
        None
    };

    let connection_id = prompt_default("\nAccount name", "reddit");

    let mut settings = empty_settings();
    settings_set_string(&mut settings, "client_id", client_id.trim());
    settings_set_string(&mut settings, "client_secret", client_secret.trim());
    settings_set_opt_string(&mut settings, "refresh_token", refresh_token);
    settings_set_string_list(&mut settings, "subreddits", &subreddits);
    settings_set_string_list(&mut settings, "keywords", &keywords);
    settings_set_u32(&mut settings, "min_score", min_score);

    let connection = ConnectionConfig {
        id: connection_id,
        connector_type: reddit_type,
        ignore_conversations: vec![],
        settings,
    };

    cfg.connections.push(connection);
    eprintln!("  ✓ Reddit configured.");
    Ok(())
}
