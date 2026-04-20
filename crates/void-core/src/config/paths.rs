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

pub fn default_config() -> String {
    format!(
        r#"[store]
path = "{}"

[sync]
gmail_poll_interval_secs = 30
calendar_poll_interval_secs = 60
hackernews_poll_interval_secs = 3600

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
