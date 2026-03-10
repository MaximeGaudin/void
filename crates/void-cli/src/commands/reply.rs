use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, VoidConfig};
use void_core::db::Database;
use void_core::models::MessageContent;

use crate::commands::channel_factory;
use crate::output::parse_channel_type;

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
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void config init` first."))?;

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

    let channel_type = parse_channel_type(&account.account_type.to_string())
        .ok_or_else(|| anyhow::anyhow!("Unknown channel type: {}", account.account_type))?;

    let store_path = cfg.store_path();
    let channel = channel_factory::build_channel(account, &store_path)?;

    let reply_id = build_reply_id(channel_type, &conv.external_id, &msg.external_id);

    let content = MessageContent::Text(args.message.clone());
    let sent_id = channel.reply(&reply_id, content, args.in_thread).await?;

    eprintln!("Reply sent (id: {sent_id})");
    Ok(())
}

fn build_reply_id(
    channel_type: void_core::models::ChannelType,
    conv_external_id: &str,
    msg_external_id: &str,
) -> String {
    use void_core::models::ChannelType;
    match channel_type {
        ChannelType::WhatsApp => format!("{conv_external_id}:{msg_external_id}"),
        ChannelType::Slack => format!("{conv_external_id}:{msg_external_id}"),
        ChannelType::Gmail => msg_external_id.to_string(),
        ChannelType::Calendar => msg_external_id.to_string(),
    }
}
