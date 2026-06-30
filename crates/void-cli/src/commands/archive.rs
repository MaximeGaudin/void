use clap::Args;
use tracing::debug;

use crate::service::writes::{self, ArchiveParams};

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Message IDs to archive (one or more)
    pub message_ids: Vec<String>,

    /// Archive all unarchived messages before this date (YYYY-MM-DD).
    /// Mutually exclusive with positional message IDs.
    #[arg(long)]
    pub before: Option<String>,

    /// Restrict --before to a specific connector (e.g. slack, gmail)
    #[arg(long)]
    pub connector: Option<String>,
}

pub async fn run(args: &ArchiveArgs) -> anyhow::Result<()> {
    if args.before.is_some() && !args.message_ids.is_empty() {
        anyhow::bail!("--before cannot be combined with positional message IDs");
    }

    debug!(count = args.message_ids.len(), before = ?args.before, "archive");
    let cfg = crate::context::config();
    let db = crate::context::open_db()?;
    let store_path = crate::context::store_path();

    let params = ArchiveParams {
        message_ids: &args.message_ids,
        before: args.before.as_deref(),
        connector: args.connector.as_deref(),
    };

    let value = writes::archive(&db, cfg, &store_path, params).await?;
    println!("{}", crate::service::render(&value)?);
    Ok(())
}
