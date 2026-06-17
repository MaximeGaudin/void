use reqwest::Client;
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://hacker-news.firebaseio.com/v0";

#[derive(Debug, Clone, Deserialize)]
pub struct HnItem {
    pub id: u64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub score: Option<u32>,
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub time: Option<i64>,
    #[serde(default, rename = "type")]
    pub item_type: Option<String>,
    #[serde(default)]
    pub descendants: Option<u32>,
}

pub struct HnClient {
    http: Client,
    base_url: String,
}

impl Default for HnClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HnClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Override the API base URL (used by tests to point at a mock server).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into(),
        }
    }

    pub async fn top_stories(&self) -> anyhow::Result<Vec<u64>> {
        let url = format!("{}/topstories.json", self.base_url);
        let ids: Vec<u64> = self.http.get(&url).send().await?.json().await?;
        Ok(ids)
    }

    pub async fn get_item(&self, id: u64) -> anyhow::Result<Option<HnItem>> {
        let url = format!("{}/item/{id}.json", self.base_url);
        let resp = self.http.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let item: Option<HnItem> = resp.json().await?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn top_stories_returns_ids_from_mock_server() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![1_u64, 2, 3]))
            .mount(&server)
            .await;

        let client = HnClient::with_base_url(server.uri());
        let ids = client.top_stories().await.unwrap();

        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn top_stories_errors_on_server_failure() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/topstories.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = HnClient::with_base_url(server.uri());
        let result = client.top_stories().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn get_item_returns_none_on_404() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/item/42.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let client = HnClient::with_base_url(server.uri());
        let item = client.get_item(42).await.unwrap();

        assert!(item.is_none());
    }

    #[tokio::test]
    async fn get_item_errors_on_server_failure() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/item/99.json"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = HnClient::with_base_url(server.uri());
        let result = client.get_item(99).await;

        assert!(result.is_err());
    }
}
