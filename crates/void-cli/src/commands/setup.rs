use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use void_core::config::{self, ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

use super::connector_factory;

// ── Prompt helpers ──────────────────────────────────────────────────────────

fn prompt(label: &str) -> String {
    eprint!("{label}");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap_or(0);
    line.trim().to_string()
}

fn prompt_default(label: &str, default: &str) -> String {
    eprint!("{label} [{default}]: ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).unwrap_or(0);
    let trimmed = line.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn confirm(label: &str) -> bool {
    let answer = prompt(&format!("{label} [y/N]: "));
    matches!(answer.to_lowercase().as_str(), "y" | "yes")
}

fn confirm_default_yes(label: &str) -> bool {
    let answer = prompt(&format!("{label} [Y/n]: "));
    !matches!(answer.to_lowercase().as_str(), "n" | "no")
}

fn select(label: &str, options: &[&str]) -> usize {
    eprintln!("\n{label}");
    for (i, opt) in options.iter().enumerate() {
        eprintln!("  {}) {opt}", i + 1);
    }
    loop {
        let answer = prompt("Choice: ");
        if let Ok(n) = answer.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return n - 1;
            }
        }
        eprintln!("  Please enter a number between 1 and {}", options.len());
    }
}

fn confirm_typed(label: &str, expected_phrase: &str) -> bool {
    eprintln!("{label}");
    loop {
        let answer = prompt(&format!("  Type \"{expected_phrase}\" to continue: "));
        if answer.eq_ignore_ascii_case(expected_phrase) {
            return true;
        }
        if answer.eq_ignore_ascii_case("skip") || answer.is_empty() {
            return false;
        }
        eprintln!("  Please type exactly \"{expected_phrase}\" (or \"skip\" to skip).");
    }
}

fn separator() {
    eprintln!("\n{}\n", "─".repeat(60));
}

// ── Main entry point ────────────────────────────────────────────────────────

pub async fn run() -> anyhow::Result<()> {
    let config_path = config::default_config_path();

    // If no config exists, create default and enter menu
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, config::default_config())?;
        eprintln!("Created default config at {}", config_path.display());
        eprintln!();
    }

    let mut cfg = VoidConfig::load_or_default(&config_path);
    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    loop {
        show_menu_header(&cfg);

        let options = if cfg.connections.is_empty() {
            vec![
                "Run full setup wizard",
                "Add a connection",
                "Show configuration",
                "Edit config file",
                "Done",
            ]
        } else {
            vec![
                "Add a connection",
                "Remove a connection",
                "Rename a connection",
                "Re-authenticate a connection",
                "Show configuration",
                "Edit config file",
                "Run full setup wizard",
                "Done",
            ]
        };

        if cfg.connections.is_empty() {
            eprintln!("No connections configured yet. Run the full setup wizard to get started.");
            eprintln!();
        }

        let choice = select("What would you like to do?", &options);

        let action_idx = if cfg.connections.is_empty() {
            match choice {
                0 => 7, // Wizard
                1 => 1, // Add
                2 => 5, // Show
                3 => 6, // Edit
                4 => 8, // Done
                _ => continue,
            }
        } else {
            choice + 1
        };

        match action_idx {
            1 => {
                add_connection(&mut cfg, &store_path).await?;
                cfg.save(&config_path)?;
                eprintln!("\nConfiguration saved.");
            }
            2 => {
                remove_connection(&mut cfg)?;
                cfg.save(&config_path)?;
                eprintln!("\nConnection removed. Configuration saved.");
            }
            3 => {
                rename_connection(&mut cfg, &store_path)?;
                cfg.save(&config_path)?;
                eprintln!("\nConnection renamed. Configuration saved.");
            }
            4 => {
                reauthenticate_connection(&cfg, &store_path).await?;
            }
            5 => {
                show_configuration(&config_path, &cfg);
            }
            6 => {
                edit_config_file(&config_path)?;
                // Reload config after edit
                cfg = VoidConfig::load_or_default(&config_path);
            }
            7 => {
                run_full_wizard(&mut cfg, &store_path, &config_path).await?;
                // Wizard saves and may prompt for sync; loop continues
            }
            8 => {
                return exit_setup(&cfg).await;
            }
            _ => {}
        }

        eprintln!();
    }
}

fn show_menu_header(cfg: &VoidConfig) {
    eprintln!("╔══════════════════════════════════════════════╗");
    eprintln!("║              Void — Setup                    ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("Current connections: (none)");
    } else {
        eprintln!("Current connections:");
        for acc in &cfg.connections {
            eprintln!("  • {} ({})", acc.id, acc.connector_type);
        }
    }
    eprintln!();
}

