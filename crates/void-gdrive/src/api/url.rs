use crate::error::DriveError;

use super::types::{ExportFormat, GoogleFileKind, ParsedGoogleUrl};

/// Known Google Apps MIME types.
pub(crate) const GOOGLE_DOCS_MIME: &str = "application/vnd.google-apps.document";
pub(crate) const GOOGLE_SHEETS_MIME: &str = "application/vnd.google-apps.spreadsheet";
pub(crate) const GOOGLE_SLIDES_MIME: &str = "application/vnd.google-apps.presentation";
pub(crate) const GOOGLE_DRAWINGS_MIME: &str = "application/vnd.google-apps.drawing";

/// Parse a Google Docs/Sheets/Slides/Drive URL and extract the file ID.
///
/// Supported URL formats:
/// - `https://docs.google.com/document/d/{id}/...`
/// - `https://docs.google.com/spreadsheets/d/{id}/...`
/// - `https://docs.google.com/presentation/d/{id}/...`
/// - `https://docs.google.com/drawings/d/{id}/...`
/// - `https://drive.google.com/file/d/{id}/...`
/// - `https://drive.google.com/open?id={id}`
pub fn parse_google_url(url_str: &str) -> Result<ParsedGoogleUrl, DriveError> {
    let url = url::Url::parse(url_str).map_err(|e| DriveError::UrlParse(e.to_string()))?;

    let host = url.host_str().unwrap_or("");
    let path_segments: Vec<&str> = url.path_segments().map_or(vec![], |s| s.collect());

    match host {
        "docs.google.com" => {
            let kind = match path_segments.first().copied() {
                Some("document") => GoogleFileKind::Document,
                Some("spreadsheets") => GoogleFileKind::Spreadsheet,
                Some("presentation") => GoogleFileKind::Presentation,
                Some("drawings") => GoogleFileKind::Drawing,
                _ => {
                    return Err(DriveError::UrlParse(format!(
                        "unrecognized docs.google.com path: {}",
                        url.path()
                    )))
                }
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
            Err(DriveError::UrlParse(format!(
                "could not extract file ID from: {url_str}"
            )))
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
            Err(DriveError::UrlParse(format!(
                "could not extract file ID from: {url_str}"
            )))
        }
        _ => Err(DriveError::UrlParse(format!(
            "not a recognized Google URL (host: {host})"
        ))),
    }
}

/// Whether a MIME type represents a native Google Apps format (needs export, not download).
pub fn is_google_native_mime(mime: &str) -> bool {
    mime.starts_with("application/vnd.google-apps.")
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

/// Reduce an untrusted remote file name to a single safe path component.
///
/// Keeps only the final path segment and drops control characters, so
/// traversal sequences (`../`), absolute paths, and embedded separators cannot
/// redirect the write outside the destination directory. Falls back to
/// `download.bin` when nothing usable remains.
pub(crate) fn sanitize_download_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        "download.bin".to_string()
    } else {
        cleaned.to_string()
    }
}

pub(crate) fn urlencoded(s: &str) -> String {
    s.replace('#', "%23").replace(' ', "%20")
}
