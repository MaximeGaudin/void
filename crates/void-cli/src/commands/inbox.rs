use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::dedup_context_messages;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar)
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

pub fn run(args: &InboxArgs, json: bool, enrich_context: bool) -> anyhow::Result<()> {
    debug!(account = ?args.account, connector = ?args.connector, size = args.size, all = args.all, "inbox");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let include_muted = args.include_muted || args.all;
    let mut messages = db.recent_messages(
        args.account.as_deref(),
        args.connector.as_deref(),
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

pub fn run_conversations(args: &InboxArgs, json: bool) -> anyhow::Result<()> {
    debug!(account = ?args.account, connector = ?args.connector, size = args.size, "inbox conversations");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let conversations = db.list_conversations(
        args.account.as_deref(),
        args.connector.as_deref(),
        args.size,
        args.include_muted,
    )?;
    formatter.print_conversations(&conversations)
}
