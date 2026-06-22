use std::path::PathBuf;

const LEGACY_CONFIG_DIR: &str = ".config/void";
const LEGACY_STORE_DIR: &str = ".local/share/void";
pub(super) const CONFIG_FILENAME: &str = "config.toml";

fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn legacy_config_dir() -> PathBuf {
    dirs_home().join(LEGACY_CONFIG_DIR)
}

#[cfg(windows)]
fn legacy_store_dir() -> PathBuf {
    dirs_home().join(LEGACY_STORE_DIR)
}

fn preferred_config_dir() -> PathBuf {
    let legacy = legacy_config_dir();
    if legacy.exists() {
        return legacy;
    }

    dirs::config_dir()
        .map(|path| path.join("void"))
        .unwrap_or(legacy)
}

#[cfg(windows)]
pub(crate) fn preferred_store_dir() -> PathBuf {
    let legacy = legacy_store_dir();
    if legacy.exists() {
        return legacy;
    }

    dirs::data_dir()
        .map(|path| path.join("void"))
        .unwrap_or(legacy)
}

#[cfg(windows)]
fn default_store_path_template() -> String {
    // TOML basic strings need escaped backslashes on Windows paths.
    preferred_store_dir()
        .to_string_lossy()
        .replace('\\', "\\\\")
}

#[cfg(not(windows))]
fn default_store_path_template() -> String {
    format!("~/{LEGACY_STORE_DIR}")
}

pub fn default_config_path() -> PathBuf {
    preferred_config_dir().join(CONFIG_FILENAME)
}

/// Resolve a config path from CLI `--config` (expands leading `~`) or the default location.
pub fn resolve_config_path(path: Option<&std::path::Path>) -> PathBuf {
    match path {
        Some(p) => p
            .to_str()
            .filter(|s| s.starts_with('~'))
            .map(expand_tilde)
            .unwrap_or_else(|| p.to_path_buf()),
        None => default_config_path(),
    }
}

pub fn default_config() -> String {
    format!(
        r#"[store]
path = "{}"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60
hackernews_poll_interval_secs = 3600
googlenews_poll_interval_secs = 3600
reddit_poll_interval_secs = 3600
linkedin_poll_interval_secs = 1800
linkedin_backfill_days = 15
github_poll_interval_secs = 120

# Example connections (uncomment and fill in):
#
# [[connections]]
# id = "whatsapp"
# type = "whatsapp"
#
# [[connections]]
# id = "work-slack"
# type = "slack"
# app_token = "xapp-1-..."
# user_token = "xoxp-..."
# # app_id = "A012ABCD0A0"  # optional — enables auto-repair of event subscriptions
# # config_refresh_token = "xoxe-..."  # optional — Slack App Configuration refresh token
#
# [[connections]]
# id = "personal-gmail"
# type = "gmail"
# # credentials_file is optional — built-in Google credentials are used by default
# # credentials_file = "~/.config/void/custom-credentials.json"
#
# [[connections]]
# id = "my-calendar"
# type = "calendar"
# calendar_ids = ["primary"]
#
# [[connections]]
# id = "telegram"
# type = "telegram"
# # Optional: override built-in API credentials
# # api_id = 12345
# # api_hash = "0123456789abcdef0123456789abcdef"
#
# [[connections]]
# id = "hackernews"
# type = "hackernews"
# keywords = ["rust", "ai", "startup"]
# min_score = 100
#
# [[connections]]
# id = "reddit"
# type = "reddit"
# client_id = "your-reddit-app-client-id"
# client_secret = "your-reddit-app-client-secret"
# subreddits = ["rust", "programming", "startups"]
# keywords = ["ai", "llm"]
# min_score = 50
#
# [[connections]]
# id = "googlenews"
# type = "googlenews"
# keywords = ["intelligence artificielle", "startup"]
# when = "7d"          # recency window (e.g. 24h, 7d) — empty for no limit
# language = "fr"      # hl parameter
# country = "FR"       # gl parameter
#
# [[connections]]
# id = "linkedin"
# type = "linkedin"
# api_key = "your-unipile-api-key"
# dsn = "https://api1.unipile.com:13111"
# account_id = "your-unipile-account-id"
#
# [[connections]]
# id = "github"
# type = "github"
# token = "ghp_..."
# username = "your-github-handle"
"#,
        default_store_path_template()
    )
}

/// Expand `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs_home().join(rest)
    } else if path == "~" {
        dirs_home()
    } else {
        PathBuf::from(path)
    }
}

/// Redact a token for display: show first 8 chars + "..."
pub fn redact_token(token: &str) -> String {
    if token.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}...", &token[..8])
    }
}
