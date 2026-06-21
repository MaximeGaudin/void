use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::{Client, StatusCode};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://api.github.com";
const GITHUB_USER_AGENT: &str = "void-cli-github-connector";

#[derive(Debug, Clone, Deserialize)]
pub struct GhUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhRepository {
    pub full_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhSubject {
    pub title: String,
    #[serde(rename = "type")]
    pub subject_type: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhNotification {
    pub id: String,
    pub reason: String,
    pub updated_at: String,
    pub subject: GhSubject,
    pub repository: Option<GhRepository>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhSearchUser {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GhSearchIssue {
    pub id: u64,
    pub number: u64,
    pub title: String,
    pub html_url: String,
    pub repository_url: String,
    pub user: GhSearchUser,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GhSearchResponse {
    items: Vec<GhSearchIssue>,
}

pub struct GitHubClient {
    http: Client,
    base_url: String,
    token: String,
}

impl GitHubClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
            token: token.into(),
        }
    }

    /// Override the API base URL (used by tests to point at a mock server).
    pub fn with_base_url(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    fn auth_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, url)
            .header(AUTHORIZATION, format!("Bearer {}", self.token))
            .header(ACCEPT, "application/vnd.github+json")
            .header(USER_AGENT, GITHUB_USER_AGENT)
    }

    pub async fn current_user(&self) -> anyhow::Result<GhUser> {
        let url = format!("{}/user", self.base_url);
        let resp = self.auth_request(reqwest::Method::GET, &url).send().await?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            anyhow::bail!("GitHub token is invalid or expired");
        }
        if !resp.status().is_success() {
            anyhow::bail!("GitHub /user failed with status {}", resp.status());
        }
        Ok(resp.json().await?)
    }

    pub async fn review_requested_prs(&self) -> anyhow::Result<Vec<GhSearchIssue>> {
        let mut page = 1;
        let mut all = Vec::new();

        loop {
            let url = format!(
                "{}/search/issues?q=is:pr+is:open+review-requested:@me&per_page=100&page={page}",
                self.base_url
            );
            let resp = self.auth_request(reqwest::Method::GET, &url).send().await?;
            if resp.status() == StatusCode::UNAUTHORIZED {
                anyhow::bail!("GitHub token is invalid or expired");
            }
            if !resp.status().is_success() {
                anyhow::bail!(
                    "GitHub search failed with status {}: {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }

            let body: GhSearchResponse = resp.json().await?;
            if body.items.is_empty() {
                break;
            }
            let count = body.items.len();
            all.extend(body.items);
            if count < 100 {
                break;
            }
            page += 1;
        }

        Ok(all)
    }

    pub async fn notifications(&self, since: Option<&str>) -> anyhow::Result<Vec<GhNotification>> {
        let mut page = 1;
        let mut all = Vec::new();

        loop {
            let mut url = format!(
                "{}/notifications?all=false&participating=true&per_page=100&page={page}",
                self.base_url
            );
            if let Some(since) = since {
                url.push_str("&since=");
                url.push_str(since);
            }

            let resp = self.auth_request(reqwest::Method::GET, &url).send().await?;
            if resp.status() == StatusCode::UNAUTHORIZED {
                anyhow::bail!("GitHub token is invalid or expired");
            }
            if !resp.status().is_success() {
                anyhow::bail!(
                    "GitHub notifications failed with status {}: {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }

            let body: Vec<GhNotification> = resp.json().await?;
            if body.is_empty() {
                break;
            }
            let count = body.len();
            all.extend(body);
            if count < 100 {
                break;
            }
            page += 1;
        }

        Ok(all)
    }
}

pub fn repo_full_name_from_url(repository_url: &str) -> Option<String> {
    let prefix = "https://api.github.com/repos/";
    repository_url
        .strip_prefix(prefix)
        .map(|rest| rest.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn repo_full_name_from_url_parses_owner_repo() {
        assert_eq!(
            repo_full_name_from_url("https://api.github.com/repos/octocat/Hello-World"),
            Some("octocat/Hello-World".to_string())
        );
    }

    #[tokio::test]
    async fn current_user_returns_login_from_mock_server() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "login": "octocat"
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let user = client.current_user().await.unwrap();
        assert_eq!(user.login, "octocat");
    }

    #[tokio::test]
    async fn review_requested_prs_paginates() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{
                    "id": 1,
                    "number": 42,
                    "title": "Fix bug",
                    "html_url": "https://github.com/octocat/Hello-World/pull/42",
                    "repository_url": "https://api.github.com/repos/octocat/Hello-World",
                    "user": { "login": "octocat" },
                    "updated_at": "2024-01-01T12:00:00Z"
                }]
            })))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let prs = client.review_requested_prs().await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].number, 42);
    }

    #[tokio::test]
    async fn notifications_sends_bearer_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notifications"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let notifs = client.notifications(None).await.unwrap();
        assert!(notifs.is_empty());
    }

    #[tokio::test]
    async fn current_user_errors_on_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "bad-token");
        let err = client.current_user().await.unwrap_err();
        assert!(err.to_string().contains("invalid or expired"));
    }

    #[tokio::test]
    async fn current_user_errors_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let err = client.current_user().await.unwrap_err();
        assert!(err.to_string().contains("500"));
    }

    #[tokio::test]
    async fn review_requested_prs_errors_on_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "bad-token");
        let err = client.review_requested_prs().await.unwrap_err();
        assert!(err.to_string().contains("invalid or expired"));
    }

    #[tokio::test]
    async fn review_requested_prs_errors_on_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search/issues"))
            .respond_with(ResponseTemplate::new(403).set_body_string("rate limit exceeded"))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let err = client.review_requested_prs().await.unwrap_err();
        assert!(err.to_string().contains("403"));
    }

    #[tokio::test]
    async fn notifications_errors_on_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/notifications"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        let err = client.notifications(None).await.unwrap_err();
        assert!(err.to_string().contains("500"));
    }

    #[tokio::test]
    async fn current_user_errors_on_malformed_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let client = GitHubClient::with_base_url(server.uri(), "test-token");
        assert!(client.current_user().await.is_err());
    }
}
