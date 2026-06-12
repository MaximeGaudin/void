use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::VoidConfig;
use void_core::models::ConnectorType;

#[derive(Debug, Args)]
pub struct WhatsAppArgs {
    #[command(subcommand)]
    pub command: WhatsAppCommand,
}

#[derive(Debug, Subcommand)]
pub enum WhatsAppCommand {
    /// Download media from a WhatsApp message (requires active sync connection)
    Download(DownloadArgs),
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Message ID (void internal ID or external ID)
    pub message_id: String,
    /// Output file path
    #[arg(long)]
    pub out: String,
    /// WhatsApp connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

pub async fn run(args: &WhatsAppArgs) -> anyhow::Result<()> {
    match &args.command {
        WhatsAppCommand::Download(a) => run_download(a).await,
    }
}

async fn run_download(args: &DownloadArgs) -> anyhow::Result<()> {
    let cfg = crate::context::void_config();

    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    if msg.connector != "whatsapp" {
        anyhow::bail!(
            "Message {} is from connector '{}', not whatsapp.",
            args.message_id,
            msg.connector
        );
    }

    let meta = msg
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Message has no media metadata."))?;

    let direct_path = meta["direct_path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No direct_path in metadata — not a media message."))?;
    let media_key = meta["media_key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No media_key in metadata."))?;
    let file_sha256 = meta["file_sha256"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No file_sha256 in metadata."))?;
    let file_enc_sha256 = meta["file_enc_sha256"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No file_enc_sha256 in metadata."))?;
    let file_length = meta["file_size"].as_u64().unwrap_or(0);
    let media_type = meta["media_type"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No media_type in metadata."))?;

    let connector = build_wa_connector(args.connection.as_deref(), cfg)?;

    eprintln!(
        "Downloading {} ({} bytes) from WhatsApp...",
        media_type, file_length
    );

    let data = connector
        .download_media(
            direct_path,
            media_key,
            file_sha256,
            file_enc_sha256,
            file_length,
            media_type,
        )
        .await?;

    crate::commands::write_download(&args.out, &data)?;
    eprintln!("Saved to {} ({} bytes).", args.out, data.len());
    Ok(())
}

fn build_wa_connector(
    connection_filter: Option<&str>,
    cfg: &VoidConfig,
) -> anyhow::Result<void_whatsapp::connector::WhatsAppConnector> {
    let connection = cfg
        .connections
        .iter()
        .find(|a| {
            let is_wa = a.connector_type == ConnectorType::WhatsApp;
            let name_matches = connection_filter.map_or(true, |n| a.id == n);
            is_wa && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No WhatsApp connection found in config. Run `void setup` to add one.")
        })?;

    let store_path = crate::context::store_path();
    let session_db = store_path.join(format!("whatsapp-{}.db", connection.id));
    debug!(connection_id = %connection.id, "building WhatsApp connector for CLI");
    Ok(void_whatsapp::connector::WhatsAppConnector::new(
        &connection.id,
        session_db.to_str().unwrap_or(""),
    ))
}
