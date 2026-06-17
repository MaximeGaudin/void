use std::path::Path;

use crate::error::DriveError;
use tracing::debug;

use super::types::{DownloadResult, ExportFormat, FileMetadata};
use super::url::{
    default_export_formats, is_google_native_mime, sanitize_download_name, urlencoded,
};

const DEFAULT_BASE_URL: &str = "https://www.googleapis.com";

pub const DRIVE_SCOPES: &str = "https://www.googleapis.com/auth/drive.readonly";

/// Google Drive API client.
pub struct DriveApiClient {
    http: reqwest::Client,
    access_token: String,
    base_url: String,
}

impl DriveApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: void_gmail::api::build_http_client(),
            access_token: access_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(access_token: &str, base_url: &str) -> Self {
        Self {
            http: void_gmail::api::build_http_client(),
            access_token: access_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    /// Fetch file metadata from Google Drive.
    pub async fn get_file_metadata(&self, file_id: &str) -> anyhow::Result<FileMetadata> {
        debug!(file_id, "gdrive: get_file_metadata");
        let url = format!("{}/drive/v3/files/{}", self.base_url, urlencoded(file_id));
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("fields", "id,name,mimeType,size")])
            .send()
            .await?;
        let resp = check_response(resp).await?;

        let meta: FileMetadata = resp.json().await?;
        debug!(file_id, name = %meta.name, mime = %meta.mime_type, "gdrive: metadata ok");
        Ok(meta)
    }

    /// Download a binary (non-Google-native) file from Drive.
    pub async fn download_file(&self, file_id: &str) -> anyhow::Result<Vec<u8>> {
        debug!(file_id, "gdrive: download_file");
        let url = format!("{}/drive/v3/files/{}", self.base_url, urlencoded(file_id));
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("alt", "media")])
            .send()
            .await?;
        let resp = check_response(resp).await?;

        let bytes = resp.bytes().await?;
        debug!(file_id, size = bytes.len(), "gdrive: download ok");
        Ok(bytes.to_vec())
    }

    /// Export a Google-native file (Docs/Sheets/Slides/Drawings) to a specific format.
    pub async fn export_file(&self, file_id: &str, export_mime: &str) -> anyhow::Result<Vec<u8>> {
        debug!(file_id, export_mime, "gdrive: export_file");
        let url = format!(
            "{}/drive/v3/files/{}/export",
            self.base_url,
            urlencoded(file_id)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("mimeType", export_mime)])
            .send()
            .await?;
        let resp = check_response(resp).await?;

        let bytes = resp.bytes().await?;
        debug!(file_id, size = bytes.len(), "gdrive: export ok");
        Ok(bytes.to_vec())
    }

    /// High-level: fetch metadata, then download or export the file.
    /// For Google-native formats, `format` selects the export format.
    /// If `format` is None, defaults to text for Google Docs/Sheets and PDF for presentations.
    pub async fn fetch_file(
        &self,
        file_id: &str,
        format: Option<ExportFormat>,
    ) -> anyhow::Result<DownloadResult> {
        let meta = self.get_file_metadata(file_id).await?;

        if is_google_native_mime(&meta.mime_type) {
            let (text_default, _binary_default) = default_export_formats(&meta.mime_type);
            let export_fmt = format.unwrap_or(text_default);
            let data = self.export_file(file_id, export_fmt.mime_type()).await?;

            Ok(DownloadResult {
                file_name: meta.name,
                mime_type: export_fmt.mime_type().to_string(),
                data,
                export_format: Some(export_fmt),
            })
        } else {
            if format.is_some() {
                anyhow::bail!(
                    "file \"{}\" is not a Google-native format ({}); \
                     --format only applies to Google Docs/Sheets/Slides",
                    meta.name,
                    meta.mime_type,
                );
            }
            let data = self.download_file(file_id).await?;
            Ok(DownloadResult {
                file_name: meta.name,
                mime_type: meta.mime_type,
                data,
                export_format: None,
            })
        }
    }

    /// Save a download result to disk, auto-naming based on file metadata.
    pub fn save_to_disk(
        result: &DownloadResult,
        output: Option<&Path>,
    ) -> Result<std::path::PathBuf, DriveError> {
        let dest = if let Some(path) = output {
            path.to_path_buf()
        } else {
            // `file_name` comes straight from the Drive API and is fully
            // attacker-controlled (anyone who shares a file picks its name).
            // Reduce it to a single safe component so a name like
            // `../../../.zshrc` cannot escape the working directory.
            let name = sanitize_download_name(&result.file_name);
            if let Some(fmt) = &result.export_format {
                let stem = Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&name);
                std::path::PathBuf::from(format!("{}.{}", stem, fmt.extension()))
            } else {
                std::path::PathBuf::from(&name)
            }
        };

        if let Some(parent) = dest.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(&dest, &result.data)?;
        Ok(dest)
    }
}

