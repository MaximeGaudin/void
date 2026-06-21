use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::Client;
use serde::Deserialize;

const DEFAULT_OAUTH_BASE: &str = "https://www.reddit.com";
const DEFAULT_API_BASE: &str = "https://oauth.reddit.com";
const USER_AGENT: &str = "void-cli/1.0 (by /u/void-cli)";
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Deserialize)]
pub struct RedditListing {
    pub data: RedditListingData,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedditListingData {
    #[serde(default)]
    pub children: Vec<RedditChild>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedditChild {
    pub data: RedditPost,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedditPost {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub score: Option<i32>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub permalink: Option<String>,
    #[serde(default)]
    pub num_comments: Option<u32>,
    #[serde(default)]
    pub upvote_ratio: Option<f64>,
    #[serde(default)]
    pub created_utc: Option<f64>,
    #[serde(default)]
    pub subreddit: Option<String>,
    #[serde(default)]
    pub selftext: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

pub struct RedditClient {
    http: Client,
    client_id: String,
    client_secret: String,
    oauth_base: String,
    api_base: String,
    token: Mutex<Option<CachedToken>>,
}

impl RedditClient {
    pub fn new(client_id: &str, client_secret: &str) -> Self {
        Self::with_bases(
            client_id,
            client_secret,
            DEFAULT_OAUTH_BASE,
            DEFAULT_API_BASE,
        )
    }

    fn with_bases(client_id: &str, client_secret: &str, oauth_base: &str, api_base: &str) -> Self {
        Self {
            http: Client::new(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            oauth_base: oauth_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: Mutex::new(None),
        }
    }

    pub fn user_agent(&self) -> &'static str {
        USER_AGENT
    }

    pub async fn subreddit_hot(
        &self,
        subreddit: &str,
        limit: u32,
    ) -> anyhow::Result<Vec<RedditPost>> {
        let token = self.access_token().await?;
        let url = format!("{}/r/{}/hot", self.api_base, sanitize_subreddit(subreddit));

        let response = self
            .http
            .get(&url)
            .query(&[("limit", limit.to_string())])
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Reddit API error {status}: {body}");
        }

        let listing: RedditListing = response.json().await?;
        Ok(listing.data.children.into_iter().map(|c| c.data).collect())
    }

    async fn access_token(&self) -> anyhow::Result<String> {
        if let Some(token) = self.cached_token()? {
            return Ok(token);
        }
        self.fetch_token().await
    }

    fn cached_token(&self) -> anyhow::Result<Option<String>> {
        let guard = self
            .token
            .lock()
            .map_err(|_| anyhow::anyhow!("token lock poisoned"))?;
        Ok(guard.as_ref().and_then(|cached| {
            if cached.expires_at > Instant::now() {
                Some(cached.access_token.clone())
            } else {
                None
            }
        }))
    }

    async fn fetch_token(&self) -> anyhow::Result<String> {
        let url = format!("{}/api/v1/access_token", self.oauth_base);
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.client_id, self.client_secret));

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Basic {auth}"))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("grant_type=client_credentials")
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Reddit OAuth error {status}: {body}");
        }

        let token_response: TokenResponse = response.json().await?;
        let expires_at =
            Instant::now() + Duration::from_secs(token_response.expires_in) - TOKEN_REFRESH_MARGIN;

        let access_token = token_response.access_token.clone();
        let mut guard = self
            .token
            .lock()
            .map_err(|_| anyhow::anyhow!("token lock poisoned"))?;
        *guard = Some(CachedToken {
            access_token: token_response.access_token,
            expires_at,
        });

        Ok(access_token)
    }

    #[cfg(test)]
    pub(crate) fn expired_token_for_test(&self, token: &str) {
        let mut guard = self.token.lock().unwrap();
        *guard = Some(CachedToken {
            access_token: token.to_string(),
            expires_at: Instant::now() - Duration::from_secs(1),
        });
    }
}

/// Normalize subreddit names for API paths and stable IDs.
pub fn sanitize_subreddit(name: &str) -> String {
    name.trim()
        .trim_start_matches("r/")
        .trim_start_matches('/')
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{body_string, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    #[test]
    fn sanitize_subreddit_strips_prefix_and_invalid_chars() {
        assert_eq!(sanitize_subreddit("r/Rust"), "rust");
        assert_eq!(sanitize_subreddit("/programming"), "programming");
        assert_eq!(sanitize_subreddit("start-ups!"), "startups");
    }

    #[test]
    fn deserialize_listing_response() {
        let json = r#"{
            "kind": "Listing",
            "data": {
                "children": [{
                    "kind": "t3",
                    "data": {
                        "id": "abc123",
                        "title": "Hello Rust",
                        "author": "dev",
                        "score": 150,
                        "url": "https://example.com",
                        "permalink": "/r/rust/comments/abc123/hello/",
                        "num_comments": 42,
                        "upvote_ratio": 0.91,
                        "created_utc": 1700000000.0,
                        "subreddit": "rust",
                        "selftext": "body"
                    }
                }]
            }
        }"#;
        let listing: RedditListing = serde_json::from_str(json).unwrap();
        assert_eq!(listing.data.children.len(), 1);
        let post = &listing.data.children[0].data;
        assert_eq!(post.id, "abc123");
        assert_eq!(post.title.as_deref(), Some("Hello Rust"));
        assert_eq!(post.score, Some(150));
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let json = r#"{
            "data": {
                "children": [{
                    "data": {
                        "id": "x1",
                        "title": null,
                        "author": null,
                        "score": null,
                        "url": null,
                        "permalink": null,
                        "num_comments": null,
                        "upvote_ratio": null,
                        "created_utc": null,
                        "subreddit": null,
                        "selftext": null
                    }
                }]
            }
        }"#;
        let listing: RedditListing = serde_json::from_str(json).unwrap();
        let post = &listing.data.children[0].data;
        assert_eq!(post.id, "x1");
        assert!(post.title.is_none());
        assert!(post.score.is_none());
    }

    #[tokio::test]
    async fn token_request_uses_client_credentials_and_user_agent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/access_token"))
            .and(header("User-Agent", USER_AGENT))
            .and(header("Authorization", "Basic Y2xpZW50OmFwcC1zZWNyZXQ="))
            .and(body_string("grant_type=client_credentials"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok123",
                "token_type": "bearer",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases("client", "app-secret", &server.uri(), &server.uri());
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "tok123");
    }

    #[tokio::test]
    async fn subreddit_request_uses_bearer_and_limit() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok123",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/r/rust/hot"))
            .and(query_param("limit", "100"))
            .and(header("Authorization", "Bearer tok123"))
            .and(header("User-Agent", USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "children": [] }
            })))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases("client", "secret", &server.uri(), &server.uri());
        let posts = client.subreddit_hot("rust", 100).await.unwrap();
        assert!(posts.is_empty());
    }

    #[tokio::test]
    async fn refreshes_expired_token() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fresh-token",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/r/rust/hot"))
            .and(header("Authorization", "Bearer fresh-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "children": [] }
            })))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases("client", "secret", &server.uri(), &server.uri());
        client.expired_token_for_test("stale-token");
        let posts = client.subreddit_hot("rust", 100).await.unwrap();
        assert!(posts.is_empty());
    }
}
