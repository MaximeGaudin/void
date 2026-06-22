use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::commands;
use crate::context;

/// Void: unified communication CLI for WhatsApp, Telegram, Slack, Gmail, Google Calendar, and LinkedIn
#[derive(Debug, Parser)]
#[command(name = "void", version, about)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,

    /// Override store directory
    #[arg(long, global = true)]
    pub(crate) store: Option<String>,

    /// Override config file path (local client profile when store.mode = remote)
    #[arg(long, global = true)]
    pub(crate) config: Option<String>,

    /// Enable verbose logging
    #[arg(long, short, global = true)]
    pub(crate) verbose: bool,

    /// Disable context enrichment (related messages) on output
    #[arg(long, global = true)]
    pub(crate) no_context: bool,

    /// Force local store (set by SSH proxy on the server; hidden)
    #[arg(long, global = true, hide = true)]
    pub(crate) local_store: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Interactive setup wizard — configure all connectors
    Setup,
    /// Start background sync
    Sync(commands::sync::SyncArgs),
    /// Check configuration and connectivity
    Doctor(commands::doctor::DoctorArgs),
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
    /// Gmail-specific operations (search, threads, drafts, labels, attachments, forward)
    Gmail(commands::gmail::GmailArgs),
    /// Hacker News configuration (keywords, min-score)
    Hn(commands::hackernews::HackerNewsArgs),
    /// Google News configuration (keywords, recency, language, country)
    Gn(commands::googlenews::GoogleNewsArgs),
    /// Reddit configuration (subreddits, keywords, min-score)
    #[command(name = "reddit", alias = "rd")]
    Reddit(commands::reddit::RedditArgs),
    /// Slack-specific operations (react, edit, schedule, open, forward)
    Slack(commands::slack::SlackArgs),
    /// WhatsApp-specific operations (media download)
    Whatsapp(commands::whatsapp::WhatsAppArgs),
    /// Telegram-specific operations (media download, forward)
    Telegram(commands::telegram::TelegramArgs),
    /// LinkedIn-specific operations (media download via Unipile)
    Linkedin(commands::linkedin::LinkedInArgs),
    /// Calendar events
    Calendar(commands::calendar::CalendarArgs),
    /// Manage hooks — LLM prompts triggered by events or schedules
    Hook(commands::hook::HookArgs),
    /// Remote store utilities (status, cache refresh)
    Remote(commands::remote::RemoteArgs),
}

fn refresh_policy_for_cli(cli: &Cli) -> void_core::store::RefreshPolicy {
    use void_core::config::{resolve_config_path, StoreMode, VoidConfig};
    use void_core::store::RefreshPolicy;

    let config_path = cli
        .config
        .as_deref()
        .map(|s| resolve_config_path(Some(std::path::Path::new(s))))
        .unwrap_or_else(void_core::config::default_config_path);

    let is_remote = std::fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| VoidConfig::parse(&content).ok())
        .is_some_and(|cfg| cfg.store.mode == StoreMode::Remote);

    if !is_remote {
        return RefreshPolicy::UseCache;
    }

    match &cli.command {
        Some(cmd) if !context::runs_with_local_cache(cmd) => RefreshPolicy::ProxyOnly,
        _ => RefreshPolicy::UseCache,
    }
}

pub fn run() -> anyhow::Result<()> {
    let mut cmd = Cli::command();
    crate::output::patch_connector_arg_help(&mut cmd);
    let matches = cmd.get_matches();
    let cli = Cli::from_arg_matches(&matches)?;

    let refresh = refresh_policy_for_cli(&cli);
    context::init(
        cli.config.as_deref(),
        cli.store.as_deref(),
        refresh,
        cli.local_store,
    )?;

    if let Some(Command::Sync(ref args)) = cli.command {
        if args.status {
            return commands::sync::show_status();
        }
        if args.stop {
            if context::is_remote() {
                context::ensure_local_sync_allowed()?;
            }
            return commands::sync::stop_daemon();
        }
        if args.daemon {
            context::ensure_local_sync_allowed()?;
            return commands::sync::daemonize(args, cli.verbose);
        }
        if args.daemon_inner {
            return commands::sync::run_daemon_inner(args, cli.verbose);
        }
    }

    if context::is_remote() {
        if matches!(cli.command, Some(Command::Setup)) {
            context::ensure_local_setup_allowed()?;
        } else if matches!(cli.command, Some(Command::Sync(_))) {
            context::ensure_local_sync_allowed()?;
        } else if let Some(ref cmd) = cli.command {
            if !context::runs_with_local_cache(cmd) {
                context::proxy_current_command()?;
            }
        }
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli))
}

