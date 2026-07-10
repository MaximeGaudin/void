use clap::Args;
use tracing::debug;

use crate::service;
use crate::service::reads::{self, MessagesQuery};

#[derive(Debug, Args)]
pub struct MessagesArgs {
    /// Conversation ID or Slack message link
    pub target: String,
    /// Show messages since this date (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,
    /// Show messages until this date (YYYY-MM-DD)
    #[arg(long)]
    pub until: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

pub fn run(args: &MessagesArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(target = %args.target, size = args.size, page = args.page, "messages");
    let db = crate::context::open_db()?;
    let query = MessagesQuery {
        target: &args.target,
        since: args.since.as_deref(),
        until: args.until.as_deref(),
        size: args.size,
        page: args.page,
    };
    let value = reads::messages(&db, &query, enrich_context)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
