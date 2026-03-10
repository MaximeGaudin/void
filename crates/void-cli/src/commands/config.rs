use clap::{Args, Subcommand};
use void_core::config::{self, VoidConfig};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Create default config file
    Init,
    /// Show current configuration
    Show,
    /// Open config in $EDITOR
    Edit,
    /// Print config file path
    Path,
}

pub fn run(args: &ConfigArgs) -> anyhow::Result<()> {
    match &args.command {
        ConfigCommand::Init => cmd_init(),
        ConfigCommand::Show => cmd_show(),
        ConfigCommand::Edit => cmd_edit(),
        ConfigCommand::Path => cmd_path(),
    }
}

fn cmd_init() -> anyhow::Result<()> {
    let path = config::default_config_path();
    if path.exists() {
        eprintln!("Config file already exists at {}", path.display());
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, config::default_config())?;
    eprintln!("Created default config at {}", path.display());
    Ok(())
}

fn cmd_show() -> anyhow::Result<()> {
    let path = config::default_config_path();
    if !path.exists() {
        eprintln!("No config file found. Run `void config init` to create one.");
        return Ok(());
    }
    let cfg = VoidConfig::load(&path)?;
    eprintln!("Config file: {}", path.display());
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
                } => {
                    eprintln!("    app_token:  {}", config::redact_token(app_token));
                    eprintln!("    user_token: {}", config::redact_token(user_token));
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
    Ok(())
}

fn cmd_edit() -> anyhow::Result<()> {
    let path = config::default_config_path();
    if !path.exists() {
        eprintln!("No config file found. Run `void config init` first.");
        return Ok(());
    }
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new(&editor).arg(&path).status()?;
    if !status.success() {
        anyhow::bail!("{editor} exited with status {status}");
    }
    Ok(())
}

fn cmd_path() -> anyhow::Result<()> {
    println!("{}", config::default_config_path().display());
    Ok(())
}
