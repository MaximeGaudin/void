use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use void_core::config::{self, AccountConfig, AccountSettings, AccountType, VoidConfig};

use super::channel_factory;

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
    eprintln!("╔══════════════════════════════════════════════╗");
    eprintln!("║            Void — Setup Wizard               ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!();
    eprintln!("This wizard will guide you through connecting your");
    eprintln!("communication channels (Gmail, Slack, WhatsApp,");
    eprintln!("Google Calendar) to Void.");
    eprintln!();
    eprintln!("You can run this wizard again at any time with `void setup`.");

    let config_path = config::default_config_path();
    let mut cfg = VoidConfig::load_or_default(&config_path);
    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    if !cfg.accounts.is_empty() {
        eprintln!();
        eprintln!("Current accounts:");
        for acc in &cfg.accounts {
            eprintln!("  • {} ({})", acc.id, acc.account_type);
        }
    }

    separator();
    setup_gmail(&mut cfg, &store_path).await?;
    separator();
    setup_slack(&mut cfg, &store_path).await?;
    separator();
    setup_whatsapp(&mut cfg, &store_path).await?;
    separator();
    setup_calendar(&mut cfg, &store_path).await?;
    separator();

    cfg.save(&config_path)?;
    eprintln!("Configuration saved to {}", config_path.display());

    eprintln!();
    if cfg.accounts.is_empty() {
        eprintln!("No connectors configured. Run `void setup` again when ready.");
    } else {
        eprintln!("Configured accounts:");
        for acc in &cfg.accounts {
            eprintln!("  • {} ({})", acc.id, acc.account_type);
        }
        eprintln!();
        if confirm("Start syncing now? (`void sync --daemon`)") {
            eprintln!();
            let args = super::sync::SyncArgs {
                channels: None,
                daemon: true,
                restart: false,
                clear: false,
            };
            super::sync::daemonize(&args, false)?;
        } else {
            eprintln!();
            eprintln!("You can start syncing later with: void sync --daemon");
        }
    }

    eprintln!();
    eprintln!("Setup complete.");
    Ok(())
}

// ── Gmail ───────────────────────────────────────────────────────────────────

async fn setup_gmail(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("📧  GMAIL");
    eprintln!();
    eprintln!("Connects your Gmail inbox. Void syncs your recent emails and");
    eprintln!("lets you search, read, reply, and archive from the CLI.");

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

    eprintln!();
    eprintln!("To connect Gmail, you need a Google OAuth credentials file.");
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
    eprintln!("  6. Save it somewhere safe, e.g. ~/.config/void/gmail.json");
    eprintln!();

    let creds = prompt("Path to credentials JSON file: ");
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

    if confirm("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Gmail authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void auth gmail {account_id}");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void auth gmail {account_id}");
    }

    cfg.accounts.push(account);
    Ok(())
}

// ── Slack ───────────────────────────────────────────────────────────────────

async fn setup_slack(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("💬  SLACK");
    eprintln!();
    eprintln!("Connects a Slack workspace. Void syncs your channels, DMs,");
    eprintln!("and lets you search and reply from the CLI.");

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

    if confirm("Verify tokens now?") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Slack tokens verified successfully."),
            Err(e) => {
                eprintln!("  ✗ Verification failed: {e}");
                eprintln!("    Check your tokens and retry with: void auth slack {account_id}");
            }
        }
    } else {
        eprintln!("  You can verify later with: void auth slack {account_id}");
    }

    cfg.accounts.push(account);
    Ok(())
}

// ── WhatsApp ────────────────────────────────────────────────────────────────

async fn setup_whatsapp(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("📱  WHATSAPP");
    eprintln!();
    eprintln!("Connects your WhatsApp account via QR code (like WhatsApp Web).");
    eprintln!("No credentials or API keys needed.");

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

    if confirm("Pair now?") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ WhatsApp paired successfully."),
            Err(e) => {
                eprintln!("  ✗ Pairing failed: {e}");
                eprintln!("    You can retry later with: void auth whatsapp {account_id}");
            }
        }
    } else {
        eprintln!("  You can pair later with: void auth whatsapp {account_id}");
    }

    cfg.accounts.push(account);
    Ok(())
}

// ── Google Calendar ─────────────────────────────────────────────────────────

async fn setup_calendar(cfg: &mut VoidConfig, store_path: &Path) -> anyhow::Result<()> {
    eprintln!("📅  GOOGLE CALENDAR");
    eprintln!();
    eprintln!("Syncs your Google Calendar events. Lets you view today's agenda,");
    eprintln!("this week's schedule, and upcoming events from the CLI.");

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

    eprintln!();

    let gmail_creds = cfg.accounts.iter().find_map(|a| {
        if a.account_type == AccountType::Gmail {
            if let AccountSettings::Gmail { credentials_file } = &a.settings {
                return Some(credentials_file.clone());
            }
        }
        None
    });

    let creds = if let Some(ref gmail_path) = gmail_creds {
        eprintln!("You have a Gmail credentials file configured: {gmail_path}");
        eprintln!("Google Calendar can reuse the same credentials file.");
        eprintln!("(If you do, you need to also enable the \"Google Calendar API\"");
        eprintln!(" in the same Google Cloud project.)");
        eprintln!();

        if confirm("Reuse Gmail credentials?") {
            gmail_path.clone()
        } else {
            let path = prompt("Path to Calendar credentials JSON: ");
            if path.is_empty() {
                eprintln!("  Skipped (no path provided).");
                return Ok(());
            }
            path
        }
    } else {
        eprintln!("To connect Google Calendar, you need a Google OAuth credentials");
        eprintln!("file, similar to Gmail setup:");
        eprintln!();
        eprintln!("  1. Go to https://console.cloud.google.com/apis/credentials");
        eprintln!("  2. Create a project (or select an existing one)");
        eprintln!("  3. Enable the \"Google Calendar API\" (APIs & Services > Library)");
        eprintln!("  4. Go to \"OAuth consent screen\" and configure it");
        eprintln!("  5. Go to Credentials > Create Credentials > OAuth client ID");
        eprintln!("     - Application type: Desktop app");
        eprintln!("     - Download the JSON file");
        eprintln!();

        let path = prompt("Path to credentials JSON file: ");
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

    if confirm("Authenticate now? (opens browser for Google sign-in)") {
        match authenticate_account(&account, store_path).await {
            Ok(()) => eprintln!("  ✓ Calendar authenticated successfully."),
            Err(e) => {
                eprintln!("  ✗ Authentication failed: {e}");
                eprintln!("    You can retry later with: void auth calendar {account_id}");
            }
        }
    } else {
        eprintln!("  You can authenticate later with: void auth calendar {account_id}");
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
    let mut channel = channel_factory::build_channel(account, store_path)?;
    let channel_mut = Arc::get_mut(&mut channel)
        .ok_or_else(|| anyhow::anyhow!("internal error: could not get mutable channel ref"))?;
    channel_mut.authenticate().await
}
