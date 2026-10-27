use thiserror::Error;

#[derive(Debug, Error)]
pub enum WithingsError {
    #[error("authentication error: {0}")]
    Auth(String),
    #[error("Withings API error (status {status}): {message}")]
    Api { status: i64, message: String },
    #[error("decode error: {0}")]
    Decode(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
}

impl WithingsError {
    /// Withings reports an expired or revoked access token as status 401 inside
    /// an HTTP 200 body.
    pub fn is_invalid_token(&self) -> bool {
        matches!(self, WithingsError::Api { status: 401, .. })
    }
}
