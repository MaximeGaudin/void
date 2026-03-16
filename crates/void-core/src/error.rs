use thiserror::Error;

/// Database errors
#[derive(Debug, Error)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database lock poisoned")]
    LockPoisoned,
    #[error("{0}")]
    Other(String),
}

/// Configuration errors
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("{0}")]
    Other(String),
}

/// Hook errors
#[derive(Debug, Error)]
pub enum HookError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Hook not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Other(String),
}
