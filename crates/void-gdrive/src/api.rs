use std::path::Path;

use anyhow::Context;
use serde::Deserialize;
use tracing::debug;

const DEFAULT_BASE_URL: &str = "https://www.googleapis.com";

pub const DRIVE_SCOPES: &str = "https://www.googleapis.com/auth/drive.readonly";

/// Google Drive API client.
pub struct DriveApiClient {
    http: reqwest::Client,
    access_token: String,
    base_url: String,
}

/// Metadata for a Google Drive file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    #[serde(default)]
    pub size: Option<String>,
}

/// The kind of Google resource identified from a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoogleFileKind {
    Document,
    Spreadsheet,
    Presentation,
    Drawing,
    Drive,
}

impl std::fmt::Display for GoogleFileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Document => write!(f, "document"),
            Self::Spreadsheet => write!(f, "spreadsheet"),
            Self::Presentation => write!(f, "presentation"),
            Self::Drawing => write!(f, "drawing"),
            Self::Drive => write!(f, "drive"),
        }
    }
}

/// Result of parsing a Google URL.
#[derive(Debug, Clone)]
pub struct ParsedGoogleUrl {
    pub file_id: String,
    pub kind: GoogleFileKind,
}

/// Parse a Google Docs/Sheets/Slides/Drive URL and extract the file ID.
///
/// Supported URL formats:
/// - `https://docs.google.com/document/d/{id}/...`
/// - `https://docs.google.com/spreadsheets/d/{id}/...`
/// - `https://docs.google.com/presentation/d/{id}/...`
/// - `https://docs.google.com/drawings/d/{id}/...`
/// - `https://drive.google.com/file/d/{id}/...`
/// - `https://drive.google.com/open?id={id}`
pub fn parse_google_url(url_str: &str) -> anyhow::Result<ParsedGoogleUrl> {
    let url = url::Url::parse(url_str).context("invalid URL")?;

    let host = url.host_str().unwrap_or("");
    let path_segments: Vec<&str> = url.path_segments().map_or(vec![], |s| s.collect());

    match host {
        "docs.google.com" => {
            let kind = match path_segments.first().copied() {
                Some("document") => GoogleFileKind::Document,
                Some("spreadsheets") => GoogleFileKind::Spreadsheet,
                Some("presentation") => GoogleFileKind::Presentation,
                Some("drawings") => GoogleFileKind::Drawing,
                _ => anyhow::bail!(
                    "unrecognized docs.google.com path: {}",
                    url.path()
                ),
            };
            // Pattern: /{type}/d/{file_id}/...
            if path_segments.get(1).copied() == Some("d") {
                if let Some(id) = path_segments.get(2) {
                    if !id.is_empty() {
                        return Ok(ParsedGoogleUrl {
                            file_id: id.to_string(),
                            kind,
                        });
                    }
                }
            }
            anyhow::bail!("could not extract file ID from: {url_str}")
        }
        "drive.google.com" => {
            // Pattern: /file/d/{file_id}/...
            if path_segments.first().copied() == Some("file")
                && path_segments.get(1).copied() == Some("d")
            {
                if let Some(id) = path_segments.get(2) {
                    if !id.is_empty() {
                        return Ok(ParsedGoogleUrl {
                            file_id: id.to_string(),
                            kind: GoogleFileKind::Drive,
                        });
                    }
                }
            }
            // Pattern: /open?id={file_id}
            if let Some(id) = url.query_pairs().find(|(k, _)| k == "id").map(|(_, v)| v) {
                if !id.is_empty() {
                    return Ok(ParsedGoogleUrl {
                        file_id: id.to_string(),
                        kind: GoogleFileKind::Drive,
                    });
                }
            }
            anyhow::bail!("could not extract file ID from: {url_str}")
        }
        _ => anyhow::bail!("not a recognized Google URL (host: {host})"),
    }
}

/// Known Google Apps MIME types.
const GOOGLE_DOCS_MIME: &str = "application/vnd.google-apps.document";
const GOOGLE_SHEETS_MIME: &str = "application/vnd.google-apps.spreadsheet";
const GOOGLE_SLIDES_MIME: &str = "application/vnd.google-apps.presentation";
const GOOGLE_DRAWINGS_MIME: &str = "application/vnd.google-apps.drawing";

