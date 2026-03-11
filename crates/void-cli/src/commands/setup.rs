use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use void_core::config::{self, AccountConfig, AccountSettings, AccountType, VoidConfig};

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

        let options = if cfg.accounts.is_empty() {
            vec![
                "Run full setup wizard",
                "Add a connector account",
                "Show configuration",
                "Edit config file",
                "Done",
            ]
        } else {
            vec![
                "Add a connector account",
                "Remove an account",
                "Rename an account",
                "Re-authenticate an account",
                "Show configuration",
                "Edit config file",
                "Run full setup wizard",
                "Done",
            ]
        };

        if cfg.accounts.is_empty() {
            eprintln!("No accounts configured yet. Run the full setup wizard to get started.");
            eprintln!();
        }

        let choice = select("What would you like to do?", &options);

        let action_idx = if cfg.accounts.is_empty() {
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
                add_connector_account(&mut cfg, &store_path).await?;
                cfg.save(&config_path)?;
                eprintln!("\nConfiguration saved.");
            }
            2 => {
                remove_account(&mut cfg)?;
                cfg.save(&config_path)?;
                eprintln!("\nAccount removed. Configuration saved.");
            }
            3 => {
                rename_account(&mut cfg, &store_path)?;
                cfg.save(&config_path)?;
                eprintln!("\nAccount renamed. Configuration saved.");
            }
            4 => {
                reauthenticate_account(&cfg, &store_path).await?;
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

    if cfg.accounts.is_empty() {
        eprintln!("Current accounts: (none)");
    } else {
        eprintln!("Current accounts:");
        for acc in &cfg.accounts {
            eprintln!("  • {} ({})", acc.id, acc.account_type);
        }
    }
    eprintln!();
}

async fn add_connector_account(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    let choice = select(
        "Which connector type?",
        &["Gmail", "Slack", "WhatsApp", "Google Calendar"],
    );

    separator();
    match choice {
        0 => setup_gmail(cfg, store_path, true).await?,
        1 => setup_slack(cfg, store_path, true).await?,
        2 => setup_whatsapp(cfg, store_path, true).await?,
        3 => setup_calendar(cfg, store_path, true).await?,
        _ => {}
    }
    Ok(())
}

