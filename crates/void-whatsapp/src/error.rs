use thiserror::Error;

#[derive(Debug, Error)]
pub enum WhatsAppError {
    #[error("Auth error: {0}")]
    Auth(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Media error: {0}")]
    Media(String),
    #[error("Decode error: {0}")]
    Decode(String),
    #[error("{0}")]
    Other(String),
}
