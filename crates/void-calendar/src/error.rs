use thiserror::Error;

#[derive(Debug, Error)]
pub enum CalendarError {
    #[error("API error: {0}")]
    Api(String),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Other(String),
}
