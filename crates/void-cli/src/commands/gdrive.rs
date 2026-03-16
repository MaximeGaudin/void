use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::{self, expand_tilde, AccountSettings, AccountType, VoidConfig};
use void_gdrive::api::{self, DriveApiClient, ExportFormat};

#[derive(Debug, Args)]
pub struct GdriveArgs {
    #[command(subcommand)]
    pub command: GdriveCommand,
}

#[derive(Debug, Subcommand)]
pub enum GdriveCommand {
    /// Download a file from Google Drive/Docs/Sheets/Slides
    Download(DownloadArgs),
    /// Show metadata for a Google Drive file
    Info(InfoArgs),
    /// Authenticate with Google Drive (request drive.readonly scope)
    Auth(AuthArgs),
}

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// Google Drive/Docs/Sheets/Slides URL or file ID
    pub url: String,
    /// Output file path (auto-named from file metadata if omitted)
    #[arg(long, short)]
    pub output: Option<String>,
    /// Export format for Google-native files: txt, md, pdf, docx, csv, xlsx, pptx, png, svg
    #[arg(long, short)]
    pub format: Option<String>,
    /// Google account to use (defaults to first gmail/calendar/gdrive account)
    #[arg(long)]
    pub account: Option<String>,
    /// Print content to stdout instead of saving to file
    #[arg(long)]
    pub stdout: bool,
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Google Drive/Docs/Sheets/Slides URL or file ID
    pub url: String,
    /// Google account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct AuthArgs {
    /// Google account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &GdriveArgs, json: bool) -> anyhow::Result<()> {
    match &args.command {
        GdriveCommand::Download(dl) => run_download(dl, json).await,
        GdriveCommand::Info(info) => run_info(info, json).await,
        GdriveCommand::Auth(auth) => run_auth(auth).await,
    }
}

async fn run_download(args: &DownloadArgs, json: bool) -> anyhow::Result<()> {
    let (client, _cfg) = build_drive_client(args.account.as_deref())?;
    let client = client.await?;

    let file_id = resolve_file_id(&args.url)?;
    debug!(file_id = %file_id, "gdrive download");

    let format = args
        .format
        .as_deref()
        .map(|f| {
            ExportFormat::from_name(f).ok_or_else(|| {
                anyhow::anyhow!(
                    "unknown format \"{f}\". Valid: txt, md, pdf, docx, csv, xlsx, pptx, png, svg"
                )
            })
        })
        .transpose()?;

    let result = client.fetch_file(&file_id, format).await?;

    if args.stdout {
        use std::io::Write;
        std::io::stdout().write_all(&result.data)?;
        return Ok(());
    }

    let output_path = args.output.as_deref().map(std::path::Path::new);
    let dest = DriveApiClient::save_to_disk(&result, output_path)?;

    if json {
        let out = serde_json::json!({
            "data": {
                "file": dest.display().to_string(),
                "name": result.file_name,
                "mime_type": result.mime_type,
                "size": result.data.len(),
                "export_format": result.export_format.map(|f| f.extension()),
            },
            "error": null,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let size = result.data.len();
        let human_size = if size < 1024 {
            format!("{size} B")
        } else if size < 1024 * 1024 {
            format!("{:.1} KB", size as f64 / 1024.0)
        } else {
            format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
        };
        eprintln!(
            "Downloaded \"{}\" ({}) → {}",
            result.file_name,
            human_size,
            dest.display()
        );
    }

    Ok(())
}

async fn run_info(args: &InfoArgs, json: bool) -> anyhow::Result<()> {
    let (client, _cfg) = build_drive_client(args.account.as_deref())?;
    let client = client.await?;

    let file_id = resolve_file_id(&args.url)?;
    let meta = client.get_file_metadata(&file_id).await?;

    let is_native = api::is_google_native_mime(&meta.mime_type);
    let (text_fmt, bin_fmt) = if is_native {
        let (t, b) = api::default_export_formats(&meta.mime_type);
        (Some(t.extension()), Some(b.extension()))
    } else {
        (None, None)
    };

    if json {
        let out = serde_json::json!({
            "data": {
                "id": meta.id,
                "name": meta.name,
                "mime_type": meta.mime_type,
                "size": meta.size,
                "google_native": is_native,
                "default_text_format": text_fmt,
                "default_binary_format": bin_fmt,
            },
            "error": null,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        eprintln!("  Name: {}", meta.name);
        eprintln!("  ID:   {}", meta.id);
        eprintln!("  Type: {}", meta.mime_type);
        if let Some(size) = &meta.size {
            eprintln!("  Size: {} bytes", size);
        }
        if is_native {
            eprintln!("  Google native: yes");
            eprintln!(
                "  Default export: {} (text), {} (binary)",
                text_fmt.unwrap_or("n/a"),
                bin_fmt.unwrap_or("n/a")
            );
        }
    }

    Ok(())
}

async fn run_auth(args: &AuthArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("cannot load config: {e}\nRun `void setup` first."))?;

    let account = find_google_account(&cfg, args.account.as_deref())?;
    let credentials_file = extract_credentials_file(&account.settings);
    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();

    api::authenticate_drive(
        &store_path,
        &account.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
    )
    .await?;

    eprintln!("Google Drive authenticated for account \"{}\".", account.id);
    Ok(())
}

/// Resolve a URL or bare file ID to a file ID string.
fn resolve_file_id(url_or_id: &str) -> anyhow::Result<String> {
    if url_or_id.starts_with("http://") || url_or_id.starts_with("https://") {
        let parsed = api::parse_google_url(url_or_id)?;
        Ok(parsed.file_id)
    } else {
        Ok(url_or_id.to_string())
    }
}

/// Future type for building a Drive API client.
type DriveClientFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<DriveApiClient>> + Send>>;

/// Build a Drive API client future and config from the user's stored tokens.
fn build_drive_client(
    account_filter: Option<&str>,
) -> anyhow::Result<(DriveClientFuture, VoidConfig)> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("cannot load config: {e}\nRun `void setup` first."))?;

    let account = find_google_account(&cfg, account_filter)?;
    let credentials_file = extract_credentials_file(&account.settings);
    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();
    let account_id = account.id.clone();

    let fut = Box::pin(async move {
        api::build_drive_client(
            &store_path,
            &account_id,
            cred_path.as_deref().and_then(|p| p.to_str()),
        )
        .await
        .map_err(Into::into)
    });

    Ok((fut, cfg))
}

/// Find the first Google account (gmail, calendar, or gdrive) matching the filter.
fn find_google_account<'a>(
    cfg: &'a VoidConfig,
    filter: Option<&str>,
) -> anyhow::Result<&'a void_core::config::AccountConfig> {
    let google_types = [AccountType::Gmail, AccountType::Calendar];
    cfg.accounts
        .iter()
        .find(|a| {
            let is_google = google_types.contains(&a.account_type);
            let name_matches = filter.map_or(true, |n| a.id == n);
            is_google && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Google account (gmail/calendar) found in config. Run `void setup` to add one."
            )
        })
}

fn extract_credentials_file(settings: &AccountSettings) -> Option<String> {
    match settings {
        AccountSettings::Gmail { credentials_file } => credentials_file.clone(),
        AccountSettings::Calendar {
            credentials_file, ..
        } => credentials_file.clone(),
        _ => None,
    }
}
