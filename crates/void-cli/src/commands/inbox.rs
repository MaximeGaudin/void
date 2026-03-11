use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of messages to show
    #[arg(long, default_value = "50")]
    pub limit: i64,
    /// Include archived messages
    #[arg(long)]
    pub all: bool,
}

pub fn run(args: &InboxArgs, json: bool) -> anyhow::Result<()> {
    debug!(account = ?args.account, connector = ?args.connector, limit = args.limit, all = args.all, "inbox");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let mut messages = db.recent_messages(
        args.account.as_deref(),
        args.connector.as_deref(),
        args.limit,
        args.all,
    )?;
    messages.reverse();
    formatter.print_messages(&messages)
}

pub fn run_conversations(args: &InboxArgs, json: bool) -> anyhow::Result<()> {
    debug!(account = ?args.account, connector = ?args.connector, limit = args.limit, "inbox conversations");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let conversations =
        db.list_conversations(args.account.as_deref(), args.connector.as_deref(), args.limit)?;
    formatter.print_conversations(&conversations)
}
