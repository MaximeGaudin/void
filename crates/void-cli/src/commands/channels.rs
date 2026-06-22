use clap::Args;
use tracing::debug;

use crate::service;
use crate::service::reads::{self, ChannelsQuery};

#[derive(Debug, Args)]
pub struct ChannelsArgs {
    /// Search channels/groups by name (supports partial match)
    #[arg()]
    pub search: Option<String>,
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    #[arg(long, help = crate::output::CONNECTOR_FILTER_HELP)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
    /// Include muted channels/groups
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &ChannelsArgs) -> anyhow::Result<()> {
    debug!(search = ?args.search, connection = ?args.connection, connector = ?args.connector, size = args.size, page = args.page, "channels");
    let db = crate::context::open_db()?;
    let query = ChannelsQuery {
        search: args.search.as_deref(),
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
        size: args.size,
        page: args.page,
        include_muted: args.include_muted,
    };
    let value = reads::channels(&db, &query)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