/// Check HTTP response status, extracting the Google API error message on failure.
/// Produces actionable hints for common errors (missing scopes, Drive API not enabled).
async fn check_response(resp: reqwest::Response) -> Result<reqwest::Response, DriveError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let detail = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str().map(|s| s.to_string()))
        })
        .unwrap_or(body);

    let lower = detail.to_lowercase();
    if status == reqwest::StatusCode::FORBIDDEN
        && lower.contains("insufficient authentication scopes")
    {
        return Err(DriveError::Auth(
            "your current token does not include Google Drive scopes. \
             Run `void drive auth` to authorize Drive access."
                .into(),
        ));
    }
    if (status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND)
        && lower.contains("drive api")
        && lower.contains("not been used")
    {
        return Err(DriveError::Auth(
            "the Google Drive API is not enabled for your Cloud project. \
             Enable it at: https://console.cloud.google.com/apis/library/drive.googleapis.com \
             Then run `void drive auth`."
                .into(),
        ));
    }

    Err(DriveError::Api(format!(
        "Google API error ({status}): {detail}"
    )))
}

/// Token path dedicated to Drive (avoids overwriting Gmail/Calendar tokens).
pub fn drive_token_cache_path(store_path: &Path, connection_id: &str) -> std::path::PathBuf {
    store_path.join(format!("{connection_id}-drive-token.json"))
}

/// Create a Drive API client. Tries the Drive-specific token first, then falls
/// back to the shared Gmail token (which may already have sufficient scopes).
pub async fn build_drive_client(
    store_path: &Path,
    connection_id: &str,
    credentials_file: Option<&str>,
) -> Result<DriveApiClient, DriveError> {
    let drive_path = drive_token_cache_path(store_path, connection_id);
    let gmail_path = void_gmail::auth::token_cache_path(store_path, connection_id);

    let token_path = if drive_path.exists() {
        drive_path
    } else if gmail_path.exists() {
        gmail_path
    } else {
        return Err(DriveError::Auth(format!(
            "no Google token found for connection \"{connection_id}\". \
             Run `void drive auth --connection {connection_id}` first."
        )));
    };

    let mut cache = void_gmail::auth::TokenCache::load(&token_path)
        .map_err(|e| DriveError::Auth(e.to_string()))?;

    let is_expired = cache
        .expires_at
        .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
        .unwrap_or(true);

    if is_expired {
        debug!(connection_id, "refreshing Drive access token");
        if let Some(ref refresh_token) = cache.refresh_token {
            let creds = void_gmail::auth::load_client_credentials(credentials_file)
                .map_err(|e| DriveError::Auth(e.to_string()))?;
            let http = void_gmail::api::build_http_client();
            cache = void_gmail::auth::refresh_access_token(&http, &creds, refresh_token)
                .await
                .map_err(|e| DriveError::Auth(e.to_string()))?;
            cache
                .save(&token_path)
                .map_err(|e| DriveError::Auth(e.to_string()))?;
        } else {
            return Err(DriveError::Auth(
                "token expired and no refresh token. Run `void drive auth`".into(),
            ));
        }
    }

    Ok(DriveApiClient::new(&cache.access_token))
}

/// Run the interactive OAuth flow for Drive scopes.
/// Saves to a Drive-specific token file so Gmail/Calendar tokens are not overwritten.
pub async fn authenticate_drive(
    store_path: &Path,
    connection_id: &str,
    credentials_file: Option<&str>,
) -> Result<(), DriveError> {
    let creds = void_gmail::auth::load_client_credentials(credentials_file)
        .map_err(|e| DriveError::Auth(e.to_string()))?;
    let token_path = drive_token_cache_path(store_path, connection_id);
    let cache = void_gmail::auth::authorize_interactive(&creds, Some(DRIVE_SCOPES))
        .await
        .map_err(|e| DriveError::Auth(e.to_string()))?;
    cache
        .save(&token_path)
        .map_err(|e| DriveError::Auth(e.to_string()))?;
    Ok(())
}
