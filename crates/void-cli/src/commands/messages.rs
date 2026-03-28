use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::dedup_context_messages;

use super::pagination::{build_meta, parse_page};
use super::resolve::{resolve_messages_target, MessagesTarget};
use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct MessagesArgs {
    /// Conversation ID or Slack message link
    pub target: String,
    /// Show messages since this date (YYYY-MM-DD)
    #[arg(long)]
    pub since: Option<String>,
    /// Show messages until this date (YYYY-MM-DD)
    #[arg(long)]
    pub until: Option<String>,
    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "100")]
    pub size: i64,
    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

pub fn run(args: &MessagesArgs, enrich_context: bool) -> anyhow::Result<()> {
    debug!(target = %args.target, size = args.size, page = args.page, "messages");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new();

    match resolve_messages_target(&args.target) {
        MessagesTarget::Link {
            message_id,
            conversation_id,
        } => {
            let msg = db
                .get_message(&message_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Message not found for link (id: {message_id}, conversation: {conversation_id})"
                    )
                })?;
            let mut messages = vec![msg];
            if enrich_context {
                db.enrich_with_context(&mut messages)?;
            }
            formatter.print_messages(&messages)
        }
        MessagesTarget::ConversationId(conv_id) => {
            let since = args.since.as_deref().and_then(parse_date_to_ts);
            let until = args.until.as_deref().and_then(parse_date_to_ts);
            let offset = parse_page(args.size, args.page)?;

            let (mut messages, total_elements) =
                db.list_messages_paginated(&conv_id, args.size, offset, since, until)?;
            if enrich_context {
                db.enrich_with_context(&mut messages)?;
                messages = dedup_context_messages(messages);
            }
            let meta = build_meta(args.page, args.size, total_elements);
            formatter.print_paginated(&messages, meta)
        }
    }
}

fn parse_date_to_ts(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}
