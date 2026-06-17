use void_googlenews::api::GoogleNewsClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SAMPLE_RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>"ai" - Google News</title>
    <item>
      <title>First article - Le Monde</title>
      <link>https://news.google.com/rss/articles/AAA</link>
      <guid isPermaLink="false">CBMiFIRST</guid>
      <pubDate>Mon, 09 Jun 2025 12:00:00 GMT</pubDate>
      <description>&lt;a href="x"&gt;First&lt;/a&gt;</description>
      <source url="https://www.lemonde.fr">Le Monde</source>
    </item>
    <item>
      <title>Second article - Reuters</title>
      <link>https://news.google.com/rss/articles/BBB</link>
      <guid isPermaLink="false">CBMiSECOND</guid>
      <pubDate>Tue, 10 Jun 2025 08:30:00 GMT</pubDate>
      <description>Second snippet</description>
      <source url="https://www.reuters.com">Reuters</source>
    </item>
  </channel>
</rss>"#;

#[tokio::test]
async fn search_parses_feed_from_mock_server() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rss/search"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(SAMPLE_RSS, "application/xml; charset=UTF-8"),
        )
        .mount(&server)
        .await;

    let client = GoogleNewsClient::with_base_url(format!("{}/rss/search", server.uri()));
    let items = client.search("ai", "7d", "fr", "FR").await.unwrap();

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].stable_id(), Some("CBMiFIRST"));
    assert_eq!(items[0].source_name(), Some("Le Monde"));
    assert_eq!(items[1].stable_id(), Some("CBMiSECOND"));
    assert_eq!(items[1].source_name(), Some("Reuters"));
}

#[tokio::test]
async fn search_errors_on_http_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/rss/search"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let client = GoogleNewsClient::with_base_url(format!("{}/rss/search", server.uri()));
    let result = client.search("ai", "", "fr", "FR").await;
    assert!(result.is_err());
}
