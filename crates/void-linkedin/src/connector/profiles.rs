use std::collections::HashMap;

use tracing::debug;

use crate::api::{UnipileChatAttendee, UnipileClient, UnipileMessage, UnipileUserProfile};

/// Resolved sender display info stored on messages.
#[derive(Debug, Clone)]
pub struct SenderProfile {
    pub display_name: String,
    pub profile_url: Option<String>,
    pub avatar_url: Option<String>,
    pub public_identifier: Option<String>,
}

impl SenderProfile {
    fn unknown(provider_id: &str) -> Self {
        Self {
            display_name: provider_id.to_string(),
            profile_url: None,
            avatar_url: None,
            public_identifier: None,
        }
    }
}

/// In-memory cache for Unipile user/attendee lookups (avoids repeated profile API calls).
#[derive(Default)]
pub struct ProfileCache {
    by_provider_id: HashMap<String, SenderProfile>,
}

impl ProfileCache {
    pub async fn resolve(
        &mut self,
        client: &UnipileClient,
        account_id: &str,
        msg: &UnipileMessage,
    ) -> SenderProfile {
        let provider_id = match msg.sender_id.as_deref() {
            Some(id) if !id.is_empty() => id,
            _ => return SenderProfile::unknown("unknown"),
        };

        if let Some(profile) = self.by_provider_id.get(provider_id) {
            return profile.clone();
        }

        let profile = if let Some(ref attendee_id) = msg.sender_attendee_id {
            match client.get_chat_attendee(attendee_id).await {
                Ok(attendee) => profile_from_attendee(&attendee),
                Err(e) => {
                    debug!(
                        attendee_id,
                        error = %e,
                        "chat_attendees lookup failed, falling back to users API"
                    );
                    fetch_user_profile(client, account_id, provider_id).await
                }
            }
        } else {
            fetch_user_profile(client, account_id, provider_id).await
        };

        self.by_provider_id
            .insert(provider_id.to_string(), profile.clone());
        profile
    }

    pub async fn resolve_provider(
        &mut self,
        client: &UnipileClient,
        account_id: &str,
        provider_id: &str,
        attendee_id: Option<&str>,
    ) -> SenderProfile {
        if let Some(profile) = self.by_provider_id.get(provider_id) {
            return profile.clone();
        }

        let profile = if let Some(attendee_id) = attendee_id {
            match client.get_chat_attendee(attendee_id).await {
                Ok(attendee) => profile_from_attendee(&attendee),
                Err(_) => fetch_user_profile(client, account_id, provider_id).await,
            }
        } else {
            fetch_user_profile(client, account_id, provider_id).await
        };

        self.by_provider_id
            .insert(provider_id.to_string(), profile.clone());
        profile
    }
}

async fn fetch_user_profile(
    client: &UnipileClient,
    account_id: &str,
    provider_id: &str,
) -> SenderProfile {
    match client.get_user_profile(account_id, provider_id).await {
        Ok(user) => profile_from_user(&user),
        Err(e) => {
            debug!(provider_id, error = %e, "users profile lookup failed");
            SenderProfile::unknown(provider_id)
        }
    }
}

fn profile_from_user(user: &UnipileUserProfile) -> SenderProfile {
    let display_name = user
        .display_name()
        .unwrap_or_else(|| user.provider_id.clone());
    SenderProfile {
        display_name,
        profile_url: user.profile_url(),
        avatar_url: user.profile_picture_url.clone(),
        public_identifier: user.public_identifier.clone(),
    }
}

fn profile_from_attendee(attendee: &UnipileChatAttendee) -> SenderProfile {
    let display_name = attendee
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| attendee.provider_id.clone());
    SenderProfile {
        display_name,
        profile_url: attendee.profile_url.clone(),
        avatar_url: attendee.picture_url.clone(),
        public_identifier: None,
    }
}

pub(crate) fn build_message_metadata(
    msg: &UnipileMessage,
    profile: &SenderProfile,
) -> Option<serde_json::Value> {
    let media = super::extract::extract_media_metadata(msg);
    let mut obj = match media {
        Some(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };

    obj.insert(
        "author_name".to_string(),
        serde_json::Value::String(profile.display_name.clone()),
    );
    if let Some(url) = &profile.profile_url {
        obj.insert(
            "author_profile_url".to_string(),
            serde_json::Value::String(url.clone()),
        );
    }
    if let Some(id) = &profile.public_identifier {
        obj.insert(
            "public_identifier".to_string(),
            serde_json::Value::String(id.clone()),
        );
    }
    if let Some(pid) = &msg.sender_id {
        obj.insert(
            "provider_id".to_string(),
            serde_json::Value::String(pid.clone()),
        );
    }
    if let Some(aid) = &msg.sender_attendee_id {
        obj.insert(
            "sender_attendee_id".to_string(),
            serde_json::Value::String(aid.clone()),
        );
    }

    if obj.is_empty() {
        None
    } else {
        Some(serde_json::Value::Object(obj))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::UnipileUserProfile;

    #[test]
    fn profile_from_user_builds_name_and_url() {
        let user = UnipileUserProfile {
            provider_id: "ACo123".into(),
            first_name: Some("Matthieu".into()),
            last_name: Some("Lambda".into()),
            public_identifier: Some("matthieulambda".into()),
            profile_picture_url: None,
            public_profile_url: None,
        };
        let p = profile_from_user(&user);
        assert_eq!(p.display_name, "Matthieu Lambda");
        assert_eq!(
            p.profile_url.as_deref(),
            Some("https://www.linkedin.com/in/matthieulambda")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn profile_cache_resolve_prefers_chat_attendee() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/chat_attendees/att-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "att-1",
                "name": "Aubin Rioufol",
                "provider_id": "ACoAABFBQBcBtnr0Y6FNrtQpItSVnTX8Sxzl7Jg"
            })))
            .mount(&server)
            .await;

        let client = UnipileClient::with_api_base(&format!("{}/api/v1", server.uri()), "test-key");
        let mut cache = ProfileCache::default();
        let msg = UnipileMessage {
            sender_id: Some("ACoAABFBQBcBtnr0Y6FNrtQpItSVnTX8Sxzl7Jg".into()),
            sender_attendee_id: Some("att-1".into()),
            ..Default::default()
        };
        let profile = cache.resolve(&client, "acc-1", &msg).await;
        assert_eq!(profile.display_name, "Aubin Rioufol");

        // Second resolve hits cache (no extra mock needed).
        let again = cache.resolve(&client, "acc-1", &msg).await;
        assert_eq!(again.display_name, "Aubin Rioufol");
    }

    #[test]
    fn build_message_metadata_includes_author_fields() {
        let msg = UnipileMessage {
            sender_id: Some("ACo123".into()),
            ..Default::default()
        };
        let profile = SenderProfile {
            display_name: "Matthieu Lambda".into(),
            profile_url: Some("https://www.linkedin.com/in/matthieulambda".into()),
            avatar_url: None,
            public_identifier: Some("matthieulambda".into()),
        };
        let meta = build_message_metadata(&msg, &profile).unwrap();
        assert_eq!(meta["author_name"], "Matthieu Lambda");
        assert_eq!(
            meta["author_profile_url"],
            "https://www.linkedin.com/in/matthieulambda"
        );
    }
}
