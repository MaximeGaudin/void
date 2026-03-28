use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use super::pagination::{build_meta, parse_page};
use crate::output::{resolve_connector_filter, OutputFormatter};

#[derive(Debug, Args)]
pub struct ContactsArgs {
    /// Search contacts by name or address (supports partial match)
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
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

pub fn run(args: &ContactsArgs) -> anyhow::Result<()> {
    debug!(search = ?args.search, connection = ?args.connection, connector = ?args.connector, size = args.size, page = args.page, "contacts");
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();
    let offset = parse_page(args.size, args.page)?;

    let (contacts, total_elements) = db.list_contacts_paginated(
        args.connection.as_deref(),
        connector.as_deref(),
        args.search.as_deref(),
        args.size,
        offset,
    )?;
    let meta = build_meta(args.page, args.size, total_elements);
    formatter.print_paginated(&contacts, meta)
}
