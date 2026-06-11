use chrono::{DateTime, Utc};
use serde::{self, Deserialize, Deserializer, Serializer};

/// Serialize an `i64` epoch timestamp as an ISO 8601 string (UTC).
/// Deserialize accepts both an ISO 8601 string and a raw integer for
/// backward compatibility with older data.
pub mod epoch_iso8601 {
    use super::*;

    pub fn serialize<S>(ts: &i64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match DateTime::<Utc>::from_timestamp(*ts, 0) {
            Some(dt) => {
                serializer.serialize_str(&dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
            }
            None => serializer.serialize_i64(*ts),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum TsOrString {
            Ts(i64),
            Str(String),
        }

        match TsOrString::deserialize(deserializer)? {
            TsOrString::Ts(ts) => Ok(ts),
            TsOrString::Str(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.timestamp())
                .map_err(serde::de::Error::custom),
        }
    }
}

/// Same as `epoch_iso8601` but for `Option<i64>` fields.
pub mod epoch_iso8601_opt {
    use super::*;

    pub fn serialize<S>(ts: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match ts {
            Some(ts) => match DateTime::<Utc>::from_timestamp(*ts, 0) {
                Some(dt) => serializer
                    .serialize_some(&dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
                None => serializer.serialize_some(ts),
            },
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum TsOrString {
            Ts(i64),
            Str(String),
        }

        let opt: Option<TsOrString> = Option::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(TsOrString::Ts(ts)) => Ok(Some(ts)),
            Some(TsOrString::Str(s)) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| Some(dt.timestamp()))
                .map_err(serde::de::Error::custom),
        }
    }
}
