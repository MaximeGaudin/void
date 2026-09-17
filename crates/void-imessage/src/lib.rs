//! iMessage adapter for Void: reads and sends messages via macOS Messages.
//!
//! Supports reading local history from `chat.db` and sending via AppleScript
//! with SQLite receipt confirmation.

pub mod connector;

pub const CONNECTOR_ID: &str = "imessage";

/// Default path of the Messages store for the current user.
///
/// Fails rather than guessing when `HOME` is unset: an invented path would
/// surface later as a confusing "database missing" instead of the real cause.
pub fn connector_default_db_path() -> anyhow::Result<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        connector::store::default_db_path()
            .ok_or_else(|| anyhow::anyhow!("HOME is not set, cannot locate the Messages database"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        anyhow::bail!("the Messages database only exists on macOS")
    }
}
