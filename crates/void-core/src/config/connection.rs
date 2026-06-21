use serde::de::Deserializer;

use crate::models::ConnectorType;
use serde::{Deserialize, Serialize};

/// Default Google News UI language (`hl` parameter).
pub(crate) fn default_gn_language() -> String {
    "fr".to_string()
}

/// Default Google News country edition (`gl` parameter).
pub(crate) fn default_gn_country() -> String {
    "FR".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_conversations: Vec<String>,
    #[serde(flatten)]
    pub settings: ConnectionSettings,
}

/// Custom deserializer that uses the `type` field to drive which
/// `ConnectionSettings` variant to parse, avoiding the ambiguity of
/// `#[serde(untagged)]` (Gmail and Calendar share `credentials_file`).
impl<'de> Deserialize<'de> for ConnectionConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw: RawConnectionConfig = RawConnectionConfig::deserialize(deserializer)?;
        let settings = match raw.connector_type {
            ConnectorType::Slack => ConnectionSettings::Slack {
                app_token: raw
                    .app_token
                    .ok_or_else(|| serde::de::Error::missing_field("app_token"))?,
                user_token: raw
                    .user_token
                    .ok_or_else(|| serde::de::Error::missing_field("user_token"))?,
                app_id: raw.slack_app_id,
                config_refresh_token: raw.config_refresh_token,
            },
            ConnectorType::Gmail => ConnectionSettings::Gmail {
                credentials_file: raw.credentials_file,
            },
            ConnectorType::Calendar => ConnectionSettings::Calendar {
                credentials_file: raw.credentials_file,
                calendar_ids: raw.calendar_ids.unwrap_or_default(),
            },
            ConnectorType::WhatsApp => ConnectionSettings::WhatsApp {},
            ConnectorType::Telegram => ConnectionSettings::Telegram {
                api_id: raw.api_id,
                api_hash: raw.api_hash,
            },
            ConnectorType::HackerNews => ConnectionSettings::HackerNews {
                keywords: raw.keywords.unwrap_or_default(),
                min_score: raw.min_score.unwrap_or(0),
            },
            ConnectorType::GoogleNews => ConnectionSettings::GoogleNews {
                keywords: raw.keywords.unwrap_or_default(),
                when: raw.when.unwrap_or_default(),
                language: raw.language.unwrap_or_else(default_gn_language),
                country: raw.country.unwrap_or_else(default_gn_country),
            },
            ConnectorType::LinkedIn => ConnectionSettings::LinkedIn {
                api_key: raw
                    .api_key
                    .ok_or_else(|| serde::de::Error::missing_field("api_key"))?,
                dsn: raw
                    .dsn
                    .ok_or_else(|| serde::de::Error::missing_field("dsn"))?,
                account_id: raw
                    .account_id
                    .ok_or_else(|| serde::de::Error::missing_field("account_id"))?,
            },
            ConnectorType::Reddit => ConnectionSettings::Reddit {
                client_id: raw
                    .client_id
                    .ok_or_else(|| serde::de::Error::missing_field("client_id"))?,
                client_secret: raw
                    .client_secret
                    .ok_or_else(|| serde::de::Error::missing_field("client_secret"))?,
                refresh_token: raw.refresh_token,
                subreddits: raw.subreddits.unwrap_or_default(),
                keywords: raw.keywords.unwrap_or_default(),
                min_score: raw.min_score.unwrap_or(0),
            },
        };
        Ok(ConnectionConfig {
            id: raw.id,
            connector_type: raw.connector_type,
            ignore_conversations: raw.ignore_conversations.unwrap_or_default(),
            settings,
        })
    }
}