/// Whether a MIME type represents a native Google Apps format (needs export, not download).
pub fn is_google_native_mime(mime: &str) -> bool {
    mime.starts_with("application/vnd.google-apps.")
}

/// Supported export formats for each Google native type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    PlainText,
    Markdown,
    Pdf,
    Docx,
    Csv,
    Xlsx,
    Pptx,
    Png,
    Svg,
}

impl ExportFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::PlainText => "text/plain",
            Self::Markdown => "text/markdown",
            Self::Pdf => "application/pdf",
            Self::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            Self::Csv => "text/csv",
            Self::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Self::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Png => "image/png",
            Self::Svg => "image/svg+xml",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::PlainText => "txt",
            Self::Markdown => "md",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
            Self::Pptx => "pptx",
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "text" | "txt" | "plain" => Some(Self::PlainText),
            "markdown" | "md" => Some(Self::Markdown),
            "pdf" => Some(Self::Pdf),
            "docx" | "word" => Some(Self::Docx),
            "csv" => Some(Self::Csv),
            "xlsx" | "excel" => Some(Self::Xlsx),
            "pptx" | "powerpoint" => Some(Self::Pptx),
            "png" => Some(Self::Png),
            "svg" => Some(Self::Svg),
            _ => None,
        }
    }
}

/// Pick the best default export format based on the Google native MIME type.
/// Returns (text_format, binary_format).
pub fn default_export_formats(google_mime: &str) -> (ExportFormat, ExportFormat) {
    match google_mime {
        GOOGLE_DOCS_MIME => (ExportFormat::PlainText, ExportFormat::Pdf),
        GOOGLE_SHEETS_MIME => (ExportFormat::Csv, ExportFormat::Xlsx),
        GOOGLE_SLIDES_MIME => (ExportFormat::PlainText, ExportFormat::Pdf),
        GOOGLE_DRAWINGS_MIME => (ExportFormat::Svg, ExportFormat::Pdf),
        _ => (ExportFormat::PlainText, ExportFormat::Pdf),
    }
}

/// Download result with file content bytes and metadata.
#[derive(Debug)]
pub struct DownloadResult {
    pub file_name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub export_format: Option<ExportFormat>,
}

