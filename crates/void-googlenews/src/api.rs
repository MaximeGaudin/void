use reqwest::Client;
use serde::Deserialize;

const RSS_BASE: &str = "https://news.google.com/rss/search";
/// Google News' RSS endpoint returns an empty body for non-browser clients,
/// so we present a desktop browser User-Agent (same trick as the skill script).
const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Clone, Deserialize)]
pub struct RssFeed {
    pub channel: Channel,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    #[serde(default, rename = "item")]
    pub items: Vec<RssItem>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RssItem {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub guid: Option<Guid>,
    #[serde(default, rename = "pubDate")]
    pub pub_date: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub source: Option<Source>,
}

/// `<guid isPermaLink="false">CBMi...</guid>` — text content carries the stable ID.
#[derive(Debug, Clone, Deserialize)]
pub struct Guid {
    #[serde(rename = "$text", default)]
    pub value: String,
}

/// `<source url="https://...">Le Monde</source>` — attribute + text.
#[derive(Debug, Clone, Deserialize)]
pub struct Source {
    #[serde(rename = "@url", default)]
    pub url: Option<String>,
    #[serde(rename = "$text", default)]
    pub name: String,
}

impl RssItem {
    /// Stable identifier for an article: the RSS `guid`, falling back to the link.
    pub fn stable_id(&self) -> Option<&str> {
        self.guid
            .as_ref()
            .map(|g| g.value.as_str())
            .filter(|s| !s.is_empty())
            .or_else(|| self.link.as_deref().filter(|s| !s.is_empty()))
    }

    pub fn source_name(&self) -> Option<&str> {
        self.source
            .as_ref()
            .map(|s| s.name.as_str())
            .filter(|s| !s.is_empty())
    }
}

/// Sanitize an article's stable id into a key safe for use in DB ids
/// (`{connection_id}-{gid}` / `googlenews_{connection_id}_{gid}`).
pub fn sanitize_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// Strip HTML tags from an RSS description snippet.
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub struct GoogleNewsClient {
    http: Client,
    base_url: String,
}

impl Default for GoogleNewsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GoogleNewsClient {
    pub fn new() -> Self {
        Self {
            http: Client::new(),
            base_url: RSS_BASE.to_string(),
        }
    }

    /// Override the RSS base URL (used by tests to point at a mock server).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Build a Google News RSS search URL for a single keyword.
    pub fn build_url(&self, keyword: &str, when: &str, language: &str, country: &str) -> String {
        let query = if when.is_empty() {
            keyword.to_string()
        } else {
            format!("{keyword} when:{when}")
        };
        let ceid_raw = format!("{country}:{language}");
        let q = urlencoding::encode(&query);
        let hl = urlencoding::encode(language);
        let gl = urlencoding::encode(country);
        let ceid = urlencoding::encode(&ceid_raw);
        format!("{}?q={q}&hl={hl}&gl={gl}&ceid={ceid}", self.base_url)
    }

    /// Fetch and parse the RSS feed for a single keyword search.
    pub async fn search(
        &self,
        keyword: &str,
        when: &str,
        language: &str,
        country: &str,
    ) -> anyhow::Result<Vec<RssItem>> {
        let url = self.build_url(keyword, when, language, country);
        let body = self
            .http
            .get(&url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let feed: RssFeed = quick_xml::de::from_str(&body)?;
        Ok(feed.channel.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_without_when() {
        let client = GoogleNewsClient::new();
        let url = client.build_url("rust lang", "", "fr", "FR");
        assert!(url.contains("q=rust%20lang"));
        assert!(url.contains("hl=fr"));
        assert!(url.contains("gl=FR"));
        assert!(url.contains("ceid=FR%3Afr"));
        assert!(!url.contains("when%3A"));
    }

    #[test]
    fn build_url_with_when_appends_operator() {
        let client = GoogleNewsClient::new();
        let url = client.build_url("ai", "7d", "en", "US");
        assert!(url.contains("q=ai%20when%3A7d"));
    }

    #[test]
    fn strip_html_removes_tags_and_collapses_whitespace() {
        let s = strip_html("<a href=\"x\">Hello</a>   <b>world</b>");
        assert_eq!(s, "Hello world");
    }

    #[test]
    fn sanitize_id_replaces_non_alnum() {
        assert_eq!(sanitize_id("CBMi-aB/c=="), "CBMi_aB_c__");
    }

    #[test]
    fn deserialize_realistic_rss() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rss version="2.0">
          <channel>
            <title>"ai" - Google News</title>
            <item>
              <title>Big AI breakthrough - Le Monde</title>
              <link>https://news.google.com/rss/articles/ABC123</link>
              <guid isPermaLink="false">CBMiQWh0dHBz</guid>
              <pubDate>Mon, 09 Jun 2025 12:00:00 GMT</pubDate>
              <description>&lt;a href="x"&gt;Big AI breakthrough&lt;/a&gt;</description>
              <source url="https://www.lemonde.fr">Le Monde</source>
            </item>
          </channel>
        </rss>"#;
        let feed: RssFeed = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(feed.channel.items.len(), 1);
        let item = &feed.channel.items[0];
        assert_eq!(
            item.title.as_deref(),
            Some("Big AI breakthrough - Le Monde")
        );
        assert_eq!(item.stable_id(), Some("CBMiQWh0dHBz"));
        assert_eq!(item.source_name(), Some("Le Monde"));
        assert_eq!(
            item.source.as_ref().unwrap().url.as_deref(),
            Some("https://www.lemonde.fr")
        );
    }

    #[test]
    fn deserialize_empty_channel() {
        let xml = r#"<rss version="2.0"><channel><title>x</title></channel></rss>"#;
        let feed: RssFeed = quick_xml::de::from_str(xml).unwrap();
        assert!(feed.channel.items.is_empty());
    }

    #[test]
    fn stable_id_falls_back_to_link() {
        let item = RssItem {
            link: Some("https://example.com/a".to_string()),
            ..Default::default()
        };
        assert_eq!(item.stable_id(), Some("https://example.com/a"));
    }
}
