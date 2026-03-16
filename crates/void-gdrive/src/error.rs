use thiserror::Error;

#[derive(Debug, Error)]
pub enum DriveError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("URL parse error: {0}")]
    UrlParse(String),
    #[error("{0}")]
    Other(String),
}
