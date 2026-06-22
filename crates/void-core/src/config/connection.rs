use crate::models::ConnectorType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize)]
pub struct ConnectionConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub connector_type: ConnectorType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_conversations: Vec<String>,
    #[serde(flatten)]
    pub settings: toml::Table,
}

impl<'de> Deserialize<'de> for ConnectionConfig {
    fn deserialize<D: serde::de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            #[serde(rename = "type")]
            connector_type: ConnectorType,
            #[serde(default)]
            ignore_conversations: Vec<String>,
            #[serde(flatten)]
            settings: toml::Table,
        }
        let raw = Raw::deserialize(deserializer)?;
        Ok(ConnectionConfig {
            id: raw.id,
            connector_type: raw.connector_type,
            ignore_conversations: raw.ignore_conversations,
            settings: raw.settings,
        })
    }
}

impl std::fmt::Debug for ConnectionConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionConfig")
            .field("id", &self.id)
            .field("connector_type", &self.connector_type)
            .field("ignore_conversations", &self.ignore_conversations)
            .field("settings", &SettingsDebug(&self.settings))
            .finish()
    }
}

struct SettingsDebug<'a>(&'a toml::Table);

impl std::fmt::Debug for SettingsDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use super::redact_token;
        let mut map = f.debug_map();
        for (key, value) in self.0.iter() {
            match value {
                toml::Value::String(s) => {
                    map.entry(key, &redact_token(s));
                }
                other => {
                    map.entry(key, other);
                }
            }
        }
        map.finish()
    }
}

/// Read a string field from connection settings.
pub fn settings_str<'a>(table: &'a toml::Table, key: &str) -> Option<&'a str> {
    table.get(key).and_then(|v| v.as_str())
}

/// Read a copied string field from connection settings.
pub fn settings_string(table: &toml::Table, key: &str) -> Option<String> {
    settings_str(table, key).map(String::from)
}

/// Read an integer field from connection settings.
pub fn settings_i64(table: &toml::Table, key: &str) -> Option<i64> {
    table.get(key).and_then(|v| v.as_integer())
}

/// Read a u32 field from connection settings.
pub fn settings_u32(table: &toml::Table, key: &str) -> Option<u32> {
    settings_i64(table, key).and_then(|v| u32::try_from(v).ok())
}

/// Read a string list from connection settings (TOML array of strings).
pub fn settings_string_list(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Insert a string into connection settings.
pub fn settings_set_string(table: &mut toml::Table, key: &str, value: impl Into<String>) {
    table.insert(key.into(), toml::Value::String(value.into()));
}

/// Insert an optional string into connection settings (omit when None).
pub fn settings_set_opt_string(table: &mut toml::Table, key: &str, value: Option<String>) {
    if let Some(v) = value {
        settings_set_string(table, key, v);
    }
}

/// Insert a string list into connection settings.
pub fn settings_set_string_list(table: &mut toml::Table, key: &str, values: &[String]) {
    let arr: Vec<toml::Value> = values
        .iter()
        .map(|s| toml::Value::String(s.clone()))
        .collect();
    table.insert(key.into(), toml::Value::Array(arr));
}

/// Insert a u32 into connection settings.
pub fn settings_set_u32(table: &mut toml::Table, key: &str, value: u32) {
    table.insert(key.into(), toml::Value::Integer(value as i64));
}

/// Build an empty settings table.
pub fn empty_settings() -> toml::Table {
    toml::Table::new()
}
