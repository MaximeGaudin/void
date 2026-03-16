use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use crate::commands::connector_factory;

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
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let db = Database::open(&cfg.db_path())?;

    let msg = db
        .get_message(&args.message_id)?
        .ok_or_else(|| anyhow::anyhow!("Message not found: {}", args.message_id))?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    debug!(connector = %msg.connector, account_id = %msg.account_id, "resolved message");

    let account = cfg
        .find_account(&msg.account_id)
        .or_else(|| cfg.find_account_by_connector(&msg.connector))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} account found in config for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let store_path = cfg.store_path();
    let conn = connector_factory::build_connector(account, &store_path)?;

    let fwd_id = conn
        .forward(
            &msg.external_id,
            &conv.external_id,
            &args.to,
            args.comment.as_deref(),
        )
        .await?;

    eprintln!("Message forwarded (id: {fwd_id})");
    Ok(())
}
