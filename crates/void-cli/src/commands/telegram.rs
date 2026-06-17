use clap::{Args, Subcommand};
use void_core::connector::Connector;

use crate::commands::connector_factory::build_telegram_connector;

#[derive(Debug, Args)]
pub struct TelegramArgs {
    #[command(subcommand)]
    pub command: TelegramCommand,
}

#[derive(Debug, Subcommand)]
pub enum TelegramCommand {
    /// Download media from a Telegram message
    Download(DownloadArgs),
    /// Forward a message to another chat
    Forward(ForwardArgs),
}

#[derive(Debug, Args)]
pub struct ForwardArgs {
    /// Message ID (void internal ID or external ID)
    pub message_id: String,
    /// Target chat ID, phone number, or username
    #[arg(long)]
    pub to: String,
    /// Optional comment (note: currently ignored by Telegram forwarding)
    #[arg(long)]
    pub comment: Option<String>,
    /// Telegram connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Message ID (void internal ID or external ID)
    pub message_id: String,
    /// Output file path
    #[arg(long)]
    pub out: String,
    /// Telegram connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

pub async fn run(args: &TelegramArgs) -> anyhow::Result<()> {
    match &args.command {
        TelegramCommand::Download(a) => run_download(a).await,
        TelegramCommand::Forward(a) => run_forward(a).await,
    }
}

async fn run_download(args: &DownloadArgs) -> anyhow::Result<()> {
    let cfg = crate::context::void_config();

    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    if msg.connector != "telegram" {
        anyhow::bail!(
            "Message {} is from connector '{}', not telegram.",
            args.message_id,
            msg.connector
        );
    }

    let meta = msg
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Message has no media metadata."))?;

    let document_id = meta["document_id"]
        .as_i64()
        .or_else(|| meta["photo_id"].as_i64());

    if document_id.is_none() {
        anyhow::bail!("No downloadable media in metadata.");
    }

    let connector = build_telegram_connector(args.connection.as_deref(), cfg)?;

    let raw_msg_id_str = msg.external_id.rsplit('_').next().unwrap_or("0");
    let raw_msg_id: i32 = raw_msg_id_str.parse().unwrap_or(0);

    let raw_chat_id: i64 = msg
        .external_id
        .split('_')
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    eprintln!("Downloading media from Telegram...");

    let data = connector.download_media(raw_msg_id, raw_chat_id).await?;

    crate::commands::write_download(&args.out, &data)?;
    eprintln!("Saved to {} ({} bytes).", args.out, data.len());
    Ok(())
}

async fn run_forward(args: &ForwardArgs) -> anyhow::Result<()> {
    let cfg = crate::context::void_config();

    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    super::resolve::check_forward_connector(&args.message_id, &msg.connector, "telegram")?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let conn_id =
        super::resolve::resolve_forward_connection(args.connection.as_deref(), &msg.connection_id);
    let connector = build_telegram_connector(Some(conn_id), cfg)?;

    let fwd_id = connector
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
