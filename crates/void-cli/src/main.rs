mod commands;
pub mod output;

use clap::{Parser, Subcommand};

/// Void: unified communication CLI for WhatsApp, Telegram, Slack, Gmail, and Google Calendar
#[derive(Debug, Parser)]
#[command(name = "void", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Override store directory
    #[arg(long, global = true)]
    store: Option<String>,

    /// Enable verbose logging
    #[arg(long, short, global = true)]
    verbose: bool,

    /// Disable context enrichment (related messages) on output
    #[arg(long, global = true)]
    no_context: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Interactive setup wizard — configure all connectors
    Setup,
    /// Start background sync
    Sync(commands::sync::SyncArgs),
    /// Check configuration and connectivity
    Doctor,
    /// Show recent messages across all connectors
    Inbox(commands::inbox::InboxArgs),
    /// List conversations
    Conversations(commands::inbox::InboxArgs),
    /// Show messages in a conversation
    Messages(commands::messages::MessagesArgs),
    /// List contacts across all connectors
    Contacts(commands::contacts::ContactsArgs),
    /// List channels and groups (excluding DMs)
    Channels(commands::channels::ChannelsArgs),
    /// Full-text search across messages
    Search(commands::search::SearchArgs),
    /// Send a new message
    Send(commands::send::SendArgs),
    /// Reply to a message
    Reply(commands::reply::ReplyArgs),
    /// Forward a message to another recipient
    Forward(commands::forward::ForwardArgs),
    /// Archive one or more messages (e.g., remove from Gmail inbox)
    Archive(commands::archive::ArchiveArgs),
    /// Mute or unmute conversations/channels (hides from inbox)
    Mute(commands::mute::MuteArgs),
    /// Gmail-specific operations (search, threads, drafts, labels, attachments)
    Gmail(commands::gmail::GmailArgs),
    /// Hacker News configuration (keywords, min-score)
    Hn(commands::hackernews::HackerNewsArgs),
    /// Slack-specific operations (react, edit)
    Slack(commands::slack::SlackArgs),
    /// WhatsApp-specific operations (media download)
    Whatsapp(commands::whatsapp::WhatsAppArgs),
    /// Telegram-specific operations (media download)
    Telegram(commands::telegram::TelegramArgs),
    /// Calendar events
    Calendar(commands::calendar::CalendarArgs),
    /// Download files from Google Drive/Docs/Sheets/Slides
    Drive(commands::gdrive::GdriveArgs),
    /// Start an AI-powered agent for processing communications
    Agent(commands::agent::AgentArgs),
    /// Manage hooks — LLM prompts triggered by events or schedules
    Hook(commands::hook::HookArgs),
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Sync(ref args)) = cli.command {
        if args.stop {
            return commands::sync::stop_daemon();
        }
        if args.daemon {
            return commands::sync::daemonize(args, cli.verbose);
        }
        if args.daemon_inner {
            return commands::sync::run_daemon_inner(args, cli.verbose);
        }
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> anyhow::Result<()> {
    let base_level = if cli.verbose { "debug" } else { "warn" };
    let filter = format!("{base_level},wa_rs::handlers::notification=error");
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&filter)),
        )
        .with_writer(std::io::stderr)
        .init();

    match &cli.command {
        Some(Command::Setup) => commands::setup::run().await,
        Some(Command::Sync(args)) => commands::sync::run(args).await,
        Some(Command::Doctor) => commands::doctor::run(),
        Some(Command::Inbox(args)) => commands::inbox::run(args, !cli.no_context),
        Some(Command::Conversations(args)) => commands::inbox::run_conversations(args),
        Some(Command::Messages(args)) => commands::messages::run(args, !cli.no_context),
        Some(Command::Contacts(args)) => commands::contacts::run(args),
        Some(Command::Channels(args)) => commands::channels::run(args),
        Some(Command::Search(args)) => commands::search::run(args, !cli.no_context),
        Some(Command::Send(args)) => commands::send::run(args).await,
        Some(Command::Reply(args)) => commands::reply::run(args).await,
        Some(Command::Forward(args)) => commands::forward::run(args).await,
        Some(Command::Archive(args)) => commands::archive::run(args).await,
        Some(Command::Mute(args)) => commands::mute::run(args),
        Some(Command::Gmail(args)) => commands::gmail::run(args).await,
        Some(Command::Hn(args)) => Ok(commands::hackernews::run(args)?),
        Some(Command::Slack(args)) => commands::slack::run(args).await,
        Some(Command::Whatsapp(args)) => commands::whatsapp::run(args).await,
        Some(Command::Telegram(args)) => commands::telegram::run(args).await,
        Some(Command::Calendar(args)) => commands::calendar::run(args).await,
        Some(Command::Drive(args)) => commands::gdrive::run(args).await,
        Some(Command::Agent(args)) => commands::agent::run(args, cli.verbose).await,
        Some(Command::Hook(args)) => commands::hook::run(args),
        None => {
            commands::status::run();
            Ok(())
        }
    }
}
