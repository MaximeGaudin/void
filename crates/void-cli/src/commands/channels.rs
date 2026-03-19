use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::{resolve_connector_filter, OutputFormatter};

#[derive(Debug, Args)]
pub struct ChannelsArgs {
    /// Search channels/groups by name (supports partial match)
    #[arg()]
    pub search: Option<String>,
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar, telegram, hackernews)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
    /// Include muted channels/groups
    #[arg(long)]
    pub include_muted: bool,
}

pub fn run(args: &ChannelsArgs) -> anyhow::Result<()> {
    debug!(search = ?args.search, connection = ?args.connection, connector = ?args.connector, size = args.size, "channels");
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let channels = db.list_channels(
        args.connection.as_deref(),
        connector.as_deref(),
        args.search.as_deref(),
        args.size,
        args.include_muted,
    )?;
    formatter.print_conversations(&channels)
}
