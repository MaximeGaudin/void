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

#[cfg(test)]
mod tests {
    use super::TelegramError;

    #[test]
    fn telegram_error_display_auth() {
        let e = TelegramError::Auth("bad token".into());
        assert_eq!(e.to_string(), "authentication error: bad token");
    }

    #[test]
    fn telegram_error_display_connection() {
        let e = TelegramError::Connection("timeout".into());
        assert_eq!(e.to_string(), "connection error: timeout");
    }

    #[test]
    fn telegram_error_display_media() {
        let e = TelegramError::Media("no file".into());
        assert_eq!(e.to_string(), "media error: no file");
    }

    #[test]
    fn telegram_error_display_decode() {
        let e = TelegramError::Decode("bad bytes".into());
        assert_eq!(e.to_string(), "decode error: bad bytes");
    }

    #[test]
    fn telegram_error_display_other() {
        let e = TelegramError::Other("misc".into());
        assert_eq!(e.to_string(), "misc");
    }
}
