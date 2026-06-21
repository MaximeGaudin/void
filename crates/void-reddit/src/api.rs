use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine;
use reqwest::Client;
use serde::Deserialize;

pub const OAUTH_REDIRECT_URI: &str = "http://localhost:8765";
pub const OAUTH_SCOPES: &str = "read submit identity";

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
    pub(crate) children: Vec<RedditListingItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum RedditListingItem {
    #[serde(rename = "t3")]
    Post(RedditPost),
    #[serde(rename = "t1")]
    Comment(RedditComment),
    #[serde(rename = "more")]
    More(MoreStub),
}

#[derive(Debug, Clone, Deserialize)]
pub struct MoreStub {
    #[serde(default)]
    #[allow(dead_code)]
    count: u32,
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

#[derive(Debug, Clone, Deserialize)]
pub struct RedditComment {
    pub id: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub score: Option<i32>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub link_id: Option<String>,
    #[serde(default)]
    pub created_utc: Option<f64>,
    #[serde(default)]
    pub depth: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_replies")]
    pub(crate) replies: RedditReplies,
}

#[derive(Debug, Clone, Default)]
pub(crate) enum RedditReplies {
    #[default]
    Empty,
    Listing(RedditListing),
}

fn deserialize_replies<'de, D>(deserializer: D) -> Result<RedditReplies, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.as_str().is_some_and(str::is_empty) {
        return Ok(RedditReplies::Empty);
    }
    if let Ok(listing) = serde_json::from_value::<RedditListing>(value) {
        return Ok(RedditReplies::Listing(listing));
    }
    Ok(RedditReplies::Empty)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

pub struct RedditClient {
    http: Client,
    client_id: String,
    client_secret: String,
    refresh_token: Option<String>,
    oauth_base: String,
    api_base: String,
    token: Mutex<Option<CachedToken>>,
}

impl RedditClient {
    pub fn new(client_id: &str, client_secret: &str) -> Self {
        Self::with_refresh_token(client_id, client_secret, None)
    }

    pub fn with_refresh_token(
        client_id: &str,
        client_secret: &str,
        refresh_token: Option<String>,
    ) -> Self {
        Self::with_bases(
            client_id,
            client_secret,
            refresh_token,
            DEFAULT_OAUTH_BASE,
            DEFAULT_API_BASE,
        )
    }

    fn with_bases(
        client_id: &str,
        client_secret: &str,
        refresh_token: Option<String>,
        oauth_base: &str,
        api_base: &str,
    ) -> Self {
        Self {
            http: Client::new(),
            client_id: client_id.to_string(),
            client_secret: client_secret.to_string(),
            refresh_token,
            oauth_base: oauth_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
            token: Mutex::new(None),
        }
    }

    pub fn has_user_token(&self) -> bool {
        self.refresh_token.is_some()
    }

