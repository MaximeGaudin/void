use serde::de::Deserializer;

use crate::models::ConnectorType;
use serde::{Deserialize, Serialize};

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
    keywords: Option<Vec<String>>,
    #[serde(default)]
    min_score: Option<u32>,
    #[serde(default)]
    ignore_conversations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConnectionSettings {
    Slack {
        app_token: String,
        user_token: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        app_id: Option<String>,
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
}
