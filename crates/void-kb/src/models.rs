use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "file")]
    File,
    #[serde(rename = "synced")]
    Synced,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::File => "file",
            Self::Synced => "synced",
        }
    }
}

impl std::fmt::Display for SourceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for SourceType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(Self::Text),
            "file" => Ok(Self::File),
            "synced" => Ok(Self::Synced),
            other => Err(format!("unknown source type: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub content: String,
    pub source_type: SourceType,
    pub source_path: Option<String>,
    pub content_hash: String,
    pub expiration: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub metadata: Vec<MetadataEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: i64,
    pub document_id: String,
    pub content: String,
    pub chunk_index: i64,
    pub start_byte: i64,
    pub end_byte: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document_id: String,
    pub content: String,
    pub chunk: String,
    pub metadata: serde_json::Value,
    pub score: f64,
    pub source_type: SourceType,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncFolder {
    pub id: String,
    pub folder_path: String,
    pub interval_secs: i64,
    pub last_scan_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbStatus {
    pub document_count: i64,
    pub chunk_count: i64,
    pub sync_folder_count: i64,
    pub db_size_bytes: u64,
}
