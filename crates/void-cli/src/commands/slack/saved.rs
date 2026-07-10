use clap::Args;
use tracing::debug;

use crate::service;
use crate::service::reads::{self, SlackSavedQuery};

#[derive(Debug, Args)]
pub struct SavedArgs {
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "50")]
    pub size: i64,
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

pub fn run(args: &SavedArgs) -> anyhow::Result<()> {
    debug!(
        connection = ?args.connection,
        size = args.size,
        page = args.page,
        "slack saved"
    );
    let db = crate::context::open_db()?;
    let query = SlackSavedQuery {
        connection: args.connection.as_deref(),
        size: args.size,
        page: args.page,
    };
    let value = reads::slack_saved(&db, &query)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
