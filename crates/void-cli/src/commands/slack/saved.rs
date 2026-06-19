use clap::Args;
use tracing::debug;

use super::super::pagination::{build_meta, parse_page};
use crate::output::OutputFormatter;

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
    let _cfg = crate::context::config();
    let db = crate::context::open_db()?;
    let formatter = OutputFormatter::new();
    let offset = parse_page(args.size, args.page)?;

    let (mut messages, total_elements) =
        db.list_saved_messages(args.connection.as_deref(), Some("slack"), args.size, offset)?;
    messages.reverse();
    let meta = build_meta(args.page, args.size, total_elements);
    formatter.print_paginated(&messages, meta)
}
