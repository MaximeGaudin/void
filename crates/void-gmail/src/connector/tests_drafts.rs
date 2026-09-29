//! Draft update / reply-header and search timestamp tests.

use super::api_methods::{create_draft_with_api, update_draft_with_api};
use super::*;
use crate::api::{GmailApiClient, GmailMessage};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Existing reply draft on `thread_orig`, carrying reply headers and one
/// attachment (`notes.txt` = "hello").
async fn mount_existing_reply_draft(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/drafts/r-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "r-1",
            "message": {
                "id": "draftmsg",
                "threadId": "thread_orig",
                "payload": {
                    "mimeType": "multipart/mixed",
                    "headers": [
                        {"name": "To", "value": "alice@example.com"},
                        {"name": "Subject", "value": "Re: Hello"},
                        {"name": "In-Reply-To", "value": "<orig@mail.example.com>"},
                        {"name": "References", "value": "<root@mail.example.com> <orig@mail.example.com>"}
                    ],
                    "parts": [
                        {"mimeType": "text/html", "body": {"data": "aGk", "size": 2}},
                        {"mimeType": "text/plain", "filename": "notes.txt",
                         "body": {"attachmentId": "att1", "size": 5}}
                    ]
                }
            }
        })))
        .mount(server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/gmail/v1/users/me/drafts/r-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "r-1",
            "message": { "id": "draftmsg2", "threadId": "thread_orig" }
        })))
        .expect(1)
        .mount(server)
        .await;
}

async fn mount_attachment(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(
            "/gmail/v1/users/me/messages/draftmsg/attachments/att1",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": "aGVsbG8", "size": 5
        })))
        .mount(server)
        .await;
}

/// Body JSON and decoded RFC 2822 raw of the first `verb` request sent.
async fn sent_raw(server: &MockServer, verb: &str) -> (serde_json::Value, String) {
    let req = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method.as_str() == verb)
        .expect("draft write request");
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    let raw = URL_SAFE_NO_PAD
        .decode(body["message"]["raw"].as_str().unwrap())
        .unwrap();
    (body, String::from_utf8(raw).unwrap())
}

fn to_alice() -> ComposeRecipients<'static> {
    ComposeRecipients::to_only("alice@example.com")
}

#[tokio::test]
async fn update_draft_keeps_thread_reply_headers_and_attachments() {
    let server = MockServer::start().await;
    mount_existing_reply_draft(&server).await;
    mount_attachment(&server).await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let draft = update_draft_with_api(&api, "r-1", to_alice(), "Re: Hello", "New", None, None)
        .await
        .unwrap();
    assert_eq!(draft.id.as_deref(), Some("r-1"));

    let (body, raw) = sent_raw(&server, "PUT").await;
    assert_eq!(body["message"]["threadId"], "thread_orig");
    assert!(raw.contains("In-Reply-To: <orig@mail.example.com>\r\n"));
    assert!(raw.contains("References: <root@mail.example.com> <orig@mail.example.com>\r\n"));
    assert!(raw.contains("Content-Type: text/plain; name=\"notes.txt\""));
    // "hello" in standard base64
    assert!(raw.contains("aGVsbG8="));
}

#[tokio::test]
async fn update_draft_with_file_replaces_attachments_but_keeps_thread() {
    let server = MockServer::start().await;
    mount_existing_reply_draft(&server).await;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("shot.png");
    std::fs::write(&file, b"png").unwrap();

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    update_draft_with_api(
        &api,
        "r-1",
        to_alice(),
        "Re: Hello",
        "With screenshot",
        None,
        Some(&file),
    )
    .await
    .unwrap();

    let (body, raw) = sent_raw(&server, "PUT").await;
    assert_eq!(body["message"]["threadId"], "thread_orig");
    assert!(raw.contains("In-Reply-To: <orig@mail.example.com>\r\n"));
    assert!(raw.contains("filename=\"shot.png\""));
    assert!(!raw.contains("notes.txt"));
}