#[derive(Deserialize)]
struct RawConnectionConfig {
    id: String,
    #[serde(rename = "type")]
    connector_type: ConnectorType,
    #[serde(default)]
    app_token: Option<String>,
    #[serde(default)]
    user_token: Option<String>,
    #[serde(default)]
    credentials_file: Option<String>,
    #[serde(default)]
    calendar_ids: Option<Vec<String>>,
    #[serde(default)]
    api_id: Option<i32>,
    #[serde(default)]
    api_hash: Option<String>,
    #[serde(default, rename = "app_id")]
    slack_app_id: Option<String>,
    #[serde(default)]
    config_refresh_token: Option<String>,
    #[serde(default)]
    keywords: Option<Vec<String>>,
    #[serde(default)]
    min_score: Option<u32>,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    dsn: Option<String>,
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    subreddits: Option<Vec<String>>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    ignore_conversations: Option<Vec<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConnectionSettings {
    Slack {
        app_token: String,
        user_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        config_refresh_token: Option<String>,
    },
    Gmail {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials_file: Option<String>,
    },
    Calendar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        credentials_file: Option<String>,
        #[serde(default)]
        calendar_ids: Vec<String>,
    },
    WhatsApp {},
    Telegram {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_id: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_hash: Option<String>,
    },
    HackerNews {
        #[serde(default)]
        keywords: Vec<String>,
        #[serde(default)]
        min_score: u32,
    },
    GoogleNews {
        #[serde(default)]
        keywords: Vec<String>,
        #[serde(default)]
        when: String,
        #[serde(default = "default_gn_language")]
        language: String,
        #[serde(default = "default_gn_country")]
        country: String,
    },
    LinkedIn {
        api_key: String,
        dsn: String,
        account_id: String,
    },
    Reddit {
        client_id: String,
        client_secret: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(default)]
        subreddits: Vec<String>,
        #[serde(default)]
        keywords: Vec<String>,
        #[serde(default)]
        min_score: u32,
    },
}

// Manual `Debug` so secret-bearing fields are redacted: a stray `debug!(?config)`
// or `{:?}` must never dump live tokens. `ConnectionConfig`/`VoidConfig` derive
// `Debug` but route through this impl, so they are covered transitively.
impl std::fmt::Debug for ConnectionSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use super::redact_token;
        match self {
            Self::Slack {
                app_token,
                user_token,
                app_id,
                config_refresh_token,
            } => f
                .debug_struct("Slack")
                .field("app_token", &redact_token(app_token))
                .field("user_token", &redact_token(user_token))
                .field("app_id", app_id)
                .field(
                    "config_refresh_token",
                    &config_refresh_token.as_deref().map(redact_token),
                )
                .finish(),
            Self::Gmail { credentials_file } => f
                .debug_struct("Gmail")
                .field("credentials_file", credentials_file)
                .finish(),
            Self::Calendar {
                credentials_file,
                calendar_ids,
            } => f
                .debug_struct("Calendar")
                .field("credentials_file", credentials_file)
                .field("calendar_ids", calendar_ids)
                .finish(),
            Self::WhatsApp {} => f.debug_struct("WhatsApp").finish(),
            Self::Telegram { api_id, api_hash } => f
                .debug_struct("Telegram")
                .field("api_id", api_id)
                .field("api_hash", &api_hash.as_deref().map(redact_token))
                .finish(),
            Self::HackerNews {
                keywords,
                min_score,
            } => f
                .debug_struct("HackerNews")
                .field("keywords", keywords)
                .field("min_score", min_score)
                .finish(),
            Self::GoogleNews {
                keywords,
                when,
                language,
                country,
            } => f
                .debug_struct("GoogleNews")
                .field("keywords", keywords)
                .field("when", when)
                .field("language", language)
                .field("country", country)
                .finish(),
            Self::LinkedIn {
                api_key,
                dsn,
                account_id,
            } => f
                .debug_struct("LinkedIn")
                .field("api_key", &redact_token(api_key))
                .field("dsn", dsn)
                .field("account_id", account_id)
                .finish(),
            Self::Reddit {
                client_id,
                client_secret,
                refresh_token,
                subreddits,
                keywords,
                min_score,
            } => f
                .debug_struct("Reddit")
                .field("client_id", &redact_token(client_id))
                .field("client_secret", &redact_token(client_secret))
                .field("refresh_token", &refresh_token.as_deref().map(redact_token))
                .field("subreddits", subreddits)
                .field("keywords", keywords)
                .field("min_score", min_score)
                .finish(),
        }
    }
}
