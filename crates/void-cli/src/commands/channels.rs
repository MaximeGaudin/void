use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct ChannelsArgs {
    /// Search channels/groups by name (supports partial match)
    #[arg()]
    pub search: Option<String>,
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of channels to show
    #[arg(long, default_value = "100")]
    pub limit: i64,
}

pub fn run(args: &ChannelsArgs, json: bool) -> anyhow::Result<()> {
    debug!(search = ?args.search, account = ?args.account, connector = ?args.connector, limit = args.limit, "channels");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let channels = db.list_channels(
        args.account.as_deref(),
        args.connector.as_deref(),
        args.search.as_deref(),
        args.limit,
    )?;
    formatter.print_conversations(&channels)
}