async fn add_connection(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    let choice = select(
        "Which connector type?",
        &[
            "Gmail",
            "Slack",
            "WhatsApp",
            "Telegram",
            "Google Calendar",
            "Google Drive",
            "Hacker News",
        ],
    );

    separator();
    match choice {
        0 => setup_gmail(cfg, store_path, true).await?,
        1 => setup_slack(cfg, store_path, true).await?,
        2 => setup_whatsapp(cfg, store_path, true).await?,
        3 => setup_telegram(cfg, store_path, true).await?,
        4 => setup_calendar(cfg, store_path, true).await?,
        5 => setup_gdrive(cfg, store_path).await?,
        6 => setup_hackernews(cfg, true)?,
        _ => {}
    }
    Ok(())
}

fn remove_connection(cfg: &mut VoidConfig) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to remove.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which connection would you like to remove?", &options_refs);
    cfg.connections.remove(choice);
    Ok(())
}

fn rename_connection(cfg: &mut VoidConfig, store_path: &std::path::Path) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to rename.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which connection would you like to rename?", &options_refs);
    let new_name = prompt("New connection name: ");
    if new_name.is_empty() {
        return Ok(());
    }

    let old_name = cfg.connections[choice].id.clone();
    let connector_type = &cfg.connections[choice].connector_type;

    // Rename token files (Gmail / Calendar)
    let old_token = store_path.join(format!("{old_name}-token.json"));
    let new_token = store_path.join(format!("{new_name}-token.json"));
    if old_token.exists() {
        std::fs::rename(&old_token, &new_token)?;
        eprintln!(
            "  Renamed token: {} → {}",
            old_token.display(),
            new_token.display()
        );
    }

    // Rename Drive token file if present
    let old_drive_token = store_path.join(format!("{old_name}-drive-token.json"));
    let new_drive_token = store_path.join(format!("{new_name}-drive-token.json"));
    if old_drive_token.exists() {
        std::fs::rename(&old_drive_token, &new_drive_token)?;
        eprintln!(
            "  Renamed drive token: {} → {}",
            old_drive_token.display(),
            new_drive_token.display()
        );
    }

    // Rename WhatsApp session DB
    if connector_type.to_string() == "whatsapp" {
        let old_wa = store_path.join(format!("whatsapp-{old_name}.db"));
        let new_wa = store_path.join(format!("whatsapp-{new_name}.db"));
        if old_wa.exists() {
            std::fs::rename(&old_wa, &new_wa)?;
            eprintln!(
                "  Renamed session: {} → {}",
                old_wa.display(),
                new_wa.display()
            );
        }
    }

    // Rename Telegram session file
    if connector_type.to_string() == "telegram" {
        let old_tg = store_path.join(format!("telegram-{old_name}.json"));
        let new_tg = store_path.join(format!("telegram-{new_name}.json"));
        if old_tg.exists() {
            std::fs::rename(&old_tg, &new_tg)?;
            eprintln!(
                "  Renamed session: {} → {}",
                old_tg.display(),
                new_tg.display()
            );
        }
    }

    // Update DB references (sync_state, conversations, messages)
    let db_path = cfg.db_path();
    if db_path.exists() {
        let db = void_core::db::Database::open(&db_path)?;
        db.rename_connection(&old_name, &new_name)?;
        eprintln!("  Updated database references.");
    }

    cfg.connections[choice].id = new_name;
    Ok(())
}