    pub fn user_agent(&self) -> &'static str {
        USER_AGENT
    }

    pub fn authorize_url(&self, state: &str, redirect_uri: &str) -> String {
        format!(
            "{}/api/v1/authorize?client_id={}&response_type=code&state={}&redirect_uri={}&duration=permanent&scope={}",
            self.oauth_base,
            urlencoding::encode(&self.client_id),
            urlencoding::encode(state),
            urlencoding::encode(redirect_uri),
            urlencoding::encode(OAUTH_SCOPES),
        )
    }

    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> anyhow::Result<OAuthTokens> {
        let body = format!(
            "grant_type=authorization_code&code={}&redirect_uri={}",
            urlencoding::encode(code),
            urlencoding::encode(redirect_uri),
        );
        self.request_tokens(&body).await
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
        Ok(listing
            .data
            .children
            .into_iter()
            .filter_map(|item| match item {
                RedditListingItem::Post(post) => Some(post),
                _ => None,
            })
            .collect())
    }

    pub async fn get_post_comments(
        &self,
        post_id: &str,
        sort: &str,
        limit: u32,
        depth: u32,
    ) -> anyhow::Result<(RedditPost, Vec<RedditComment>)> {
        let token = self.access_token().await?;
        let url = format!("{}/comments/{}", self.api_base, post_id);

        let response = self
            .http
            .get(&url)
            .query(&[
                ("sort", sort.to_string()),
                ("limit", limit.to_string()),
                ("depth", depth.to_string()),
            ])
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Reddit API error {status}: {body}");
        }

        let listings: Vec<RedditListing> = response.json().await?;
        let post = listings
            .first()
            .and_then(|listing| {
                listing.data.children.first().and_then(|item| match item {
                    RedditListingItem::Post(post) => Some(post.clone()),
                    _ => None,
                })
            })
            .ok_or_else(|| anyhow::anyhow!("Reddit comments response missing post listing"))?;

        let mut comments = Vec::new();
        if let Some(comment_listing) = listings.get(1) {
            flatten_comments(&comment_listing.data.children, &mut comments);
        }

        Ok((post, comments))
    }

    pub async fn post_comment(&self, thing_id: &str, text: &str) -> anyhow::Result<String> {
        if !self.has_user_token() {
            anyhow::bail!(
                "Reddit commenting requires OAuth authorization. Run `void setup` and enable commenting."
            );
        }

        let token = self.access_token().await?;
        let url = format!("{}/api/comment", self.api_base);
        let body = format!(
            "thing_id={}&text={}&api_type=json",
            urlencoding::encode(thing_id),
            urlencoding::encode(text),
        );

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;

        let status = response.status();
        let raw = response.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!("Reddit API error {status}: {raw}");
        }

        let parsed: CommentResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("Failed to parse Reddit comment response: {e}: {raw}"))?;

        if let Some(errors) = parsed.json.errors {
            if !errors.is_empty() {
                anyhow::bail!("Reddit comment error: {errors:?}");
            }
        }

        let comment_id = parsed
            .json
            .data
            .and_then(|data| data.things.into_iter().next())
            .and_then(|thing| thing.data.id)
            .ok_or_else(|| anyhow::anyhow!("Reddit comment response missing comment id"))?;

        Ok(format!("t1_{comment_id}"))
    }

    pub(crate) async fn access_token(&self) -> anyhow::Result<String> {
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
        let body = if let Some(ref refresh_token) = self.refresh_token {
            format!(
                "grant_type=refresh_token&refresh_token={}",
                urlencoding::encode(refresh_token)
            )
        } else {
            "grant_type=client_credentials".to_string()
        };

        let tokens = self.request_tokens(&body).await?;
        self.store_access_token(&tokens.access_token, tokens.expires_in);
        Ok(tokens.access_token)
    }

    async fn request_tokens(&self, body: &str) -> anyhow::Result<OAuthTokens> {
        let url = format!("{}/api/v1/access_token", self.oauth_base);
        let auth = base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", self.client_id, self.client_secret));

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Basic {auth}"))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body.to_string())
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Reddit OAuth error {status}: {body}");
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(OAuthTokens {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_in: token_response.expires_in,
        })
    }

    fn store_access_token(&self, access_token: &str, expires_in: u64) {
        let expires_at = Instant::now() + Duration::from_secs(expires_in) - TOKEN_REFRESH_MARGIN;
        if let Ok(mut guard) = self.token.lock() {
            *guard = Some(CachedToken {
                access_token: access_token.to_string(),
                expires_at,
            });
        }
    }

    #[cfg(test)]
    pub(crate) fn expired_token_for_test(&self, token: &str) {
        let mut guard = self.token.lock().unwrap();
        *guard = Some(CachedToken {
            access_token: token.to_string(),
            expires_at: Instant::now() - Duration::from_secs(1),
        });
    }

    #[cfg(test)]
    fn with_bases_for_test(
        client_id: &str,
        client_secret: &str,
        refresh_token: Option<String>,
        oauth_base: &str,
        api_base: &str,
    ) -> Self {
        Self::with_bases(
            client_id,
            client_secret,
            refresh_token,
            oauth_base,
            api_base,
        )
    }
}