fn remove_account(cfg: &mut VoidConfig) -> anyhow::Result<()> {
    if cfg.accounts.is_empty() {
        eprintln!("No accounts to remove.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .accounts
        .iter()
        .map(|a| format!("{} ({})", a.id, a.account_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which account would you like to remove?", &options_refs);
    cfg.accounts.remove(choice);
    Ok(())
}

fn rename_account(cfg: &mut VoidConfig, store_path: &std::path::Path) -> anyhow::Result<()> {
    if cfg.accounts.is_empty() {
        eprintln!("No accounts to rename.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .accounts
        .iter()
        .map(|a| format!("{} ({})", a.id, a.account_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select("Which account would you like to rename?", &options_refs);
    let new_name = prompt("New account name: ");
    if new_name.is_empty() {
        return Ok(());
    }

    let old_name = cfg.accounts[choice].id.clone();
    let account_type = &cfg.accounts[choice].account_type;

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

    // Rename WhatsApp session DB
    if account_type.to_string() == "whatsapp" {
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

    // Update DB references (sync_state, conversations, messages)
    let db_path = cfg.db_path();
    if db_path.exists() {
        let db = void_core::db::Database::open(&db_path)?;
        db.rename_account(&old_name, &new_name)?;
        eprintln!("  Updated database references.");
    }

    cfg.accounts[choice].id = new_name;
    Ok(())
}

async fn reauthenticate_account(cfg: &VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    if cfg.accounts.is_empty() {
        eprintln!("No accounts to re-authenticate.");
        return Ok(());
    }

    let options: Vec<String> = cfg
        .accounts
        .iter()
        .map(|a| format!("{} ({})", a.id, a.account_type))
        .collect();
    let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

    let choice = select(
        "Which account would you like to re-authenticate?",
        &options_refs,
    );
    let account = &cfg.accounts[choice];

    match authenticate_account(account, store_path).await {
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
    eprintln!();

    if cfg.accounts.is_empty() {
        eprintln!("No accounts configured.");
    } else {
        eprintln!("Accounts ({}):", cfg.accounts.len());
        for acc in &cfg.accounts {
            eprintln!("  - {} ({})", acc.id, acc.account_type);
            match &acc.settings {
                config::AccountSettings::Slack {
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
                config::AccountSettings::Gmail { credentials_file } => {
                    eprintln!("    credentials: {credentials_file}");
                }
                config::AccountSettings::Calendar {
                    credentials_file,
                    calendar_ids,
                } => {
                    eprintln!("    credentials:  {credentials_file}");
                    eprintln!("    calendar_ids: {calendar_ids:?}");
                }
                config::AccountSettings::WhatsApp {} => {}
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
    eprintln!("communication services (Gmail, Slack, WhatsApp,");
    eprintln!("Google Calendar) to Void.");
    eprintln!();

    separator();
    setup_gmail(cfg, store_path, false).await?;
    separator();
    setup_slack(cfg, store_path, false).await?;
    separator();
    setup_whatsapp(cfg, store_path, false).await?;
    separator();
    setup_calendar(cfg, store_path, false).await?;
    separator();

    cfg.save(config_path)?;
    eprintln!("Configuration saved to {}", config_path.display());
    Ok(())
}

async fn exit_setup(cfg: &VoidConfig) -> anyhow::Result<()> {
    eprintln!("Setup complete.");

    if cfg.accounts.is_empty() {
        eprintln!("No connectors configured. Run `void setup` again when ready.");
    } else {
        eprintln!();
        eprintln!("Configured accounts:");
        for acc in &cfg.accounts {
            eprintln!("  • {} ({})", acc.id, acc.account_type);
        }
        eprintln!();
        if confirm_default_yes("Start syncing now? (`void sync --daemon`)") {
            eprintln!();
            let args = super::sync::SyncArgs {
                connectors: None,
                daemon: true,
                restart: false,
                clear: false,
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
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.account_type == AccountType::Gmail)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Gmail", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.accounts.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();
    eprintln!("To connect Gmail, you need a Google Cloud OAuth credentials file.");
    eprintln!("This is the same file used for all Google services (Gmail, Calendar).");
    eprintln!("If you don't have one yet, follow these steps:");
    eprintln!();
    eprintln!("  1. Go to https://console.cloud.google.com/apis/credentials");
    eprintln!("  2. Create a project (or select an existing one)");
    eprintln!("  3. Enable the \"Gmail API\" (APIs & Services > Library)");
    eprintln!("  4. Go to \"OAuth consent screen\" and configure it:");
    eprintln!("     - User type: External (or Internal if using Workspace)");
    eprintln!("     - Add your email as a test user");
    eprintln!("  5. Go to Credentials > Create Credentials > OAuth client ID");
    eprintln!("     - Application type: Desktop app");
    eprintln!("     - Download the JSON file");
    eprintln!("  6. Save it somewhere safe, e.g. ~/.config/void/google-credentials.json");
    eprintln!();

    let creds = prompt("Path to Google Cloud credentials JSON: ");
    if creds.is_empty() {
        eprintln!("  Skipped (no path provided).");
        return Ok(());
    }

    let expanded = config::expand_tilde(&creds);
    if !expanded.exists() {
        eprintln!("  Warning: file not found at {}", expanded.display());
        if !confirm("  Continue anyway?") {
            return Ok(());
        }
    }

    let account_id = prompt_default("Account name", "gmail");

    let account = AccountConfig {
        id: account_id.clone(),
        account_type: AccountType::Gmail,
        settings: AccountSettings::Gmail {
            credentials_file: creds,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Gmail authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void setup");
    }

    cfg.accounts.push(account);
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
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.account_type == AccountType::Slack)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Slack", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.accounts.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();
    eprintln!("To connect Slack, you need to create a Slack App and");
    eprintln!("generate two tokens. Here's how:");
    eprintln!();
    eprintln!("  1. Go to https://api.slack.com/apps and click \"Create New App\"");
    eprintln!("  2. Choose \"From scratch\", pick a name and your workspace");
    eprintln!("  3. Under \"OAuth & Permissions\", add these User Token Scopes:");
    eprintln!("     channels:history, channels:read, chat:write,");
    eprintln!("     groups:history, groups:read, im:history, im:read,");
    eprintln!("     mpim:history, mpim:read, users:read, reactions:read");
    eprintln!("  4. Install the app to your workspace");
    eprintln!("  5. Copy the \"User OAuth Token\" (starts with xoxp-)");
    eprintln!("  6. Under \"Basic Information\" > \"App-Level Tokens\":");
    eprintln!("     Create a token with connections:write scope");
    eprintln!("     (starts with xapp-)");
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

    let account_id = prompt_default("Account name", "slack");

    let account = AccountConfig {
        id: account_id.clone(),
        account_type: AccountType::Slack,
        settings: AccountSettings::Slack {
            app_token,
            user_token,
            exclude_channels: vec![],
        },
    };

    if confirm_default_yes("Verify tokens now?") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Slack tokens verified successfully."),
            Err(e) => {
                eprintln!("  ✗ Verification failed: {e}");
                eprintln!("    Check your tokens and retry with: void setup");
            }
        }
    } else {
        eprintln!("  You can verify later with: void setup");
    }

    cfg.accounts.push(account);
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
    eprintln!("Connects your WhatsApp account via QR code (like WhatsApp Web).");
    eprintln!("No credentials or API keys needed.");

    if !add_only {
        let existing: Vec<usize> = cfg
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.account_type == AccountType::WhatsApp)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("WhatsApp", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.accounts.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    let account_id = prompt_default("\nAccount name", "whatsapp");

    let account = AccountConfig {
        id: account_id.clone(),
        account_type: AccountType::WhatsApp,
        settings: AccountSettings::WhatsApp {},
    };

    eprintln!();
    eprintln!("WhatsApp authentication requires scanning a QR code.");
    eprintln!("When you proceed, a QR code will appear in this terminal.");
    eprintln!("Open WhatsApp on your phone > Settings > Linked Devices > Link a Device,");
    eprintln!("then scan the code.");
    eprintln!();

    if confirm_default_yes("Pair now?") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ WhatsApp paired successfully."),
            Err(e) => {
                eprintln!("  ✗ Pairing failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can pair later with: void setup");
    }

    cfg.accounts.push(account);
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
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, a)| a.account_type == AccountType::Calendar)
            .map(|(i, _)| i)
            .collect();

        let action = pick_connector_action("Google Calendar", &existing, cfg);
        match action {
            ConnectorAction::Skip => return Ok(()),
            ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.accounts.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();

    let google_creds = cfg
        .accounts
        .iter()
        .find_map(|a| match (&a.account_type, &a.settings) {
            (AccountType::Gmail, AccountSettings::Gmail { credentials_file }) => {
                Some(credentials_file.clone())
            }
            (
                AccountType::Calendar,
                AccountSettings::Calendar {
                    credentials_file, ..
                },
            ) => Some(credentials_file.clone()),
            _ => None,
        });

    let creds = if let Some(ref existing_path) = google_creds {
        eprintln!("You have a Google Cloud credentials file configured: {existing_path}");
        eprintln!("Google Calendar uses the same credentials file as Gmail.");
        eprintln!("(Make sure the \"Google Calendar API\" is enabled in the same");
        eprintln!(" Google Cloud project.)");
        eprintln!();

        if confirm_default_yes("Reuse this credentials file?") {
            existing_path.clone()
        } else {
            let path = prompt("Path to Google Cloud credentials JSON: ");
            if path.is_empty() {
                eprintln!("  Skipped (no path provided).");
                return Ok(());
            }
            path
        }
    } else {
        eprintln!("To connect Google Calendar, you need a Google Cloud OAuth");
        eprintln!("credentials file (the same file used for Gmail):");
        eprintln!();
        eprintln!("  1. Go to https://console.cloud.google.com/apis/credentials");
        eprintln!("  2. Create a project (or select an existing one)");
        eprintln!("  3. Enable the \"Google Calendar API\" (APIs & Services > Library)");
        eprintln!("  4. Go to \"OAuth consent screen\" and configure it");
        eprintln!("  5. Go to Credentials > Create Credentials > OAuth client ID");
        eprintln!("     - Application type: Desktop app");
        eprintln!("     - Download the JSON file");
        eprintln!();

        let path = prompt("Path to Google Cloud credentials JSON: ");
        if path.is_empty() {
            eprintln!("  Skipped (no path provided).");
            return Ok(());
        }
        path
    };

    let expanded = config::expand_tilde(&creds);
    if !expanded.exists() {
        eprintln!("  Warning: file not found at {}", expanded.display());
        if !confirm("  Continue anyway?") {
            return Ok(());
        }
    }

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

    let account_id = prompt_default("Account name", "calendar");

    let account = AccountConfig {
        id: account_id.clone(),
        account_type: AccountType::Calendar,
        settings: AccountSettings::Calendar {
            credentials_file: creds,
            calendar_ids,
        },
    };

    if confirm_default_yes("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Calendar authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void setup");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void setup");
    }

    cfg.accounts.push(account);
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
        let choice = select(
            &format!("Would you like to set up {name}?"),
            &["Yes, set it up", "Skip for now"],
        );
        if choice == 0 {
            ConnectorAction::Add
        } else {
            ConnectorAction::Skip
        }
    } else if existing_indices.len() == 1 {
        let idx = existing_indices[0];
        let acc = &cfg.accounts[idx];
        eprintln!();
        eprintln!("  Existing account: {} ({})", acc.id, acc.account_type);
        let choice = select(
            &format!("You already have a {name} account configured:"),
            &[
                "Keep the current configuration",
                "Replace with new configuration",
                "Add another account",
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
        eprintln!("  Existing accounts:");
        for &idx in existing_indices {
            eprintln!(
                "    • {} ({})",
                cfg.accounts[idx].id, cfg.accounts[idx].account_type
            );
        }
        let choice = select(
            &format!(
                "You have {} {name} accounts configured:",
                existing_indices.len()
            ),
            &["Keep all current accounts", "Add another account", "Skip"],
        );
        match choice {
            0 => ConnectorAction::Keep,
            1 => ConnectorAction::Add,
            _ => ConnectorAction::Skip,
        }
    }
}

async fn authenticate_account(account: &AccountConfig, store_path: &Path) -> anyhow::Result<()> {
    let mut conn = connector_factory::build_connector(account, store_path)?;
    let conn_mut = Arc::get_mut(&mut conn)
        .ok_or_else(|| anyhow::anyhow!("internal error: could not get mutable connector ref"))?;
    conn_mut.authenticate().await
}
