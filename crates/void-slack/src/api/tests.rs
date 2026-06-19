#![cfg(test)]

use super::transport::MAX_RETRIES;
use super::*;
use crate::error::SlackError;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// -- Happy-path parsing --

#[tokio::test]
async fn conversations_list_parses_two_channels_and_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "channels": [
                {"id": "C1", "name": "general", "is_channel": true},
                {"id": "C2", "name": "random", "is_channel": true, "is_private": false}
            ],
            "response_metadata": {"next_cursor": "next123"}
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let resp = api.conversations_list(None, 100).await.unwrap();
    assert_eq!(resp.channels.len(), 2);
    assert_eq!(resp.channels[0].id, "C1");
    assert_eq!(resp.channels[0].name.as_deref(), Some("general"));
    assert_eq!(resp.channels[1].id, "C2");
    assert_eq!(
        resp.response_metadata
            .and_then(|m| m.next_cursor)
            .as_deref(),
        Some("next123")
    );
}

#[tokio::test]
async fn conversations_history_parses_threading_and_reactions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "messages": [
                {
                    "type": "message",
                    "ts": "1700000000.000100",
                    "user": "U1",
                    "text": "thread parent",
                    "thread_ts": "1700000000.000100",
                    "reply_count": 2,
                    "reactions": [{"name": "thumbsup", "count": 3, "users": ["U2"]}]
                },
                {
                    "type": "message",
                    "ts": "1700000001.000200",
                    "user": "U2",
                    "text": "standalone"
                }
            ],
            "has_more": false
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let resp = api
        .conversations_history("C1", 100, None, None)
        .await
        .unwrap();
    assert_eq!(resp.messages.len(), 2);
    let parent = &resp.messages[0];
    assert_eq!(parent.ts, "1700000000.000100");
    assert_eq!(parent.user.as_deref(), Some("U1"));
    assert!(parent.is_thread_parent_with_replies());
    assert_eq!(parent.reply_count, Some(2));
    assert_eq!(parent.reactions.len(), 1);
    assert_eq!(parent.reactions[0].name, "thumbsup");
    assert_eq!(parent.reactions[0].count, 3);
    // Second message is not a thread parent.
    assert!(!resp.messages[1].is_thread_parent_with_replies());
}

#[tokio::test]
async fn auth_test_parses_identity() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "url": "https://example.slack.com/",
            "team": "Example",
            "user": "alice",
            "team_id": "T1",
            "user_id": "U1"
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let resp = api.auth_test().await.unwrap();
    assert_eq!(resp.user.as_deref(), Some("alice"));
    assert_eq!(resp.user_id.as_deref(), Some("U1"));
    assert_eq!(resp.team_id.as_deref(), Some("T1"));
}

// -- Error paths --

/// Realistic 401: `{"ok":false,"error":"invalid_auth"}` -> SlackError::Api.
#[tokio::test]
async fn invalid_auth_surfaces_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": false,
            "error": "invalid_auth"
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let err = api.auth_test().await.expect_err("expected error");
    match err {
        SlackError::Api(msg) => assert!(msg.contains("invalid_auth"), "got {msg}"),
        other => panic!("expected Api error, got {other:?}"),
    }
}

/// 429 with `Retry-After: 0` exhausts MAX_RETRIES instantly -> RateLimited.
#[tokio::test]
async fn rate_limited_429_exhausts_retries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let err = api
        .conversations_list(None, 100)
        .await
        .expect_err("expected error");
    match err {
        SlackError::RateLimited(retries, label) => {
            assert_eq!(retries, MAX_RETRIES);
            assert_eq!(label, "conversations.list");
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

/// 5xx with a non-JSON body cannot decode into SlackResponse -> Http error.
#[tokio::test]
async fn server_error_5xx_surfaces_http_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.list"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let err = api
        .conversations_list(None, 100)
        .await
        .expect_err("expected error");
    assert!(matches!(err, SlackError::Http(_)), "got {err:?}");
}

/// Malformed JSON: `messages` is required (Vec) but absent -> Http decode error.
#[tokio::test]
async fn malformed_history_missing_messages_is_clean_err() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/conversations.history"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "has_more": false
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let err = api
        .conversations_history("C1", 100, None, None)
        .await
        .expect_err("expected error for missing messages");
    // `messages` is required; with `#[serde(flatten)]` a body that omits it makes
    // `data` deserialize to None, surfacing as Api("ok=true but no data").
    // Either way it is a clean Err and never a panic.
    assert!(
        matches!(err, SlackError::Http(_) | SlackError::Api(_)),
        "got {err:?}"
    );
}

#[tokio::test]
async fn search_messages_saved_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "messages": {
                "matches": [
                    {
                        "ts": "1700000000.000100",
                        "channel": {"id": "C1"},
                        "user": "U1",
                        "text": "saved item",
                        "permalink": "https://example.slack.com/archives/C1/p1700000000000100"
                    }
                ],
                "pagination": {"next_cursor": "cursor-2"}
            },
            "response_metadata": {"next_cursor": "cursor-2"}
        })))
        .mount(&server)
        .await;

    let api = SlackApiClient::with_base_url("xoxp-test", &server.uri()).unwrap();
    let resp = api.search_messages_saved(None, 20).await.unwrap();
    assert_eq!(resp.messages.matches.len(), 1);
    assert_eq!(resp.messages.matches[0].ts, "1700000000.000100");
    assert_eq!(resp.messages.matches[0].channel.id, "C1");
    assert_eq!(resp.messages.matches[0].text.as_deref(), Some("saved item"));
}
