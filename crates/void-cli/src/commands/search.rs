use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::dedup_context_messages;

use super::pagination::{build_meta, parse_page};
use crate::output::{resolve_connector_filter, OutputFormatter};

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Filter by connection (partial match on connection_id)
    #[arg(long)]
    pub connection: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp, calendar, telegram, hackernews)
    #[arg(long)]
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
    let connector = resolve_connector_filter(args.connector.as_deref())?;
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();
    let offset = parse_page(args.size, args.page)?;

    let (mut messages, total_elements) = db.search_messages_paginated(
        &args.query,
        args.connection.as_deref(),
        connector.as_deref(),
        args.size,
        offset,
        args.include_muted,
    )?;
    if enrich_context {
        db.enrich_with_context(&mut messages)?;
        messages = dedup_context_messages(messages);
    }
    let meta = build_meta(args.page, args.size, total_elements);
    formatter.print_paginated(&messages, meta)
}
