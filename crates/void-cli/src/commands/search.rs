use clap::Args;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by channel type
    #[arg(long)]
    pub channel: Option<String>,
    /// Maximum results
    #[arg(long, default_value = "50")]
    pub limit: i64,
}

pub fn run(args: &SearchArgs, json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let messages = db.search_messages(&args.query, args.limit)?;
    formatter.print_messages(&messages)
}