async fn async_main(cli: Cli) -> anyhow::Result<()> {
    let base_level = if cli.verbose { "debug" } else { "warn" };
    let filter = format!("{base_level},wa_rs::handlers::notification=error,html5ever=error");
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
        Some(Command::Doctor(args)) => commands::doctor::run(args).await,
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
        Some(Command::Gn(args)) => Ok(commands::googlenews::run(args)?),
        Some(Command::Reddit(args)) => Ok(commands::reddit::run(args)?),
        Some(Command::Slack(args)) => commands::slack::run(args).await,
        Some(Command::Whatsapp(args)) => commands::whatsapp::run(args).await,
        Some(Command::Telegram(args)) => commands::telegram::run(args).await,
        Some(Command::Linkedin(args)) => commands::linkedin::run(args).await,
        Some(Command::Calendar(args)) => commands::calendar::run(args).await,
        Some(Command::Hook(args)) => commands::hook::run(args),
        Some(Command::Remote(args)) => {
            commands::remote::run(args, cli.config.as_deref(), cli.store.as_deref())
        }
        None => {
            commands::status::run();
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("should parse")
    }

    fn parse_err(args: &[&str]) -> clap::Error {
        Cli::try_parse_from(args).expect_err("should fail to parse")
    }

    #[test]
    fn gmail_draft_create_uses_remote_proxy_in_remote_mode() {
        let cli = parse(&[
            "void",
            "gmail",
            "draft",
            "create",
            "--to",
            "a@b.com",
            "--subject",
            "s",
            "--body",
            "b",
        ]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(!context::runs_with_local_cache(cmd));
    }

    #[test]
    fn send_with_file_uses_remote_proxy_in_remote_mode() {
        let cli = parse(&[
            "void",
            "send",
            "--via",
            "gmail",
            "--to",
            "a@b.com",
            "--message",
            "hi",
            "--file",
            "/tmp/x.pdf",
        ]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(!context::runs_with_local_cache(cmd));
    }

    #[test]
    fn gmail_attachment_uses_remote_proxy_in_remote_mode() {
        let cli = parse(&[
            "void",
            "gmail",
            "attachment",
            "m1",
            "a1",
            "--out",
            "/tmp/x.pdf",
        ]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(!context::runs_with_local_cache(cmd));
    }

    #[test]
    fn whatsapp_download_uses_remote_proxy_in_remote_mode() {
        let cli = parse(&["void", "whatsapp", "download", "m1", "--out", "/tmp/x.jpg"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(!context::runs_with_local_cache(cmd));
    }

    #[test]
    fn calendar_day_today_uses_local_cache() {
        let cli = parse(&["void", "calendar", "--day", "today"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(context::runs_with_local_cache(cmd));
    }

    #[test]
    fn hn_keywords_list_uses_local_cache() {
        let cli = parse(&["void", "hn", "keywords", "list"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(context::runs_with_local_cache(cmd));
    }

    #[test]
    fn reddit_keywords_list_uses_local_cache() {
        let cli = parse(&["void", "reddit", "keywords", "list"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(context::runs_with_local_cache(cmd));
    }

    #[test]
    fn slack_saved_uses_local_cache() {
        let cli = parse(&["void", "slack", "saved"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(context::runs_with_local_cache(cmd));
    }

    #[test]
    fn reddit_min_score_uses_remote_proxy_in_remote_mode() {
        let cli = parse(&["void", "reddit", "min-score", "100"]);
        let cmd = cli.command.as_ref().expect("command");
        assert!(!context::runs_with_local_cache(cmd));
    }

    #[test]
    fn parse_slack_saved_minimal() {
        let cli = parse(&["void", "slack", "saved"]);
        match cli.command {
            Some(Command::Slack(ref args)) => match &args.command {
                commands::slack::SlackCommand::Saved(saved) => {
                    assert!(saved.connection.is_none());
                    assert_eq!(saved.size, 50);
                    assert_eq!(saved.page, 1);
                }
                other => panic!("expected Saved, got {other:?}"),
            },
            other => panic!("expected Slack, got {other:?}"),
        }
    }

    #[test]
    fn parse_slack_saved_with_filters() {
        let cli = parse(&[
            "void",
            "slack",
            "saved",
            "--connection",
            "slack-gladia",
            "-n",
            "10",
            "--page",
            "2",
        ]);
        match cli.command {
            Some(Command::Slack(ref args)) => match &args.command {
                commands::slack::SlackCommand::Saved(saved) => {
                    assert_eq!(saved.connection.as_deref(), Some("slack-gladia"));
                    assert_eq!(saved.size, 10);
                    assert_eq!(saved.page, 2);
                }
                other => panic!("expected Saved, got {other:?}"),
            },
            other => panic!("expected Slack, got {other:?}"),
        }
    }

    // --- Gmail forward parsing ---

    #[test]
    fn parse_gmail_forward_minimal() {
        let cli = parse(&["void", "gmail", "forward", "msg123", "--to", "a@b.com"]);
        match cli.command {
            Some(Command::Gmail(ref g)) => match &g.command {
                commands::gmail::GmailCommand::Forward(f) => {
                    assert_eq!(f.message_id, "msg123");
                    assert_eq!(f.to, "a@b.com");
                    assert!(f.comment.is_none());
                    assert!(f.connection.is_none());
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Gmail, got {other:?}"),
        }
    }

    #[test]
    fn parse_gmail_forward_with_comment_and_connection() {
        let cli = parse(&[
            "void",
            "gmail",
            "forward",
            "msg1",
            "--to",
            "x@y.com",
            "--comment",
            "FYI",
            "--connection",
            "work",
        ]);
        match cli.command {
            Some(Command::Gmail(ref g)) => match &g.command {
                commands::gmail::GmailCommand::Forward(f) => {
                    assert_eq!(f.comment.as_deref(), Some("FYI"));
                    assert_eq!(f.connection.as_deref(), Some("work"));
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Gmail, got {other:?}"),
        }
    }

    #[test]
    fn parse_gmail_forward_maps_to_gmail_subcommand() {
        let cli = parse(&["void", "gmail", "forward", "m1", "--to", "a@b.com"]);
        assert!(matches!(cli.command, Some(Command::Gmail(_))));
    }

    #[test]
    fn parse_gmail_forward_requires_to() {
        parse_err(&["void", "gmail", "forward", "msg123"]);
    }

    #[test]
    fn parse_gmail_forward_requires_message_id() {
        parse_err(&["void", "gmail", "forward", "--to", "a@b.com"]);
    }

    // --- Slack forward parsing ---

    #[test]
    fn parse_slack_forward_minimal() {
        let cli = parse(&["void", "slack", "forward", "msg456", "--to", "C12345"]);
        match cli.command {
            Some(Command::Slack(ref s)) => match &s.command {
                commands::slack::SlackCommand::Forward(f) => {
                    assert_eq!(f.message_id, "msg456");
                    assert_eq!(f.to, "C12345");
                    assert!(f.comment.is_none());
                    assert!(f.connection.is_none());
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Slack, got {other:?}"),
        }
    }

    #[test]
    fn parse_slack_forward_with_comment_and_connection() {
        let cli = parse(&[
            "void",
            "slack",
            "forward",
            "msg1",
            "--to",
            "C999",
            "--comment",
            "check this",
            "--connection",
            "acme",
        ]);
        match cli.command {
            Some(Command::Slack(ref s)) => match &s.command {
                commands::slack::SlackCommand::Forward(f) => {
                    assert_eq!(f.comment.as_deref(), Some("check this"));
                    assert_eq!(f.connection.as_deref(), Some("acme"));
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Slack, got {other:?}"),
        }
    }

    #[test]
    fn parse_slack_forward_maps_to_slack_subcommand() {
        let cli = parse(&["void", "slack", "forward", "m1", "--to", "C1"]);
        assert!(matches!(cli.command, Some(Command::Slack(_))));
    }

    #[test]
    fn parse_slack_forward_requires_to() {
        parse_err(&["void", "slack", "forward", "msg456"]);
    }

    #[test]
    fn parse_slack_forward_requires_message_id() {
        parse_err(&["void", "slack", "forward", "--to", "C12345"]);
    }

    // --- Telegram forward parsing ---

    #[test]
    fn parse_telegram_forward_minimal() {
        let cli = parse(&["void", "telegram", "forward", "msg789", "--to", "chat42"]);
        match cli.command {
            Some(Command::Telegram(ref t)) => match &t.command {
                commands::telegram::TelegramCommand::Forward(f) => {
                    assert_eq!(f.message_id, "msg789");
                    assert_eq!(f.to, "chat42");
                    assert!(f.comment.is_none());
                    assert!(f.connection.is_none());
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Telegram, got {other:?}"),
        }
    }

    #[test]
    fn parse_telegram_forward_with_comment_and_connection() {
        let cli = parse(&[
            "void",
            "telegram",
            "forward",
            "m1",
            "--to",
            "chat1",
            "--comment",
            "note",
            "--connection",
            "personal",
        ]);
        match cli.command {
            Some(Command::Telegram(ref t)) => match &t.command {
                commands::telegram::TelegramCommand::Forward(f) => {
                    assert_eq!(f.comment.as_deref(), Some("note"));
                    assert_eq!(f.connection.as_deref(), Some("personal"));
                }
                other => panic!("expected Forward, got {other:?}"),
            },
            other => panic!("expected Telegram, got {other:?}"),
        }
    }

    #[test]
    fn parse_telegram_forward_maps_to_telegram_subcommand() {
        let cli = parse(&["void", "telegram", "forward", "m1", "--to", "c1"]);
        assert!(matches!(cli.command, Some(Command::Telegram(_))));
    }

    #[test]
    fn parse_telegram_forward_requires_to() {
        parse_err(&["void", "telegram", "forward", "msg789"]);
    }

    #[test]
    fn parse_telegram_forward_requires_message_id() {
        parse_err(&["void", "telegram", "forward", "--to", "chat42"]);
    }

    // --- Global forward regression ---

    #[test]
    fn parse_global_forward_still_works() {
        let cli = parse(&["void", "forward", "msg1", "--to", "someone"]);
        assert!(matches!(cli.command, Some(Command::Forward(_))));
    }

    // --- Unsupported connector forward rejection ---

    #[test]
    fn parse_whatsapp_forward_is_rejected() {
        parse_err(&["void", "whatsapp", "forward", "msg1", "--to", "dest"]);
    }

    #[test]
    fn parse_calendar_forward_is_rejected() {
        parse_err(&["void", "calendar", "forward", "msg1", "--to", "dest"]);
    }

    #[test]
    fn parse_hn_forward_is_rejected() {
        parse_err(&["void", "hn", "forward", "msg1", "--to", "dest"]);
    }

    // --- Help surface tests ---

    #[test]
    fn parse_doctor_non_interactive() {
        let cli = parse(&["void", "doctor", "--non-interactive"]);
        match cli.command {
            Some(Command::Doctor(ref args)) => assert!(args.non_interactive),
            other => panic!("expected Doctor, got {other:?}"),
        }
    }

    #[test]
    fn help_gmail_lists_forward_subcommand() {
        let err = Cli::try_parse_from(["void", "gmail", "help"]).unwrap_err();
        let help = err.to_string();
        assert!(
            help.contains("forward"),
            "Gmail help should list 'forward': {help}"
        );
    }

    #[test]
    fn help_slack_lists_forward_subcommand() {
        let err = Cli::try_parse_from(["void", "slack", "help"]).unwrap_err();
        let help = err.to_string();
        assert!(
            help.contains("forward"),
            "Slack help should list 'forward': {help}"
        );
    }

    #[test]
    fn help_telegram_lists_forward_subcommand() {
        let err = Cli::try_parse_from(["void", "telegram", "help"]).unwrap_err();
        let help = err.to_string();
        assert!(
            help.contains("forward"),
            "Telegram help should list 'forward': {help}"
        );
    }
}