fn flatten_comments(children: &[RedditListingItem], out: &mut Vec<RedditComment>) {
    for item in children {
        match item {
            RedditListingItem::Comment(comment) => {
                if let RedditReplies::Listing(listing) = &comment.replies {
                    flatten_comments(&listing.data.children, out);
                }
                out.push(RedditComment {
                    replies: RedditReplies::Empty,
                    ..comment.clone()
                });
            }
            RedditListingItem::More(_) => {}
            RedditListingItem::Post(_) => {}
        }
    }
}

#[derive(Debug, Deserialize)]
struct CommentResponse {
    json: CommentResponseJson,
}

#[derive(Debug, Deserialize)]
struct CommentResponseJson {
    #[serde(default)]
    errors: Option<Vec<serde_json::Value>>,
    data: Option<CommentResponseData>,
}

#[derive(Debug, Deserialize)]
struct CommentResponseData {
    things: Vec<CommentThing>,
}

#[derive(Debug, Deserialize)]
struct CommentThing {
    data: CommentThingData,
}

#[derive(Debug, Deserialize)]
struct CommentThingData {
    id: Option<String>,
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

/// Extract a Reddit post ID from an external ID, URL, or raw ID.
pub fn extract_post_id(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        anyhow::bail!("post id is empty");
    }

    if let Some(id) = trimmed.strip_prefix("t3_") {
        return Ok(id.to_string());
    }

    if trimmed.contains("_post_") {
        if let Some(rest) = trimmed.rsplit("_post_").next() {
            return Ok(rest.to_string());
        }
    }

    if trimmed.starts_with("reddit_") {
        if let Some(rest) = trimmed.rsplit('_').next() {
            return Ok(rest.to_string());
        }
    }

    if trimmed.contains("/comments/") {
        let parts: Vec<&str> = trimmed.split('/').collect();
        if let Some(idx) = parts.iter().position(|p| *p == "comments") {
            if let Some(id) = parts.get(idx + 1) {
                return Ok(id.to_string());
            }
        }
    }

    if trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Ok(trimmed.to_string());
    }

    anyhow::bail!("unable to parse Reddit post id from '{trimmed}'")
}

/// Extract a Reddit comment ID from a void message external ID.
pub fn extract_comment_id_from_external(
    msg_external_id: &str,
    connection_id: &str,
) -> anyhow::Result<String> {
    let prefix = format!("reddit_{connection_id}_comment_");
    msg_external_id
        .strip_prefix(&prefix)
        .map(str::to_string)
        .ok_or_else(|| {
            anyhow::anyhow!("unable to parse Reddit comment id from '{msg_external_id}'")
        })
}

