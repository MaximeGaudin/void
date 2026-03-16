use thiserror::Error;

#[derive(Debug, Error)]
pub enum GmailError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}
