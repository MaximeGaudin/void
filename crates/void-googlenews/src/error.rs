use thiserror::Error;

#[derive(Debug, Error)]
pub enum GoogleNewsError {
    #[error("API error: {0}")]
    Api(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("{0}")]
    Other(String),
}
