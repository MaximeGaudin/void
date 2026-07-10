use clap::Args;
use tracing::debug;

use crate::service;
use crate::service::reads::{self, InboxQuery};

#[derive(Debug, Args)]
pub struct InboxArgs {
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
    /// Include archived messages
    #[arg(long)]
    pub all: bool,
    /// Include messages from muted conversations
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &InboxArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(connection = ?args.connection, connector = ?args.connector, size = args.size, page = args.page, all = args.all, "inbox");
    let db = crate::context::open_db()?;
    let query = InboxQuery {
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
        size: args.size,
        page: args.page,
        all: args.all,
        include_muted: args.include_muted,
    };
    let value = reads::inbox(&db, &query, enrich_context)?;
    println!("{}", service::render(&value)?);
    Ok(())
}

pub fn run_conversations(args: &InboxArgs) -> anyhow::Result<()> {
    debug!(connection = ?args.connection, connector = ?args.connector, size = args.size, page = args.page, "inbox conversations");
    let db = crate::context::open_db()?;
    let query = InboxQuery {
        connection: args.connection.as_deref(),
        connector: args.connector.as_deref(),
        size: args.size,
        page: args.page,
        all: args.all,
        include_muted: args.include_muted,
    };
    let value = reads::conversations(&db, &query)?;
    println!("{}", service::render(&value)?);
    Ok(())
}
