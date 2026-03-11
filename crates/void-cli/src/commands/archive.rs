use std::collections::HashMap;
use std::sync::Arc;

use clap::Args;
use tracing::{debug, info, warn};

use void_core::config::{self, VoidConfig};
use void_core::connector::Connector;
use void_core::db::Database;

use super::connector_factory;

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Message IDs to archive (one or more)
    pub message_ids: Vec<String>,
}

pub async fn run(args: &ArchiveArgs, json: bool) -> anyhow::Result<()> {
    if args.message_ids.is_empty() {
        anyhow::bail!("at least one message ID is required");
    }

    debug!(count = args.message_ids.len(), "bulk archive");
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;

    let mut connectors: HashMap<String, Arc<dyn Connector>> = HashMap::new();
    let mut results = Vec::new();

    for message_id in &args.message_ids {
        let msg = match db.get_message(message_id)? {
            Some(m) => m,
            None => {
                warn!(message_id, "message not found, skipping");
                results.push(serde_json::json!({
                    "message_id": message_id,
                    "is_archived": false,
                    "error": "message not found",
                }));
                continue;
            }
        };

        let conv = match db.get_conversation(&msg.conversation_id)? {
            Some(c) => c,
            None => {
                warn!(message_id, conversation_id = %msg.conversation_id, "conversation not found, skipping");
                results.push(serde_json::json!({
                    "message_id": message_id,
                    "is_archived": false,
                    "error": "conversation not found",
                }));
                continue;
            }
        };

        let connector_key = format!("{}:{}", msg.connector, msg.account_id);
        if !connectors.contains_key(&connector_key) {
            if let Some(account) = cfg
                .find_account(&msg.account_id)
                .or_else(|| cfg.find_account_by_connector(&msg.connector))
            {
                match connector_factory::build_connector(account, &cfg.store_path()) {
                    Ok(c) => {
                        connectors.insert(connector_key.clone(), c);
                    }
                    Err(e) => {
                        warn!(connector_key, error = %e, "failed to build connector");
                    }
                }
            }
        }

        let remote_synced = if let Some(conn) = connectors.get(&connector_key) {
            match conn.archive(&msg.external_id, &conv.external_id).await {
                Ok(()) => true,
                Err(e) => {
                    warn!(message_id, error = %e, "remote archive failed, local only");
                    false
                }
            }
        } else {
            false
        };

        db.mark_message_archived(message_id)?;
        info!(message_id, remote_synced, "archived");

        results.push(serde_json::json!({
            "message_id": message_id,
            "is_archived": true,
            "remote_synced": remote_synced,
        }));
    }

    if json {
        let output = serde_json::json!({ "data": results, "error": null });
        println!("{}", serde_json::to_string(&output)?);
    } else {
        let output = serde_json::json!({ "data": results, "error": null });
        println!("{}", serde_json::to_string_pretty(&output)?);
    }
    Ok(())
}
