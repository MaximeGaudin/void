//! HTTP integration tests against a wiremock Unipile API.

use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::UnipileClient;
use crate::error::LinkedInError;

fn test_client(server: &MockServer) -> UnipileClient {
    UnipileClient::with_api_base(&format!("{}/api/v1", server.uri()), "test-api-key")
}

#[tokio::test]
async fn get_account_returns_linked_account() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/accounts/acc-1"))
        .and(header("X-API-KEY", "test-api-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "acc-1",
            "type": "LINKEDIN"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let account = client.get_account("acc-1").await.unwrap();
    assert_eq!(account.id.as_deref(), Some("acc-1"));
    assert_eq!(account.r#type.as_deref(), Some("LINKEDIN"));
}

#[tokio::test]
async fn get_account_unauthorized_returns_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/accounts/bad"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client.get_account("bad").await.unwrap_err();
    assert!(matches!(err, LinkedInError::Auth(_)));
}

#[tokio::test]
async fn send_message_in_chat_posts_multipart() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chats/chat-42/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "sent-msg-99"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let id = client
        .send_message_in_chat("chat-42", "Hello from void", None)
        .await
        .unwrap();
    assert_eq!(id, "sent-msg-99");
}

#[tokio::test]
async fn start_new_chat_returns_message_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/chats"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "message_id": "new-msg-1"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let id = client
        .start_new_chat(
            "acc-1",
            "ACoAABFBQBcBtnr0Y6FNrtQpItSVnTX8Sxzl7Jg",
            "Hi!",
            None,
        )
        .await
        .unwrap();
    assert_eq!(id, "new-msg-1");
}

#[tokio::test]
async fn get_user_profile_fetches_by_provider_id() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/users/ACo123"))
        .and(query_param("account_id", "acc-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "provider_id": "ACo123",
            "first_name": "Matthieu",
            "last_name": "Lambda",
            "public_identifier": "matthieulambda"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let profile = client.get_user_profile("acc-1", "ACo123").await.unwrap();
    assert_eq!(profile.provider_id, "ACo123");
    assert_eq!(profile.first_name.as_deref(), Some("Matthieu"));
}

#[tokio::test]
async fn get_chat_attendee_fetches_attendee() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/chat_attendees/att-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "att-1",
            "name": "Aubin Rioufol",
            "provider_id": "ACoAABFBQBcBtnr0Y6FNrtQpItSVnTX8Sxzl7Jg",
            "profile_url": "https://www.linkedin.com/in/aubin-rioufol"
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let attendee = client.get_chat_attendee("att-1").await.unwrap();
    assert_eq!(attendee.name.as_deref(), Some("Aubin Rioufol"));
}

#[tokio::test]
async fn list_chats_passes_cursor_and_after() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/chats"))
        .and(query_param("account_id", "acc-1"))
        .and(query_param("account_type", "LINKEDIN"))
        .and(query_param("cursor", "page-2"))
        .and(query_param("after", "2026-05-01T00:00:00.000Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "ChatList",
            "items": [{ "object": "Chat", "id": "c1" }],
            "cursor": null
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let list = client
        .list_chats(
            "acc-1",
            Some("page-2"),
            Some("2026-05-01T00:00:00.000Z"),
            50,
        )
        .await
        .unwrap();
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, "c1");
}

#[tokio::test]
async fn list_chat_messages_passes_after_filter() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/chats/chat-1/messages"))
        .and(query_param("after", "2026-05-01T00:00:00.000Z"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "object": "MessageList",
            "items": [{ "object": "Message", "id": "m1", "text": "hi", "is_sender": 0 }]
        })))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let list = client
        .list_chat_messages("chat-1", None, Some("2026-05-01T00:00:00.000Z"), 100)
        .await
        .unwrap();
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].id, "m1");
    assert!(list.items[0].is_syncable());
}

#[tokio::test]
async fn download_attachment_returns_bytes() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/messages/msg-1/attachments/att-1"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"pdf-bytes"))
        .mount(&server)
        .await;

    let client = test_client(&server);
    let bytes = client.download_attachment("msg-1", "att-1").await.unwrap();
    assert_eq!(bytes, b"pdf-bytes");
}
