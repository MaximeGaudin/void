use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::settings_string;
use void_core::models::ConnectorType;

#[derive(Debug, Args)]
pub struct LinkedInArgs {
    #[command(subcommand)]
    pub command: LinkedInCommand,
}

#[derive(Debug, Subcommand)]
pub enum LinkedInCommand {
    /// Download media from a LinkedIn message
    Download(DownloadArgs),
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Message ID (void internal ID or external ID)
    pub message_id: String,
    /// Output file path
    #[arg(long)]
    pub out: String,
    /// LinkedIn connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

pub async fn run(args: &LinkedInArgs) -> anyhow::Result<()> {
    match &args.command {
        LinkedInCommand::Download(a) => run_download(a).await,
    }
}

async fn run_download(args: &DownloadArgs) -> anyhow::Result<()> {
    let cfg = crate::context::void_config();

    let db = crate::context::open_db()?;

    let msg = super::resolve::resolve_message(&db, &args.message_id)?;

    if msg.connector != "linkedin" {
        anyhow::bail!(
            "Message {} is from connector '{}', not linkedin.",
            args.message_id,
            msg.connector
        );
    }

    let meta = msg
        .metadata
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Message has no media metadata."))?;

    let unipile_message_id = meta["message_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing message_id in metadata."))?;
    let attachment_id = meta["attachment_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing attachment_id in metadata."))?;

    let connection = if let Some(ref id) = args.connection {
        cfg.find_connection(id)
            .ok_or_else(|| anyhow::anyhow!("Connection '{id}' not found."))?
    } else {
        cfg.find_connection_by_connector("linkedin")
            .ok_or_else(|| anyhow::anyhow!("No LinkedIn connection configured."))?
    };

    if connection.connector_type != ConnectorType::from_static(void_linkedin::CONNECTOR_ID) {
        anyhow::bail!(
            "Connection '{}' is not a LinkedIn connection.",
            connection.id
        );
    }

    let api_key = settings_string(&connection.settings, "api_key")
        .ok_or_else(|| anyhow::anyhow!("missing api_key"))?;
    let dsn = settings_string(&connection.settings, "dsn")
        .ok_or_else(|| anyhow::anyhow!("missing dsn"))?;
    let account_id = settings_string(&connection.settings, "account_id")
        .ok_or_else(|| anyhow::anyhow!("missing account_id"))?;

    let connector = void_linkedin::connector::LinkedInConnector::new(
        &connection.id,
        &api_key,
        &dsn,
        &account_id,
        cfg.sync
            .poll_interval_secs(void_linkedin::CONNECTOR_ID, 30 * 60),
        cfg.sync.linkedin_backfill_days(),
    );

    debug!(
        message_id = %unipile_message_id,
        attachment_id,
        "downloading LinkedIn attachment"
    );

    let bytes = connector
        .download_media(unipile_message_id, attachment_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    crate::commands::write_download(&args.out, &bytes)?;
    eprintln!("Saved {} bytes to {}", bytes.len(), args.out);
    Ok(())
}
