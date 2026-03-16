use thiserror::Error;

#[derive(Debug, Error)]
pub enum SlackError {
    #[error("API error: {0}")]
    Api(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Rate limited after {0} retries: {1}")]
    RateLimited(u32, String),

    #[error("{0}")]
    Other(String),
}
