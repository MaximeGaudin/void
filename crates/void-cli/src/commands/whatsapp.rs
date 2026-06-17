use clap::{Args, Subcommand};
use void_core::sync::is_daemon_running;

use crate::commands::connector_factory::build_whatsapp_connector_for_cli;

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

    let connector = build_whatsapp_connector_for_cli(args.connection.as_deref(), cfg)?;
    let store_path = crate::context::store_path();
    let connection_id = connector.connection_id();

    eprintln!(
        "Downloading {} ({} bytes) from WhatsApp...",
        media_type, file_length
    );

    let data = if is_daemon_running(&store_path) {
        void_whatsapp::rpc::download_media(
            &store_path,
            connection_id,
            void_whatsapp::rpc::RpcDownloadParams {
                direct_path: direct_path.to_string(),
                media_key: media_key.to_string(),
                file_sha256: file_sha256.to_string(),
                file_enc_sha256: file_enc_sha256.to_string(),
                file_length,
                media_type: media_type.to_string(),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
    } else {
        connector
            .download_media(
                direct_path,
                media_key,
                file_sha256,
                file_enc_sha256,
                file_length,
                media_type,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
    };

    crate::commands::write_download(&args.out, &data)?;
    eprintln!("Saved to {} ({} bytes).", args.out, data.len());
    Ok(())
}