impl DriveApiClient {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(access_token: &str, base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            access_token: access_token.to_string(),
            base_url: base_url.to_string(),
        }
    }

    /// Fetch file metadata from Google Drive.
    pub async fn get_file_metadata(&self, file_id: &str) -> anyhow::Result<FileMetadata> {
        debug!(file_id, "gdrive: get_file_metadata");
        let url = format!(
            "{}/drive/v3/files/{}",
            self.base_url,
            urlencoded(file_id)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("fields", "id,name,mimeType,size")])
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?;

        let meta: FileMetadata = resp
            .json()
            .await
            .context("gdrive: failed to parse file metadata")?;
        debug!(file_id, name = %meta.name, mime = %meta.mime_type, "gdrive: metadata ok");
        Ok(meta)
    }

    /// Download a binary (non-Google-native) file from Drive.
    pub async fn download_file(&self, file_id: &str) -> anyhow::Result<Vec<u8>> {
        debug!(file_id, "gdrive: download_file");
        let url = format!(
            "{}/drive/v3/files/{}",
            self.base_url,
            urlencoded(file_id)
        );
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.access_token)
            .query(&[("alt", "media")])
            .send()
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?;

        let bytes = resp.bytes().await?;
        debug!(file_id, size = bytes.len(), "gdrive: download ok");
        Ok(bytes.to_vec())
    }

    /// Export a Google-native file (Docs/Sheets/Slides/Drawings) to a specific format.
    pub async fn export_file(
        &self,
        file_id: &str,
        export_mime: &str,
    ) -> anyhow::Result<Vec<u8>> {
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
            .await?
            .error_for_status()
            .map_err(anyhow::Error::from)?;

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
    ) -> anyhow::Result<std::path::PathBuf> {
        let dest = if let Some(path) = output {
            path.to_path_buf()
        } else {
            let name = &result.file_name;
            if let Some(fmt) = &result.export_format {
                let stem = Path::new(name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name);
                std::path::PathBuf::from(format!("{}.{}", stem, fmt.extension()))
            } else {
                std::path::PathBuf::from(name)
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

fn urlencoded(s: &str) -> String {
    s.replace('#', "%23").replace(' ', "%20")
}

/// Create a Drive API client from the user's stored OAuth tokens (reuses gmail auth).
pub async fn build_drive_client(
    store_path: &Path,
    account_id: &str,
    credentials_file: Option<&str>,
) -> anyhow::Result<DriveApiClient> {
    let token_path = void_gmail::auth::token_cache_path(store_path, account_id);
    let mut cache = void_gmail::auth::TokenCache::load(&token_path)?;

    let is_expired = cache
        .expires_at
        .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
        .unwrap_or(true);

    if is_expired {
        debug!(account_id, "refreshing Drive access token");
        if let Some(ref refresh_token) = cache.refresh_token {
            let creds = void_gmail::auth::load_client_credentials(credentials_file)?;
            let http = reqwest::Client::new();
            cache = void_gmail::auth::refresh_access_token(&http, &creds, refresh_token).await?;
            cache.save(&token_path)?;
        } else {
            anyhow::bail!("token expired and no refresh token. Run `void setup`");
        }
    }

    Ok(DriveApiClient::new(&cache.access_token))
}

/// Run the interactive OAuth flow for Drive scopes.
pub async fn authenticate_drive(
    store_path: &Path,
    account_id: &str,
    credentials_file: Option<&str>,
) -> anyhow::Result<()> {
    let creds = void_gmail::auth::load_client_credentials(credentials_file)?;
    let token_path = void_gmail::auth::token_cache_path(store_path, account_id);
    let cache = void_gmail::auth::authorize_interactive(&creds, Some(DRIVE_SCOPES)).await?;
    cache.save(&token_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_google_doc_url() {
        let result = parse_google_url(
            "https://docs.google.com/document/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit",
        )
        .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Document);
    }

    #[test]
    fn parse_google_sheet_url() {
        let result = parse_google_url(
            "https://docs.google.com/spreadsheets/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit#gid=0",
        )
        .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Spreadsheet);
    }

    #[test]
    fn parse_google_slides_url() {
        let result = parse_google_url(
            "https://docs.google.com/presentation/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit",
        )
        .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Presentation);
    }

    #[test]
    fn parse_google_drawing_url() {
        let result = parse_google_url(
            "https://docs.google.com/drawings/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/edit",
        )
        .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Drawing);
    }

    #[test]
    fn parse_drive_file_url() {
        let result = parse_google_url(
            "https://drive.google.com/file/d/1aBcDeFgHiJkLmNoPqRsTuVwXyZ/view?usp=sharing",
        )
        .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Drive);
    }

    #[test]
    fn parse_drive_open_url() {
        let result =
            parse_google_url("https://drive.google.com/open?id=1aBcDeFgHiJkLmNoPqRsTuVwXyZ")
                .unwrap();
        assert_eq!(result.file_id, "1aBcDeFgHiJkLmNoPqRsTuVwXyZ");
        assert_eq!(result.kind, GoogleFileKind::Drive);
    }

    #[test]
    fn parse_invalid_url_fails() {
        assert!(parse_google_url("https://example.com/foo").is_err());
        assert!(parse_google_url("not a url").is_err());
    }

    #[test]
    fn parse_incomplete_drive_url_fails() {
        assert!(parse_google_url("https://drive.google.com/file/d/").is_err());
    }

    #[test]
    fn export_format_from_name_roundtrip() {
        for name in ["txt", "md", "pdf", "docx", "csv", "xlsx", "pptx", "png", "svg"] {
            assert!(ExportFormat::from_name(name).is_some(), "failed for: {name}");
        }
        assert!(ExportFormat::from_name("unknown").is_none());
    }

    #[test]
    fn default_formats_for_docs() {
        let (text, bin) = default_export_formats(GOOGLE_DOCS_MIME);
        assert_eq!(text, ExportFormat::PlainText);
        assert_eq!(bin, ExportFormat::Pdf);
    }

    #[test]
    fn default_formats_for_sheets() {
        let (text, bin) = default_export_formats(GOOGLE_SHEETS_MIME);
        assert_eq!(text, ExportFormat::Csv);
        assert_eq!(bin, ExportFormat::Xlsx);
    }

    #[test]
    fn is_google_native() {
        assert!(is_google_native_mime("application/vnd.google-apps.document"));
        assert!(is_google_native_mime("application/vnd.google-apps.spreadsheet"));
        assert!(!is_google_native_mime("application/pdf"));
        assert!(!is_google_native_mime("text/plain"));
    }

    #[test]
    fn save_to_disk_generates_correct_name() {
        let result = DownloadResult {
            file_name: "My Document".to_string(),
            mime_type: "text/plain".to_string(),
            data: b"hello world".to_vec(),
            export_format: Some(ExportFormat::PlainText),
        };
        let dir = std::env::temp_dir().join(format!("void-gdrive-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = DriveApiClient::save_to_disk(&result, None).unwrap();
        assert_eq!(dest.file_name().unwrap().to_str().unwrap(), "My Document.txt");
        std::fs::remove_file(&dest).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn api_get_metadata() {
        let mock_server = wiremock::MockServer::start().await;

        let body = r#"{
            "id": "abc123",
            "name": "Test Doc",
            "mimeType": "application/vnd.google-apps.document",
            "size": null
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/abc123"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(body))
            .mount(&mock_server)
            .await;

        let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
        let meta = api.get_file_metadata("abc123").await.unwrap();
        assert_eq!(meta.name, "Test Doc");
        assert_eq!(meta.mime_type, "application/vnd.google-apps.document");
    }

    #[tokio::test]
    async fn api_download_file() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/bin123"))
            .and(wiremock::matchers::query_param("alt", "media"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_bytes(b"file content here"),
            )
            .mount(&mock_server)
            .await;

        let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
        let data = api.download_file("bin123").await.unwrap();
        assert_eq!(data, b"file content here");
    }

    #[tokio::test]
    async fn api_export_file() {
        let mock_server = wiremock::MockServer::start().await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/doc123/export"))
            .and(wiremock::matchers::query_param("mimeType", "text/plain"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string("exported plain text content"),
            )
            .mount(&mock_server)
            .await;

        let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
        let data = api.export_file("doc123", "text/plain").await.unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), "exported plain text content");
    }

    #[tokio::test]
    async fn fetch_file_exports_google_native() {
        let mock_server = wiremock::MockServer::start().await;

        let meta_body = r#"{
            "id": "doc456",
            "name": "My Spreadsheet",
            "mimeType": "application/vnd.google-apps.spreadsheet"
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/doc456"))
            .and(wiremock::matchers::query_param("fields", "id,name,mimeType,size"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
            .mount(&mock_server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/doc456/export"))
            .and(wiremock::matchers::query_param("mimeType", "text/csv"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_string("col1,col2\na,b"),
            )
            .mount(&mock_server)
            .await;

        let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
        let result = api.fetch_file("doc456", None).await.unwrap();
        assert_eq!(result.file_name, "My Spreadsheet");
        assert_eq!(result.export_format, Some(ExportFormat::Csv));
        assert_eq!(String::from_utf8(result.data).unwrap(), "col1,col2\na,b");
    }

    #[tokio::test]
    async fn fetch_file_downloads_binary() {
        let mock_server = wiremock::MockServer::start().await;

        let meta_body = r#"{
            "id": "pdf789",
            "name": "Report.pdf",
            "mimeType": "application/pdf",
            "size": "1024"
        }"#;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/pdf789"))
            .and(wiremock::matchers::query_param("fields", "id,name,mimeType,size"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(meta_body))
            .mount(&mock_server)
            .await;

        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/drive/v3/files/pdf789"))
            .and(wiremock::matchers::query_param("alt", "media"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_bytes(b"pdf-binary-content"),
            )
            .mount(&mock_server)
            .await;

        let api = DriveApiClient::with_base_url("test-token", &mock_server.uri());
        let result = api.fetch_file("pdf789", None).await.unwrap();
        assert_eq!(result.file_name, "Report.pdf");
        assert!(result.export_format.is_none());
        assert_eq!(result.data, b"pdf-binary-content");
    }
}
