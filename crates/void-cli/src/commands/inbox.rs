use clap::Args;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::output::OutputFormatter;

#[derive(Debug, Args)]
pub struct InboxArgs {
    /// Filter by channel type: whatsapp, slack, gmail
    #[arg(long)]
    pub channel: Option<String>,
    /// Maximum number of messages to show
    #[arg(long, default_value = "50")]
    pub limit: i64,
}

pub fn run(args: &InboxArgs, json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let messages = db.recent_messages(args.channel.as_deref(), args.limit)?;
    formatter.print_messages(&messages)
}

pub fn run_conversations(args: &InboxArgs, json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;
    let formatter = OutputFormatter::new(json);

    let conversations = db.list_conversations(args.channel.as_deref(), args.limit)?;
    formatter.print_conversations(&conversations)
}
