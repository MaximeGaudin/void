use clap::Args;
use tracing::{debug, info};

use void_core::config::{self, VoidConfig};
use void_core::models::MessageContent;

use crate::commands::connector_factory;
use crate::output::parse_connector_type;

#[derive(Debug, Args)]
pub struct SendArgs {
    /// Recipient (phone number, channel name, email)
    #[arg(long)]
    pub to: String,
    /// Connector to send via: whatsapp, slack, gmail
    #[arg(long)]
    pub via: String,
    /// Account to use (for multi-account connectors)
    #[arg(long)]
    pub account: Option<String>,
    /// Message text
    #[arg(long)]
    pub message: String,
    /// Email subject (gmail only)
    #[arg(long)]
    pub subject: Option<String>,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
}

pub async fn run(args: &SendArgs) -> anyhow::Result<()> {
    info!(via = %args.via, to = %args.to, "send");
    let connector_type = parse_connector_type(&args.via)
        .ok_or_else(|| anyhow::anyhow!("Unknown connector type: {}", args.via))?;

    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let target_type = connector_type.to_string();
    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let type_matches = a.account_type.to_string() == target_type;
            let name_matches = args.account.as_ref().map_or(true, |n| a.id == *n);
            type_matches && name_matches
        })
        .ok_or_else(|| anyhow::anyhow!("No {target_type} account found in config.toml"))?;

    let store_path = cfg.store_path();
    let conn = connector_factory::build_connector(account, &store_path)?;
    debug!("connector built");

    let content = if let Some(ref path) = args.file {
        MessageContent::File {
            path: path.into(),
            caption: Some(args.message.clone()),
            mime_type: None,
        }
    } else {
        MessageContent::Text(args.message.clone())
    };

    let msg_id = conn.send_message(&args.to, content).await?;
    eprintln!("Message sent (id: {msg_id})");
    Ok(())
}
