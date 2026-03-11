use clap::Args;
use tracing::{debug, info, warn};

use void_core::config::{self, VoidConfig};
use void_core::db::Database;

use super::connector_factory;

#[derive(Debug, Args)]
pub struct ReadArgs {
    /// Message ID to mark as read
    pub message_id: String,
}

pub async fn run(args: &ReadArgs) -> anyhow::Result<()> {
    debug!(message_id = %args.message_id, "read");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;

    let msg = db
        .get_message(&args.message_id)?
        .ok_or_else(|| anyhow::anyhow!("message not found: {}", args.message_id))?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("conversation not found: {}", msg.conversation_id))?;

    let account = cfg
        .find_account_by_connector(&msg.connector)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no {} account found in config.toml for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let conn = connector_factory::build_connector(account, &cfg.store_path())?;

    let remote_synced = match conn.mark_read(&msg.external_id, &conv.external_id).await {
        Ok(()) => true,
        Err(e) => {
            warn!(
                message_id = %args.message_id,
                account_id = %msg.account_id,
                error = %e,
                "remote mark_read failed, updating local state only"
            );
            false
        }
    };

    db.mark_message_read(&args.message_id)?;

    info!(message_id = %args.message_id, remote_synced, "message marked as read");
    let result = serde_json::json!({
        "data": {
            "message_id": args.message_id,
            "is_read": true,
            "remote_synced": remote_synced,
        },
        "error": null,
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
