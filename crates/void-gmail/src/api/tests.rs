use super::*;
use crate::error::GmailError;
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

// -- Happy-path parsing --

#[tokio::test]
async fn get_message_parses_threading_and_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "m1",
            "threadId": "t1",
            "snippet": "Hello there",
            "internalDate": "1741700000000",
            "labelIds": ["INBOX", "UNREAD"],
            "payload": {
                "mimeType": "text/plain",
                "headers": [
                    {"name": "From", "value": "sender@example.com"},
                    {"name": "Subject", "value": "Greetings"}
                ],
                "body": {"data": "SGVsbG8gV29ybGQ", "size": 11}
            }
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let msg = api.get_message("m1").await.unwrap();
    assert_eq!(msg.id.as_deref(), Some("m1"));
    assert_eq!(msg.thread_id.as_deref(), Some("t1"));
    assert_eq!(msg.snippet.as_deref(), Some("Hello there"));
    assert_eq!(
        msg.label_ids.as_ref().unwrap(),
        &vec!["INBOX".to_string(), "UNREAD".to_string()]
    );
    assert_eq!(
        msg.get_header("from").as_deref(),
        Some("sender@example.com")
    );
    assert_eq!(msg.get_header("Subject").as_deref(), Some("Greetings"));
    assert_eq!(msg.text_body().as_deref(), Some("Hello World"));
}

#[tokio::test]
async fn get_thread_parses_messages() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/t1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "t1",
            "snippet": "Conversation",
            "messages": [
                {"id": "m1", "threadId": "t1", "snippet": "first"},
                {"id": "m2", "threadId": "t1", "snippet": "second"}
            ]
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let thread = api.get_thread("t1").await.unwrap();
    assert_eq!(thread.id.as_deref(), Some("t1"));
    let msgs = thread.messages.unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].id.as_deref(), Some("m1"));
    assert_eq!(msgs[1].id.as_deref(), Some("m2"));
}

#[tokio::test]
async fn list_labels_parses_two_labels() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "labels": [
                {"id": "INBOX", "name": "INBOX", "type": "system"},
                {"id": "Label_1", "name": "Work", "type": "user"}
            ]
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let resp = api.list_labels().await.unwrap();
    let labels = resp.labels.unwrap();
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].id, "INBOX");
    assert_eq!(labels[1].name, "Work");
}

/// Regression: `list_history` must consume all internal pages (was a real bug).
#[tokio::test]
async fn list_history_consumes_two_pages() {
    let server = MockServer::start().await;
    // Page 1: has nextPageToken -> must trigger a second request.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                {"messagesAdded": [{"message": {"id": "m1", "threadId": "t1"}}]}
            ],
            "historyId": "100",
            "nextPageToken": "page2"
        })))
        .mount(&server)
        .await;
    // Page 2: terminal (no nextPageToken).
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .and(query_param("pageToken", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [
                {"messagesAdded": [{"message": {"id": "m2", "threadId": "t2"}}]}
            ],
            "historyId": "200"
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let resp = api.list_history("50", None).await.unwrap();
    let records = resp.history.unwrap();
    // Both pages must be present.
    assert_eq!(records.len(), 2);
    let ids: Vec<&str> = records
        .iter()
        .filter_map(|r| r.messages_added.as_ref())
        .flat_map(|ma| ma.iter().map(|m| m.message.id.as_str()))
        .collect();
    assert_eq!(ids, vec!["m1", "m2"]);
    // Latest history id is from the last page; aggregated token cleared.
    assert_eq!(resp.history_id.as_deref(), Some("200"));
    assert!(resp.next_page_token.is_none());
}

// -- Error paths --

/// `list_messages` goes straight to `.json()`, so an error body is a DECODE error.
#[tokio::test]
async fn list_messages_401_surfaces_decode_error_not_panic() {
    let server = MockServer::start().await;
    // A real Gmail 401 returns an error body whose `messages` (if present) is not
    // an array; here the top-level is an array, which cannot decode into the
    // struct -> reqwest decode error (never a panic).
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(
            ResponseTemplate::new(401).set_body_json(serde_json::json!(["invalid", "credentials"])),
        )
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .list_messages(10, None, None, None)
        .await
        .expect_err("expected error");
    // Error body does not match MessageListResponse -> reqwest decode error.
    assert!(matches!(err, GmailError::Http(_)), "got {err:?}");
}

/// `get_message` also decodes directly; 5xx with non-matching body -> decode error.
#[tokio::test]
async fn get_message_5xx_surfaces_decode_error() {
    let server = MockServer::start().await;
    // Non-JSON / non-object body cannot decode into GmailMessage -> decode error.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api.get_message("m1").await.expect_err("expected error");
    assert!(matches!(err, GmailError::Http(_)), "got {err:?}");
}

/// `list_labels` calls `.error_for_status()`, so HTTP status is preserved.
#[tokio::test]
async fn list_labels_401_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/labels"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {"code": 401, "message": "Invalid Credentials"}
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api.list_labels().await.expect_err("expected error");
    match err {
        GmailError::Http(e) => assert_eq!(e.status(), Some(reqwest::StatusCode::UNAUTHORIZED)),
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// `get_thread` preserves status via `.error_for_status()`.
#[tokio::test]
async fn get_thread_500_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/threads/t1"))
        .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api.get_thread("t1").await.expect_err("expected error");
    match err {
        GmailError::Http(e) => {
            assert_eq!(e.status(), Some(reqwest::StatusCode::INTERNAL_SERVER_ERROR))
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// `create_draft` preserves status (e.g. 429 rate-limit) via `.error_for_status()`.
#[tokio::test]
async fn create_draft_429_preserves_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .create_draft("cmF3", None)
        .await
        .expect_err("expected error");
    match err {
        GmailError::Http(e) => {
            assert_eq!(e.status(), Some(reqwest::StatusCode::TOO_MANY_REQUESTS))
        }
        other => panic!("expected Http error, got {other:?}"),
    }
}

/// Malformed JSON (missing required `id` on a MessageRef) -> clean Err, no panic.
#[tokio::test]
async fn list_messages_malformed_json_is_clean_err() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"threadId": "t1"}]
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let err = api
        .list_messages(10, None, None, None)
        .await
        .expect_err("expected decode error for missing id");
    assert!(matches!(err, GmailError::Http(_)), "got {err:?}");
}
