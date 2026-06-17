use serde::Deserialize;

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

/// Download result with file content bytes and metadata.
#[derive(Debug)]
pub struct DownloadResult {
    pub file_name: String,
    pub mime_type: String,
    pub data: Vec<u8>,
    pub export_format: Option<ExportFormat>,
}
