use clap::Args;
use tracing::info;

use crate::service::writes::{self, ForwardParams};

#[derive(Debug, Args)]
pub struct ForwardArgs {
    /// Message ID to forward
    pub message_id: String,
    /// Recipient (email address, Slack channel/user ID, etc.)
    #[arg(long)]
    pub to: String,
    /// Optional comment to include above the forwarded message
    #[arg(long)]
    pub comment: Option<String>,
}

pub async fn run(args: &ForwardArgs) -> anyhow::Result<()> {
    info!(message_id = %args.message_id, to = %args.to, "forward");
    let cfg = crate::context::void_config();
    let db = crate::context::open_db()?;
    let store_path = crate::context::store_path();

    let params = ForwardParams {
        message_id: &args.message_id,
        to: &args.to,
        comment: args.comment.as_deref(),
    };

    let fwd_id = writes::forward(&db, cfg, &store_path, params).await?;
    eprintln!("Message forwarded (id: {fwd_id})");
    Ok(())
}
