use serde::Deserialize;

/// Unipile LinkedIn payloads often use `0`/`1` integers where docs describe booleans.
/// See https://developer.unipile.com/docs/message-payload
pub(crate) mod flexible {
    use serde::de::Deserializer;
    use serde::Deserialize;

    pub fn option_bool<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BoolOrInt {
            Bool(bool),
            Int(i64),
        }

        match Option::<BoolOrInt>::deserialize(deserializer)? {
            None => Ok(None),
            Some(BoolOrInt::Bool(b)) => Ok(Some(b)),
            Some(BoolOrInt::Int(0)) => Ok(Some(false)),
            Some(BoolOrInt::Int(n)) => Ok(Some(n != 0)),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListResponse<T> {
    #[serde(default)]
    pub items: Vec<T>,
    #[serde(default)]
    pub cursor: Option<String>,
}

/// https://developer.unipile.com/docs/message-payload (chat parent object)
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileChat {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub account_type: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub attendee_provider_id: Option<String>,
    #[serde(default)]
    pub r#type: Option<i32>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub unread_count: Option<i32>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub pinned: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub archived: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub read_only: Option<bool>,
}

/// https://developer.unipile.com/docs/message-payload
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileMessage {
    /// Canonical Unipile message id (dedup key).
    #[serde(alias = "message_id")]
    pub id: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub chat_provider_id: Option<String>,
    #[serde(default)]
    pub sender_id: Option<String>,
    #[serde(default)]
    pub sender_attendee_id: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub attachments: Option<Vec<UnipileAttachment>>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub is_sender: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub seen: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub delivered: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub hidden: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub deleted: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub edited: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub is_event: Option<bool>,
    #[serde(default)]
    pub event_type: Option<i32>,
    #[serde(default)]
    pub message_type: Option<String>,
}

impl UnipileMessage {
    /// Whether this payload should be stored in the Void inbox.
    pub fn is_syncable(&self) -> bool {
        if self.id.is_empty() {
            return false;
        }
        if self.deleted.unwrap_or(false) {
            return false;
        }
        if self.hidden.unwrap_or(false) {
            return false;
        }
        if self.is_event.unwrap_or(false) {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileAttachment {
    pub id: String,
    #[serde(default)]
    pub mimetype: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub unavailable: Option<bool>,
    #[serde(default, deserialize_with = "flexible::option_bool")]
    pub sticker: Option<bool>,
}

/// https://developer.unipile.com/docs/retrieving-users
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileUserProfile {
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub public_identifier: Option<String>,
    #[serde(default)]
    pub profile_picture_url: Option<String>,
    #[serde(default)]
    pub public_profile_url: Option<String>,
}

impl UnipileUserProfile {
    pub fn display_name(&self) -> Option<String> {
        match (&self.first_name, &self.last_name) {
            (Some(f), Some(l)) if !f.is_empty() || !l.is_empty() => {
                Some(format!("{} {}", f.trim(), l.trim()).trim().to_string())
            }
            (Some(f), None) if !f.is_empty() => Some(f.trim().to_string()),
            (None, Some(l)) if !l.is_empty() => Some(l.trim().to_string()),
            _ => None,
        }
    }

    pub fn profile_url(&self) -> Option<String> {
        if let Some(url) = &self.public_profile_url {
            if !url.is_empty() {
                return Some(url.clone());
            }
        }
        self.public_identifier.as_ref().map(|id| {
            if id.starts_with("http") {
                id.clone()
            } else {
                format!("https://www.linkedin.com/in/{id}")
            }
        })
    }
}

/// https://developer.unipile.com/docs/retrieving-users
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnipileChatAttendee {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default)]
    pub profile_url: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountResponse {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct SendMessageResponse {
    #[serde(default, alias = "message_id")]
    pub(super) id: Option<String>,
}
