mod commands;
pub mod output;

use clap::{Parser, Subcommand};

/// Void: unified communication CLI for WhatsApp, Slack, Gmail, and Google Calendar
#[derive(Debug, Parser)]
#[command(name = "void", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Output as JSON instead of human-readable text
    #[arg(long, global = true)]
    json: bool,

    /// Override store directory
    #[arg(long, global = true)]
    store: Option<String>,

    /// Enable verbose logging
    #[arg(long, short, global = true)]
    verbose: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Authenticate a channel
    Auth(commands::auth::AuthArgs),
    /// Start background sync
    Sync(commands::sync::SyncArgs),
    /// Check configuration and connectivity
    Doctor,
    /// Show recent messages across all channels
    Inbox(commands::inbox::InboxArgs),
    /// List conversations
    Conversations(commands::inbox::InboxArgs),
    /// Show messages in a conversation
    Messages(commands::messages::MessagesArgs),
    /// Full-text search across messages
    Search(commands::search::SearchArgs),
    /// Send a new message
    Send(commands::send::SendArgs),
    /// Reply to a message
    Reply(commands::reply::ReplyArgs),
    /// Calendar events
    Calendar(commands::calendar::CalendarArgs),
    /// Manage accounts
    Accounts(commands::accounts::AccountsArgs),
    /// Configuration management
    Config(commands::config::ConfigArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    match &cli.command {
        Some(Command::Auth(args)) => commands::auth::run(args),
        Some(Command::Sync(args)) => commands::sync::run(args),
        Some(Command::Doctor) => commands::doctor::run(),
        Some(Command::Inbox(args)) => commands::inbox::run(args, cli.json),
        Some(Command::Conversations(args)) => commands::inbox::run_conversations(args, cli.json),
        Some(Command::Messages(args)) => commands::messages::run(args, cli.json),
        Some(Command::Search(args)) => commands::search::run(args, cli.json),
        Some(Command::Send(args)) => commands::send::run(args),
        Some(Command::Reply(args)) => commands::reply::run(args),
        Some(Command::Calendar(args)) => commands::calendar::run(args, cli.json),
        Some(Command::Accounts(args)) => commands::accounts::run(args),
        Some(Command::Config(args)) => commands::config::run(args),
        None => {
            eprintln!("void: unified communication CLI (run --help for usage)");
            Ok(())
        }
    }
}
