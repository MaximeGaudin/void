use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::dedup_context_messages;

use crate::output::{resolve_connector_filter, OutputFormatter};

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar, telegram, hackernews)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "50")]
    pub size: i64,
    /// Include archived messages
    #[arg(long)]
    pub all: bool,
    /// Include messages from muted conversations
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &InboxArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(connection = ?args.connection, connector = ?args.connector, size = args.size, all = args.all, "inbox");
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let include_muted = args.include_muted || args.all;
    let mut messages = db.recent_messages(
        args.connection.as_deref(),
        connector.as_deref(),
        args.size,
        args.all,
        include_muted,
    )?;
    messages.reverse();
    if enrich_context {
        db.enrich_with_context(&mut messages)?;
        messages = dedup_context_messages(messages);
    }
    formatter.print_messages(&messages)
}

pub fn run_conversations(args: &InboxArgs) -> anyhow::Result<()> {
    debug!(connection = ?args.connection, connector = ?args.connector, size = args.size, "inbox conversations");
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let conversations = db.list_conversations(
        args.connection.as_deref(),
        connector.as_deref(),
        args.size,
        args.include_muted,
    )?;
    formatter.print_conversations(&conversations)
}
