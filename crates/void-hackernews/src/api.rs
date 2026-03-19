use reqwest::Client;
use serde::Deserialize;

const BASE_URL: &str = "https://hacker-news.firebaseio.com/v0";

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
        }
    }

    pub async fn top_stories(&self) -> anyhow::Result<Vec<u64>> {
        let url = format!("{BASE_URL}/topstories.json");
        let ids: Vec<u64> = self.http.get(&url).send().await?.json().await?;
        Ok(ids)
    }

    pub async fn new_stories(&self) -> anyhow::Result<Vec<u64>> {
        let url = format!("{BASE_URL}/newstories.json");
        let ids: Vec<u64> = self.http.get(&url).send().await?.json().await?;
        Ok(ids)
    }

    pub async fn get_item(&self, id: u64) -> anyhow::Result<Option<HnItem>> {
        let url = format!("{BASE_URL}/item/{id}.json");
        let resp = self.http.get(&url).send().await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let item: Option<HnItem> = resp.json().await?;
        Ok(item)
    }
}
