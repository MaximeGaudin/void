use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::MessageContent;

use crate::commands::connector_factory;
use crate::output::parse_connector_type;

#[derive(Debug, Args)]
pub struct ReplyArgs {
    /// Message ID to reply to
    pub message_id: String,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Reply in thread (Slack) or as quote (WhatsApp)
    #[arg(long)]
    pub in_thread: bool,
}

pub async fn run(args: &ReplyArgs) -> anyhow::Result<()> {
    info!(message_id = %args.message_id, "reply");
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

    debug!("message and conversation found");

    let account = cfg
        .find_account_by_connector(&msg.connector)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} account found in config.toml for message {}",
                msg.connector,
                msg.id
            )
        })?;

    let connector_type = parse_connector_type(&account.account_type.to_string())
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", account.account_type))?;

    let store_path = cfg.store_path();
    let conn = connector_factory::build_connector(account, &store_path)?;

    let reply_id = build_reply_id(connector_type, &conv.external_id, &msg.external_id);

    let content = MessageContent::Text(args.message.clone());
    let sent_id = conn.reply(&reply_id, content, args.in_thread).await?;

    eprintln!("Reply sent (id: {sent_id})");
    Ok(())
}

fn build_reply_id(
    connector_type: void_core::models::ConnectorType,
    conv_external_id: &str,
    msg_external_id: &str,
) -> String {
    use void_core::models::ConnectorType;
    match connector_type {
        ConnectorType::WhatsApp => format!("{conv_external_id}:{msg_external_id}"),
        ConnectorType::Slack => format!("{conv_external_id}:{msg_external_id}"),
        ConnectorType::Gmail => msg_external_id.to_string(),
        ConnectorType::Calendar => msg_external_id.to_string(),
    }
}
