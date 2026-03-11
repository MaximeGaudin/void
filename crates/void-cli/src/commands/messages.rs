use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct MessagesArgs {
    /// Conversation ID
    pub conversation_id: String,
    /// Show messages since this date (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,
    /// Show messages until this date (YYYY-MM-DD)
    #[arg(long)]
    pub until: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
}

pub fn run(args: &MessagesArgs, json: bool) -> anyhow::Result<()> {
    debug!(conversation_id = %args.conversation_id, size = args.size, "messages");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let since = args.since.as_deref().and_then(parse_date_to_ts);
    let until = args.until.as_deref().and_then(parse_date_to_ts);

    let messages = db.list_messages(&args.conversation_id, args.size, since, until)?;
    formatter.print_messages(&messages)
}

fn parse_date_to_ts(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}
