use clap::Args;
use tracing::debug;

use crate::service;
use crate::service::reads::{self, SearchQuery};

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    #[arg(long, help = crate::output::CONNECTOR_FILTER_HELP)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "50")]
    pub size: i64,
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
    /// Include results from muted conversations
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &SearchArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(query = %args.query, connection = ?args.connection, connector = ?args.connector, size = args.size, page = args.page, "search");
    let db = crate::context::open_db()?;
    let query = SearchQuery {
        query: &args.query,
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
        size: args.size,
        page: args.page,
        include_muted: args.include_muted,
    };
    let value = reads::search(&db, &query, enrich_context)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
