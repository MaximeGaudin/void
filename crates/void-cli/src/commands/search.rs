use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Maximum results
    #[arg(long, default_value = "50")]
    pub limit: i64,
}

pub fn run(args: &SearchArgs, json: bool) -> anyhow::Result<()> {
    debug!(query = %args.query, "search");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let messages = db.search_messages(&args.query, args.limit)?;
    formatter.print_messages(&messages)
}
