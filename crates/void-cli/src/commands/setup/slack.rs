use std::path::Path;

use void_core::config::{ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::auth::{authenticate_connection, pick_connector_action, ConnectorAction};
use super::prompt::{confirm_default_yes, confirm_typed, prompt, prompt_default, separator};

pub(crate) async fn setup_slack(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("💬  SLACK");
    eprintln!();
    eprintln!("Connects a Slack workspace. Void syncs your channels, DMs,");
    eprintln!("and lets you search and reply from the CLI.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Slack)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Slack", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    // ── Critical warning: User mode, not Bot mode ───────────────────────
    eprintln!();
    eprintln!("┌─────────────────────────────────────────────────────────┐");
    eprintln!("│                    ⚠️  IMPORTANT  ⚠️                     │");
    eprintln!("├─────────────────────────────────────────────────────────┤");
    eprintln!("│  ALL Slack app settings must be configured for USER     │");
    eprintln!("│  tokens, NOT bot tokens.                                │");
    eprintln!("│                                                         │");
    eprintln!("│  This means:                                            │");
    eprintln!("│  • Add scopes under \"User Token Scopes\"                 │");
    eprintln!("│    (NOT \"Bot Token Scopes\")                             │");
    eprintln!("│  • Subscribe to events under \"on behalf of users\"       │");
    eprintln!("│    (NOT \"bot events\")                                   │");
    eprintln!("│                                                         │");
    eprintln!("│  Why? Void acts as YOU — it reads and sends messages    │");
    eprintln!("│  with your identity. No need to invite a bot to every   │");
    eprintln!("│  channel. You get access to everything you can see.     │");
    eprintln!("└─────────────────────────────────────────────────────────┘");
    eprintln!();

    if !confirm_typed("Please confirm you understand the above.", "user not bot") {
        eprintln!("  Slack setup skipped.");
        return Ok(());
    }

    // ── Step 1: Create the Slack App ────────────────────────────────────
    separator();
    eprintln!("STEP 1 — Create a Slack App");
    eprintln!();
    eprintln!("  1. Go to https://api.slack.com/apps");
    eprintln!("  2. Click \"Create New App\" > \"From scratch\"");
    eprintln!("  3. Pick a name (e.g. \"Void\") and select your workspace");
    eprintln!();
    if !confirm_default_yes("Done? Continue to next step") {
        eprintln!("  Slack setup skipped.");
        return Ok(());
    }

    // ── Step 2: User Token Scopes ───────────────────────────────────────
    separator();
    eprintln!("STEP 2 — Add User Token Scopes");
    eprintln!();
    eprintln!("  Go to \"OAuth & Permissions\" in your app settings.");
    eprintln!("  Scroll down to \"User Token Scopes\" (NOT Bot Token Scopes!).");
    eprintln!("  Add ALL of the following scopes:");
    eprintln!();
    eprintln!("    channels:history    — View messages in public channels");
    eprintln!("    channels:read       — View basic channel info");
    eprintln!("    chat:write          — Send messages as you");
    eprintln!("    files:write         — Upload and share files");
    eprintln!("    groups:history      — View messages in private channels");
    eprintln!("    groups:read         — View basic info about private channels");
    eprintln!("    im:history          — View messages in DMs");
    eprintln!("    im:read             — View basic info about DMs");
    eprintln!("    mpim:history        — View messages in group DMs");
    eprintln!("    mpim:read           — View basic info about group DMs");
    eprintln!("    reactions:read      — View emoji reactions");
    eprintln!("    reactions:write     — Add emoji reactions");
    eprintln!("    users:read          — View people in the workspace");
    eprintln!();
    if !confirm_default_yes("Done? Continue to next step") {
        eprintln!("  Slack setup skipped.");
        return Ok(());
    }

    // ── Step 3: Enable Socket Mode ──────────────────────────────────────
    separator();
    eprintln!("STEP 3 — Enable Socket Mode");
    eprintln!();
    eprintln!("  Go to \"Socket Mode\" in the left sidebar.");
    eprintln!("  Toggle \"Enable Socket Mode\" ON.");
    eprintln!("  When prompted, create an app-level token:");
    eprintln!("    • Name it anything (e.g. \"void-socket\")");
    eprintln!("    • Add the scope: connections:write");
    eprintln!("    • Click \"Generate\"");
    eprintln!("  Save this token — it starts with xapp-");
    eprintln!();
    if !confirm_default_yes("Done? Continue to next step") {
        eprintln!("  Slack setup skipped.");
        return Ok(());
    }

    // ── Step 4: Event Subscriptions ─────────────────────────────────────
    separator();
    eprintln!("STEP 4 — Subscribe to Events (on behalf of users)");
    eprintln!();
    eprintln!("  Go to \"Event Subscriptions\" in the left sidebar.");
    eprintln!("  Toggle \"Enable Events\" ON.");
    eprintln!("  Expand \"Subscribe to events on behalf of users\"");
    eprintln!("  (NOT \"Subscribe to bot events\"!)");
    eprintln!("  Add these events:");
    eprintln!();
    eprintln!("    message.channels    — Messages in public channels");
    eprintln!("    message.groups      — Messages in private channels");
    eprintln!("    message.im          — Messages in DMs");
    eprintln!("    message.mpim        — Messages in group DMs");
    eprintln!();
    eprintln!("  Click \"Save Changes\" at the bottom.");
    eprintln!();
    if !confirm_default_yes("Done? Continue to next step") {
        eprintln!("  Slack setup skipped.");
        return Ok(());
    }

    // ── Step 5: Install & collect tokens ────────────────────────────────
    separator();
    eprintln!("STEP 5 — Install the App & Collect Tokens");
    eprintln!();
    eprintln!("  Go to \"Install App\" in the left sidebar and install to your workspace.");
    eprintln!("  (If already installed, click \"Reinstall to Workspace\" to apply scope changes.)");
    eprintln!();
    eprintln!("  You need two tokens:");
    eprintln!("  • User OAuth Token (xoxp-...)  →  found under \"OAuth & Permissions\"");
    eprintln!("  • App-Level Token   (xapp-...)  →  found under \"Basic Information\"");
    eprintln!("                                      > \"App-Level Tokens\"");
    eprintln!();

    let user_token = prompt("User OAuth Token (xoxp-...): ");
    if user_token.is_empty() {
        eprintln!("  Skipped (no token provided).");
        return Ok(());
    }

    let app_token = prompt("App-Level Token  (xapp-...): ");
    if app_token.is_empty() {
        eprintln!("  Skipped (no token provided).");
        return Ok(());
    }

    let connection_id = prompt_default("Connection name", "slack");

    let mut app_id: Option<String> = None;

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Slack,
        settings: ConnectionSettings::Slack {
            app_token,
            user_token,
            exclude_channels: vec![],
            app_id: None,
        },
    };

    if confirm_default_yes("Verify tokens now?") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Slack tokens verified successfully."),
            Err(e) => {
                eprintln!("  ✗ Verification failed: {e}");
                eprintln!("    Check your tokens and retry with: void setup");
            }
        }
    } else {
        eprintln!("  You can verify later with: void setup");
    }

    // ── Step 6 (optional): Auto-repair Event Subscriptions ───────────
    separator();
    eprintln!("STEP 6 (optional) — Auto-repair Event Subscriptions");
    eprintln!();
    eprintln!("  Slack may disable your event subscriptions if void is not");
    eprintln!("  running for a while. To let void auto-repair them on each");
    eprintln!("  sync, provide your App ID and a Config Refresh Token.");
    eprintln!();
    eprintln!("  1. Find your App ID in \"Basic Information\" > \"App Credentials\"");
    eprintln!("  2. Go to https://api.slack.com/apps");
    eprintln!("     Scroll down to \"Your App Configuration Tokens\"");
    eprintln!("     Click \"Generate Token\" for your workspace");
    eprintln!("     Copy the Refresh Token (starts with xoxe-)");
    eprintln!();

    if confirm_default_yes("Set up auto-repair?") {
        let input_app_id = prompt("App ID (e.g. A012ABCD0A0): ");
        if !input_app_id.is_empty() {
            let refresh_token = prompt("Config Refresh Token (xoxe-...): ");
            if !refresh_token.is_empty() {
                let token_path = store_path
                    .join(format!("slack-config-token-{connection_id}.json"));
                if let Err(e) =
                    void_slack::manifest::save_refresh_token(&token_path, &refresh_token)
                {
                    eprintln!("  ✗ Failed to save refresh token: {e}");
                } else {
                    app_id = Some(input_app_id);
                    eprintln!("  ✓ Auto-repair configured. Event subscriptions will be");
                    eprintln!("    checked and restored automatically on each `void sync`.");
                }
            } else {
                eprintln!("  Skipped (no refresh token provided).");
            }
        } else {
            eprintln!("  Skipped (no App ID provided).");
        }
    } else {
        eprintln!("  Skipped. You can configure this later in config.toml.");
    }

    let mut connection = connection;
    if let ConnectionSettings::Slack {
        app_id: ref mut cfg_app_id,
        ..
    } = connection.settings
    {
        *cfg_app_id = app_id;
    }

    cfg.connections.push(connection);
    Ok(())
}
