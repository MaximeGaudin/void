use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelegramError {
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("media error: {0}")]
    Media(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("{0}")]
    Other(String),
}