/// Extract a Reddit post ID from a void post-body message external ID.
pub fn extract_post_id_from_postbody_external(
    msg_external_id: &str,
    connection_id: &str,
) -> anyhow::Result<String> {
    let prefix = format!("reddit_{connection_id}_postbody_");
    msg_external_id
        .strip_prefix(&prefix)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("unable to parse Reddit post id from '{msg_external_id}'"))
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
    fn extract_post_id_from_various_inputs() {
        assert_eq!(extract_post_id("abc123").unwrap(), "abc123");
        assert_eq!(extract_post_id("t3_abc123").unwrap(), "abc123");
        assert_eq!(
            extract_post_id("reddit_reddit_post_abc123").unwrap(),
            "abc123"
        );
        assert_eq!(
            extract_post_id("https://www.reddit.com/r/rust/comments/abc123/title/").unwrap(),
            "abc123"
        );
    }

    #[test]
    fn extract_comment_id_from_external_id() {
        assert_eq!(
            super::extract_comment_id_from_external("reddit_reddit_comment_xyz", "reddit").unwrap(),
            "xyz"
        );
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
        let post = match &listing.data.children[0] {
            RedditListingItem::Post(post) => post,
            _ => panic!("expected post"),
        };
        assert_eq!(post.id, "abc123");
        assert_eq!(post.title.as_deref(), Some("Hello Rust"));
        assert_eq!(post.score, Some(150));
    }

    #[test]
    fn deserialize_missing_optional_fields() {
        let json = r#"{
            "data": {
                "children": [{
                    "kind": "t3",
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
        let post = match &listing.data.children[0] {
            RedditListingItem::Post(post) => post,
            _ => panic!("expected post"),
        };
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

        let client = RedditClient::with_bases_for_test(
            "client",
            "app-secret",
            None,
            &server.uri(),
            &server.uri(),
        );
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "tok123");
    }

    #[tokio::test]
    async fn token_request_uses_refresh_token_grant() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/v1/access_token"))
            .and(body_string(
                "grant_type=refresh_token&refresh_token=refresh-abc",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "user-tok",
                "token_type": "bearer",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases_for_test(
            "client",
            "secret",
            Some("refresh-abc".into()),
            &server.uri(),
            &server.uri(),
        );
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "user-tok");
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

        let client = RedditClient::with_bases_for_test(
            "client",
            "secret",
            None,
            &server.uri(),
            &server.uri(),
        );
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

        let client = RedditClient::with_bases_for_test(
            "client",
            "secret",
            None,
            &server.uri(),
            &server.uri(),
        );
        client.expired_token_for_test("stale-token");
        let posts = client.subreddit_hot("rust", 100).await.unwrap();
        assert!(posts.is_empty());
    }

    #[tokio::test]
    async fn get_post_comments_parses_nested_tree_and_skips_more() {
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
            .and(path("/comments/abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "data": {
                        "children": [{
                            "kind": "t3",
                            "data": {
                                "id": "abc123",
                                "title": "Post title",
                                "author": "author",
                                "selftext": "body"
                            }
                        }]
                    }
                },
                {
                    "data": {
                        "children": [
                            {
                                "kind": "t1",
                                "data": {
                                    "id": "c1",
                                    "author": "u1",
                                    "body": "top",
                                    "parent_id": "t3_abc123",
                                    "link_id": "t3_abc123",
                                    "created_utc": 1700000000.0,
                                    "depth": 0,
                                    "replies": {
                                        "data": {
                                            "children": [{
                                                "kind": "t1",
                                                "data": {
                                                    "id": "c2",
                                                    "author": "u2",
                                                    "body": "reply",
                                                    "parent_id": "t1_c1",
                                                    "link_id": "t3_abc123",
                                                    "created_utc": 1700000001.0,
                                                    "depth": 1,
                                                    "replies": ""
                                                }
                                            }]
                                        }
                                    }
                                }
                            },
                            {
                                "kind": "more",
                                "data": { "count": 5 }
                            }
                        ]
                    }
                }
            ])))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases_for_test(
            "client",
            "secret",
            Some("refresh".into()),
            &server.uri(),
            &server.uri(),
        );
        let (post, comments) = client
            .get_post_comments("abc123", "new", 200, 3)
            .await
            .unwrap();
        assert_eq!(post.id, "abc123");
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].id, "c2");
        assert_eq!(comments[1].id, "c1");
    }

    #[tokio::test]
    async fn post_comment_sends_form_body_and_parses_response() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/access_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok123",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/api/comment"))
            .and(body_string("thing_id=t1_parent&text=hello&api_type=json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "json": {
                    "errors": [],
                    "data": {
                        "things": [{
                            "data": { "id": "newcomment" }
                        }]
                    }
                }
            })))
            .mount(&server)
            .await;

        let client = RedditClient::with_bases_for_test(
            "client",
            "secret",
            Some("refresh".into()),
            &server.uri(),
            &server.uri(),
        );
        let id = client.post_comment("t1_parent", "hello").await.unwrap();
        assert_eq!(id, "t1_newcomment");
    }

    #[tokio::test]
    async fn post_comment_without_user_token_fails() {
        let client = RedditClient::new("client", "secret");
        let err = client.post_comment("t3_abc", "hello").await.unwrap_err();
        assert!(err.to_string().contains("OAuth authorization"));
    }
}
