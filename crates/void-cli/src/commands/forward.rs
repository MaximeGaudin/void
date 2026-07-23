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
    /// Append the account's Gmail signature (gmail only).
    /// Pass a comment/body without an existing signature — re-appending doubles it.
    #[arg(long)]
    pub signature: bool,
    /// Send-as alias whose signature to use (requires --signature; gmail only).
    #[arg(long, requires = "signature")]
    pub signature_from: Option<String>,
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
        signature: args.signature,
        signature_from: args.signature_from.as_deref(),
    };

    let fwd_id = writes::forward(&db, cfg, &store_path, params).await?;
    eprintln!("Message forwarded (id: {fwd_id})");
    Ok(())
}
