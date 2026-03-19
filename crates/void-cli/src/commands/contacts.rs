use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::{resolve_connector_filter, OutputFormatter};

#[derive(Debug, Args)]
pub struct ContactsArgs {
    /// Search contacts by name or address (supports partial match)
    #[arg()]
    pub search: Option<String>,
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar, telegram, hackernews)
    #[arg(long)]
    pub connector: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
}

pub fn run(args: &ContactsArgs) -> anyhow::Result<()> {
    debug!(search = ?args.search, account = ?args.account, connector = ?args.connector, size = args.size, "contacts");
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    let contacts = db.list_contacts(
        args.account.as_deref(),
        connector.as_deref(),
        args.search.as_deref(),
        args.size,
    )?;
    formatter.print_contacts(&contacts)
}
