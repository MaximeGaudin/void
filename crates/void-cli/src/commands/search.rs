use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::dedup_context_messages;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "50")]
    pub size: i64,
    /// Include results from muted conversations
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &SearchArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(query = %args.query, account = ?args.account, connector = ?args.connector, size = args.size, "search");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let mut messages = db.search_messages(
        &args.query,
        args.account.as_deref(),
        args.connector.as_deref(),
        args.size,
        args.include_muted,
    )?;
    if enrich_context {
        db.enrich_with_context(&mut messages)?;
        messages = dedup_context_messages(messages);
    }
    formatter.print_messages(&messages)
}