async fn reauthenticate_connection(cfg: &VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    if cfg.connections.is_empty() {
        eprintln!("No connections to re-authenticate.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .connections
        .iter()
        .map(|a| format!("{} ({})", a.id, a.connector_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select(
        "Which connection would you like to re-authenticate?",
        &options_refs,
    );
    let connection = &cfg.connections[choice];

    match authenticate_connection(connection, store_path).await {
        Ok(()) => eprintln!("  ✓ Re-authentication successful."),
        Err(e) => eprintln!("  ✗ Re-authentication failed: {e}"),
    }
    Ok(())
}

fn show_configuration(config_path: &Path, cfg: &VoidConfig) {
    eprintln!("Config file: {}", config_path.display());
    eprintln!("Store path:  {}", cfg.store_path().display());
    eprintln!();

    eprintln!("[sync]");
    eprintln!(
        "  gmail_poll_interval_secs    = {}",
        cfg.sync.gmail_poll_interval_secs
    );
    eprintln!(
        "  calendar_poll_interval_secs = {}",
        cfg.sync.calendar_poll_interval_secs
    );
    eprintln!(
        "  hackernews_poll_interval_secs = {}",
        cfg.sync.hackernews_poll_interval_secs
    );
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("No connections configured.");
    } else {
        eprintln!("Connections ({}):", cfg.connections.len());
        for acc in &cfg.connections {
            eprintln!("  - {} ({})", acc.id, acc.connector_type);
            match &acc.settings {
                config::ConnectionSettings::Slack {
                    app_token,
                    user_token,
                    exclude_channels,
                } => {
                    eprintln!("    app_token:  {}", config::redact_token(app_token));
                    eprintln!("    user_token: {}", config::redact_token(user_token));
                    if !exclude_channels.is_empty() {
                        eprintln!("    exclude:    {}", exclude_channels.join(", "));
                    }
                }
                config::ConnectionSettings::Gmail { credentials_file } => {
                    let label = credentials_file.as_deref().unwrap_or("(built-in)");
                    eprintln!("    credentials: {label}");
                }
                config::ConnectionSettings::Calendar {
                    credentials_file,
                    calendar_ids,
                } => {
                    let label = credentials_file.as_deref().unwrap_or("(built-in)");
                    eprintln!("    credentials:  {label}");
                    eprintln!("    calendar_ids: {calendar_ids:?}");
                }
                config::ConnectionSettings::WhatsApp {} => {}
                config::ConnectionSettings::Telegram { api_id, api_hash } => {
                    if let Some(id) = api_id {
                        eprintln!("    api_id:   {id}");
                    }
                    if let Some(hash) = api_hash {
                        eprintln!("    api_hash: {}", config::redact_token(hash));
                    }
                    if api_id.is_none() && api_hash.is_none() {
                        eprintln!("    (using built-in API credentials)");
                    }
                }
                config::ConnectionSettings::HackerNews {
                    keywords,
                    min_score,
                } => {
                    if keywords.is_empty() {
                        eprintln!("    keywords:  (none — all stories)");
                    } else {
                        eprintln!("    keywords:  {}", keywords.join(", "));
                    }
                    eprintln!("    min_score: {min_score}");
                }
            }
        }
    }
}

fn edit_config_file(config_path: &Path) -> anyhow::Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new(&editor)
        .arg(config_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("{editor} exited with status {status}");
    }
    Ok(())
}

async fn run_full_wizard(
    cfg: &mut VoidConfig,
    store_path: &Path,
    config_path: &Path,
) -> anyhow::Result<()> {
    eprintln!();
    eprintln!("This wizard will guide you through connecting your");
    eprintln!("communication services (Gmail, Slack, WhatsApp, Telegram,");
    eprintln!("Google Calendar, Google Drive, Hacker News) to Void.");
    eprintln!();

    separator();
    setup_gmail(cfg, store_path, false).await?;
    separator();
    setup_slack(cfg, store_path, false).await?;
    separator();
    setup_whatsapp(cfg, store_path, false).await?;
    separator();
    setup_telegram(cfg, store_path, false).await?;
    separator();
    setup_calendar(cfg, store_path, false).await?;
    separator();
    setup_gdrive(cfg, store_path).await?;
    separator();
    setup_hackernews(cfg, false)?;
    separator();

    cfg.save(config_path)?;
    eprintln!("Configuration saved to {}", config_path.display());
    Ok(())
}

async fn exit_setup(cfg: &VoidConfig) -> anyhow::Result<()> {
    eprintln!("Setup complete.");

    if cfg.connections.is_empty() {
        eprintln!("No connectors configured. Run `void setup` again when ready.");
    } else {
        eprintln!();
        eprintln!("Configured connections:");
        for acc in &cfg.connections {
            eprintln!("  • {} ({})", acc.id, acc.connector_type);
        }
        eprintln!();
        if confirm_default_yes("Start syncing now? (`void sync --daemon`)") {
            eprintln!();
            let args = super::sync::SyncArgs {
                connectors: None,
                daemon: true,
                restart: false,
                clear: false,
                clear_connector: None,
                stop: false,
            };
            super::sync::daemonize(&args, false)?;
        } else {
            eprintln!();
            eprintln!("You can start syncing later with: void sync --daemon");
        }
    }

    eprintln!();
    Ok(())
}

// ── Gmail ───────────────────────────────────────────────────────────────────

async fn setup_gmail(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📧  GMAIL");
    eprintln!();
    eprintln!("Connects your Gmail inbox. Void syncs your recent emails and");
    eprintln!("lets you search, read, reply, and archive from the CLI.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Gmail)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Gmail", &existing, cfg);
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
    eprintln!("Void includes built-in Google OAuth credentials.");
    eprintln!("You can use your own credentials file, or use the built-in ones.");
    eprintln!();

    let custom_creds = if confirm_default_yes("Use built-in credentials? (recommended)") {
        None
    } else {
        let path = prompt("Path to Google Cloud credentials JSON: ");
        if path.is_empty() {
            eprintln!("  Skipped (no path provided).");
            return Ok(());
        }
        let expanded = config::expand_tilde(&path);
        if !expanded.exists() {
            eprintln!("  Warning: file not found at {}", expanded.display());
            if !confirm("  Continue anyway?") {
                return Ok(());
            }
        }
        Some(path)
    };

    let connection_id = prompt_default("Connection name", "gmail");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Gmail,
        settings: ConnectionSettings::Gmail {
            credentials_file: custom_creds,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Gmail authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void setup");
    }

    cfg.connections.push(connection);
    Ok(())
}

// ── Slack ───────────────────────────────────────────────────────────────────

async fn setup_slack(
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

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Slack,
        settings: ConnectionSettings::Slack {
            app_token,
            user_token,
            exclude_channels: vec![],
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

    cfg.connections.push(connection);
    Ok(())
}

// ── WhatsApp ────────────────────────────────────────────────────────────────

async fn setup_whatsapp(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📱  WHATSAPP");
    eprintln!();
    eprintln!("Connects WhatsApp via QR code (like WhatsApp Web).");
    eprintln!("No credentials or API keys needed.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::WhatsApp)
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
        connector_type: ConnectorType::WhatsApp,
        settings: ConnectionSettings::WhatsApp {},
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

// ── Telegram ────────────────────────────────────────────────────────────────

async fn setup_telegram(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("✈️  TELEGRAM");
    eprintln!();
    eprintln!("Connects Telegram via QR code (like WhatsApp).");
    eprintln!("No credentials or API keys needed.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Telegram)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Telegram", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    let connection_id = prompt_default("\nAccount name", "telegram");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Telegram,
        settings: ConnectionSettings::Telegram {
            api_id: None,
            api_hash: None,
        },
    };

    eprintln!();
    eprintln!("Telegram authentication requires scanning a QR code.");
    eprintln!("When you proceed, a QR code will appear in this terminal.");
    eprintln!("Open Telegram on your phone > Settings > Devices > Link Desktop Device,");
    eprintln!("then scan the code.");
    eprintln!();

    if confirm_default_yes("Pair now?") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Telegram paired successfully."),
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

// ── Google Calendar ─────────────────────────────────────────────────────────

async fn setup_calendar(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("📅  GOOGLE CALENDAR");
    eprintln!();
    eprintln!("Syncs your Google Calendar events. Lets you view today's agenda,");
    eprintln!("this week's schedule, and upcoming events from the CLI.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == ConnectorType::Calendar)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Google Calendar", &existing, cfg);
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

    let existing_custom_creds: Option<String> =
        cfg.connections
            .iter()
            .find_map(|a| match (&a.connector_type, &a.settings) {
                (ConnectorType::Gmail, ConnectionSettings::Gmail { credentials_file }) => {
                    credentials_file.clone()
                }
                (
                    ConnectorType::Calendar,
                    ConnectionSettings::Calendar {
                        credentials_file, ..
                    },
                ) => credentials_file.clone(),
                _ => None,
            });

    let custom_creds = if let Some(ref existing_path) = existing_custom_creds {
        eprintln!("You have a custom credentials file configured: {existing_path}");
        eprintln!();
        if confirm_default_yes("Reuse this credentials file?") {
            Some(existing_path.clone())
        } else if confirm("Use built-in credentials instead?") {
            None
        } else {
            let path = prompt("Path to Google Cloud credentials JSON: ");
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        }
    } else {
        None
    };

    eprintln!();
    eprintln!("Which calendars should Void sync?");
    eprintln!("Enter calendar IDs separated by commas.");
    eprintln!("Use \"primary\" for your main calendar.");
    let cal_input = prompt_default("Calendar IDs", "primary");
    let calendar_ids: Vec<String> = cal_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let connection_id = prompt_default("Connection name", "calendar");

    let connection = ConnectionConfig {
        id: connection_id.clone(),
        connector_type: ConnectorType::Calendar,
        settings: ConnectionSettings::Calendar {
            credentials_file: custom_creds,
            calendar_ids,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_connection(&connection, store_path).await {
            Ok(()) => eprintln!("  ✓ Calendar authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void setup");
    }

    cfg.connections.push(connection);
    Ok(())
}

// ── Google Drive ────────────────────────────────────────────────────────────

async fn setup_gdrive(cfg: &VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("📁  GOOGLE DRIVE");
    eprintln!();
    eprintln!("Enables downloading files from Google Drive, Docs, Sheets, and Slides.");
    eprintln!("This adds Drive read-only access to an existing Google connection.");

    let google_connections: Vec<(usize, &ConnectionConfig)> = cfg
        .connections
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            a.connector_type == ConnectorType::Gmail || a.connector_type == ConnectorType::Calendar
        })
        .collect();

    if google_connections.is_empty() {
        eprintln!();
        eprintln!("  No Google connections configured (Gmail or Calendar).");
        eprintln!("  Set up Gmail or Calendar first, then enable Drive access.");
        return Ok(());
    }

    if !confirm_default_yes("Enable Google Drive access?") {
        return Ok(());
    }

    let connection = if google_connections.len() == 1 {
        let (_, acc) = google_connections[0];
        eprintln!("  Using connection: {} ({})", acc.id, acc.connector_type);
        acc
    } else {
        let options: Vec<String> = google_connections
            .iter()
            .map(|(_, a)| format!("{} ({})", a.id, a.connector_type))
            .collect();
        let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
        let pick = select("Which Google connection should Drive use?", &options_refs);
        google_connections[pick].1
    };

    let drive_token = void_gdrive::api::drive_token_cache_path(store_path, &connection.id);
    if drive_token.exists() {
        eprintln!("  Drive is already authorized for \"{}\".", connection.id);
        if !confirm("  Re-authorize?") {
            return Ok(());
        }
    }

    let credentials_file = match &connection.settings {
        ConnectionSettings::Gmail { credentials_file } => credentials_file.clone(),
        ConnectionSettings::Calendar {
            credentials_file, ..
        } => credentials_file.clone(),
        _ => None,
    };
    let cred_path = credentials_file.as_ref().map(|f| config::expand_tilde(f));

    match void_gdrive::api::authenticate_drive(
        store_path,
        &connection.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
    )
    .await
    {
        Ok(()) => eprintln!("  ✓ Google Drive authorized for \"{}\".", connection.id),
        Err(e) => {
            eprintln!("  ✗ Authorization failed: {e}");
            eprintln!(
                "    You can retry later with: void drive auth --connection {}",
                connection.id
            );
        }
    }
    Ok(())
}

// ── Hacker News ─────────────────────────────────────────────────────────────

fn setup_hackernews(cfg: &mut VoidConfig, add_only: bool) -> anyhow::Result<()> {
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

// ── Shared helpers ──────────────────────────────────────────────────────────

enum ConnectorAction {
    Skip,
    Keep,
    Replace(usize),
    Add,
}

fn pick_connector_action(
    name: &str,
    existing_indices: &[usize],
    cfg: &VoidConfig,
) -> ConnectorAction {
    if existing_indices.is_empty() {
        if confirm_default_yes(&format!("Set up {name}?")) {
            ConnectorAction::Add
        } else {
            ConnectorAction::Skip
        }
    } else if existing_indices.len() == 1 {
        let idx = existing_indices[0];
        let acc = &cfg.connections[idx];
        eprintln!();
        eprintln!("  Existing connection: {} ({})", acc.id, acc.connector_type);
        let choice = select(
            &format!("You already have a {name} connection configured:"),
            &[
                "Keep the current configuration",
                "Replace with new configuration",
                "Add another connection",
                "Skip",
            ],
        );
        match choice {
            0 => ConnectorAction::Keep,
            1 => ConnectorAction::Replace(idx),
            2 => ConnectorAction::Add,
            _ => ConnectorAction::Skip,
        }
    } else {
        eprintln!();
        eprintln!("  Existing connections:");
        for &idx in existing_indices {
            eprintln!(
                "    • {} ({})",
                cfg.connections[idx].id, cfg.connections[idx].connector_type
            );
        }
        let choice = select(
            &format!(
                "You have {} {name} connections configured:",
                existing_indices.len()
            ),
            &["Keep all current connections", "Add another connection", "Skip"],
        );
        match choice {
            0 => ConnectorAction::Keep,
            1 => ConnectorAction::Add,
            _ => ConnectorAction::Skip,
        }
    }
}

async fn authenticate_connection(connection: &ConnectionConfig, store_path: &Path) -> anyhow::Result<()> {
    let mut conn = connector_factory::build_connector(connection, store_path)?;
    let conn_mut = Arc::get_mut(&mut conn)
        .ok_or_else(|| anyhow::anyhow!("internal error: could not get mutable connector ref"))?;
    conn_mut.authenticate().await
}