#[tokio::test]
async fn update_draft_reply_to_retargets_thread_and_headers() {
    let server = MockServer::start().await;
    mount_existing_reply_draft(&server).await;
    mount_attachment(&server).await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/other"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "other",
            "threadId": "thread_other",
            "payload": { "headers": [
                {"name": "Message-ID", "value": "<other@mail.example.com>"}
            ]}
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    update_draft_with_api(
        &api,
        "r-1",
        to_alice(),
        "Re: Other",
        "Body",
        Some("other"),
        None,
    )
    .await
    .unwrap();

    let (body, raw) = sent_raw(&server, "PUT").await;
    assert_eq!(body["message"]["threadId"], "thread_other");
    assert!(raw.contains("In-Reply-To: <other@mail.example.com>\r\n"));
    assert!(raw.contains("References: <other@mail.example.com>\r\n"));
}

#[tokio::test]
async fn create_draft_reply_headers_use_message_id_header() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/msg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg1",
            "threadId": "thread_abc",
            "payload": { "headers": [
                {"name": "From", "value": "alice@example.com"},
                {"name": "Message-ID", "value": "<m1@mail.example.com>"},
                {"name": "References", "value": "<root@mail.example.com>"}
            ]}
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "id": "d1" })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    create_draft_with_api(
        &api,
        "me@example.com",
        DraftRecipients {
            to: Some("alice@example.com"),
            cc: None,
            bcc: None,
        },
        "Re: Hello",
        "Thanks",
        Some("msg1"),
        None,
    )
    .await
    .unwrap();

    let (body, raw) = sent_raw(&server, "POST").await;
    assert_eq!(body["message"]["threadId"], "thread_abc");
    assert!(raw.contains("In-Reply-To: <m1@mail.example.com>\r\n"));
    assert!(raw.contains("References: <root@mail.example.com> <m1@mail.example.com>\r\n"));
}

#[test]
fn compose_with_attachments_embeds_each_file_and_falls_back_without() {
    let atts = vec![
        OutgoingAttachment {
            filename: "a.txt".into(),
            mime_type: "text/plain".into(),
            data: b"A".to_vec(),
        },
        OutgoingAttachment {
            filename: "b\"\r\nX-Evil: 1.pdf".into(),
            mime_type: "application/pdf".into(),
            data: b"B".to_vec(),
        },
    ];
    let to = ComposeRecipients::to_only("x@example.com");
    let raw = compose_rfc2822_with_attachments(to, "S", "body", &atts, None, None).unwrap();
    assert!(raw.contains("filename=\"a.txt\""));
    assert!(raw.contains("filename=\"b___X-Evil: 1.pdf\""));
    assert!(!raw.contains("\r\nX-Evil"));
    assert!(raw.ends_with("--void_boundary_001--"));

    let plain = compose_rfc2822_with_attachments(to, "S", "body", &[], None, None).unwrap();
    assert_eq!(
        plain,
        compose_rfc2822_ex(to, "S", "body", None, None, None).unwrap()
    );
}

#[test]
fn timestamp_rfc3339_uses_internal_date_then_date_header() {
    let from_internal: GmailMessage = serde_json::from_value(serde_json::json!({
        "id": "m", "internalDate": "1790000000123"
    }))
    .unwrap();
    assert_eq!(
        from_internal.timestamp_rfc3339().as_deref(),
        Some("2026-09-21T14:13:20Z")
    );

    let from_header: GmailMessage = serde_json::from_value(serde_json::json!({
        "id": "m",
        "payload": { "headers": [
            {"name": "Date", "value": "Fri, 25 Sep 2026 15:56:02 +0200"}
        ]}
    }))
    .unwrap();
    assert_eq!(
        from_header.timestamp_rfc3339().as_deref(),
        Some("2026-09-25T13:56:02Z")
    );

    let none: GmailMessage = serde_json::from_value(serde_json::json!({ "id": "m" })).unwrap();
    assert_eq!(none.timestamp_rfc3339(), None);
}
