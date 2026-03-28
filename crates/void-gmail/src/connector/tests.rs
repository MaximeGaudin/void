use super::api_methods::{build_reply_all_recipients, create_draft_with_api};
use super::*;
use crate::api::{GmailApiClient, GmailMessage};
use base64::Engine;
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_message(from: &str, to: &str, cc: Option<&str>) -> GmailMessage {
    let mut headers = vec![
        serde_json::json!({"name": "From", "value": from}),
        serde_json::json!({"name": "To",   "value": to}),
    ];
    if let Some(c) = cc {
        headers.push(serde_json::json!({"name": "Cc", "value": c}));
    }
    serde_json::from_value(serde_json::json!({
        "id": "m1",
        "threadId": "t1",
        "payload": { "headers": headers }
    }))
    .unwrap()
}

#[tokio::test]
async fn api_list_messages_paginates() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [
                {"id": "m1", "threadId": "t1"},
                {"id": "m2", "threadId": "t1"}
            ],
            "nextPageToken": "page2"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .and(query_param("pageToken", "page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [
                {"id": "m3", "threadId": "t2"}
            ]
        })))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());

    let mut all_messages = Vec::new();
    let mut page_token: Option<String> = None;

    loop {
        let resp = api
            .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
            .await
            .unwrap();
        if let Some(msgs) = resp.messages {
            all_messages.extend(msgs);
        }
        page_token = resp.next_page_token;
        if page_token.is_none() {
            break;
        }
    }

    assert_eq!(all_messages.len(), 3);
    assert_eq!(all_messages[0].id, "m1");
    assert_eq!(all_messages[1].id, "m2");
    assert_eq!(all_messages[2].id, "m3");
}

#[tokio::test]
async fn initial_sync_saves_history_id() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "emailAddress": "test@example.com",
            "historyId": "12345"
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"id": "m1", "threadId": "t1"}]
        })))
        .mount(&server)
        .await;

    let full_message = serde_json::json!({
        "id": "m1",
        "threadId": "t1",
        "snippet": "Hello",
        "internalDate": "1741700000000",
        "labelIds": ["INBOX"],
        "payload": {
            "mimeType": "text/plain",
            "headers": [
                {"name": "From", "value": "sender@example.com"},
                {"name": "Subject", "value": "Test Subject"},
                {"name": "Date", "value": "Wed, 11 Mar 2026 10:00:00 +0000"}
            ],
            "body": {"data": "SGVsbG8gV29ybGQ", "size": 11}
        }
    });

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(full_message))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let db = Database::open_in_memory().unwrap();

    let config_id = "test-gmail";
    let profile = api.get_profile().await.unwrap();

    if let Some(history_id) = &profile.history_id {
        db.set_sync_state(config_id, "history_id", history_id)
            .unwrap();
    }

    let mut page_token: Option<String> = None;
    loop {
        let resp = api
            .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
            .await
            .unwrap();
        if let Some(msgs) = resp.messages {
            for msg_ref in &msgs {
                let msg = api.get_message(&msg_ref.id).await.unwrap();
                let msg_id = msg.id.as_deref().unwrap_or("");
                let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
                let from = msg.get_header("From").unwrap_or_default();
                let connection_id = profile
                    .email_address
                    .as_deref()
                    .unwrap_or(config_id)
                    .to_string();
                let conv_id = format!("{}-{}", connection_id, thread_id);
                let subject = msg
                    .get_header("Subject")
                    .unwrap_or_else(|| "(no subject)".into());

                let conversation = Conversation {
                    id: conv_id.clone(),
                    connection_id: connection_id.clone(),
                    connector: "gmail".into(),
                    external_id: thread_id.to_string(),
                    name: Some(subject),
                    kind: ConversationKind::Thread,
                    last_message_at: msg
                        .internal_date
                        .as_deref()
                        .and_then(|d| d.parse().ok())
                        .map(|ms: i64| ms / 1000),
                    unread_count: 0,
                    is_muted: false,
                    metadata: None,
                };
                db.upsert_conversation(&conversation).unwrap();

                let message = Message {
                    id: format!("{}-{}", connection_id, msg_id),
                    conversation_id: conv_id,
                    connection_id: connection_id.clone(),
                    connector: "gmail".into(),
                    external_id: msg_id.to_string(),
                    sender: from
                        .find('<')
                        .map(|i| from[i + 1..].trim_end_matches('>').trim().to_string())
                        .unwrap_or_else(|| from.clone()),
                    sender_name: None,
                    body: msg.text_body().or(msg.snippet.clone()),
                    timestamp: msg
                        .internal_date
                        .as_deref()
                        .and_then(|d| d.parse().ok())
                        .map(|ms: i64| ms / 1000)
                        .unwrap_or(0),
                    synced_at: None,
                    is_archived: false,
                    reply_to_id: None,
                    media_type: None,
                    metadata: None,
                    context_id: Some(format!("{}-thread-{}", connection_id, thread_id)),
                    context: None,
                };
                db.upsert_message(&message).unwrap();
            }
        }
        page_token = resp.next_page_token;
        if page_token.is_none() {
            break;
        }
    }

    let history_id = db.get_sync_state(config_id, "history_id").unwrap();
    assert_eq!(history_id, Some("12345".to_string()));

    let msg = db
        .get_message("test@example.com-m1")
        .unwrap()
        .expect("message should be stored");
    assert_eq!(msg.external_id, "m1");
    assert_eq!(msg.body.as_deref(), Some("Hello World"));
}

#[tokio::test]
async fn incremental_sync_uses_history_id() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/history"))
        .and(query_param("startHistoryId", "12345"))
        .and(query_param("labelId", "INBOX"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "history": [{
                "messagesAdded": [{
                    "message": {"id": "m2", "threadId": "t2"}
                }]
            }],
            "historyId": "12346"
        })))
        .mount(&server)
        .await;

    let full_message = serde_json::json!({
        "id": "m2",
        "threadId": "t2",
        "snippet": "New message",
        "internalDate": "1741700001000",
        "labelIds": ["INBOX"],
        "payload": {
            "mimeType": "text/plain",
            "headers": [
                {"name": "From", "value": "other@example.com"},
                {"name": "Subject", "value": "Re: Test"},
                {"name": "Date", "value": "Wed, 11 Mar 2026 10:01:00 +0000"}
            ],
            "body": {"data": "TmV3IG1lc3NhZ2U=", "size": 11}
        }
    });

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/m2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(full_message))
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let db = Database::open_in_memory().unwrap();
    db.set_sync_state("test-gmail", "history_id", "12345")
        .unwrap();

    let config_id = "test-gmail";
    let history_id = db.get_sync_state(config_id, "history_id").unwrap();
    let history_id = history_id.expect("history_id should be set");

    let resp = api.list_history(&history_id, Some("INBOX")).await.unwrap();

    if let Some(records) = resp.history {
        for record in &records {
            if let Some(added) = &record.messages_added {
                for item in added {
                    let msg = api.get_message(&item.message.id).await.unwrap();
                    let msg_id = msg.id.as_deref().unwrap_or("");
                    let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
                    let connection_id = "test-gmail".to_string();
                    let conv_id = format!("{}-{}", connection_id, thread_id);

                    let conversation = Conversation {
                        id: conv_id.clone(),
                        connection_id: connection_id.clone(),
                        connector: "gmail".into(),
                        external_id: thread_id.to_string(),
                        name: Some(
                            msg.get_header("Subject")
                                .unwrap_or_else(|| "(no subject)".into()),
                        ),
                        kind: ConversationKind::Thread,
                        last_message_at: msg
                            .internal_date
                            .as_deref()
                            .and_then(|d| d.parse().ok())
                            .map(|ms: i64| ms / 1000),
                        unread_count: 0,
                        is_muted: false,
                        metadata: None,
                    };
                    db.upsert_conversation(&conversation).unwrap();

                    let from = msg.get_header("From").unwrap_or_default();
                    let message = Message {
                        id: format!("{}-{}", connection_id, msg_id),
                        conversation_id: conv_id.clone(),
                        connection_id: connection_id.clone(),
                        connector: "gmail".into(),
                        external_id: msg_id.to_string(),
                        sender: from
                            .find('<')
                            .map(|i| from[i + 1..].trim_end_matches('>').trim().to_string())
                            .unwrap_or_else(|| from.clone()),
                        sender_name: None,
                        body: msg.text_body().or(msg.snippet.clone()),
                        timestamp: msg
                            .internal_date
                            .as_deref()
                            .and_then(|d| d.parse().ok())
                            .map(|ms: i64| ms / 1000)
                            .unwrap_or(0),
                        synced_at: None,
                        is_archived: false,
                        reply_to_id: None,
                        media_type: None,
                        metadata: None,
                        context_id: Some(format!("{}-thread-{}", connection_id, thread_id)),
                        context: None,
                    };
                    db.upsert_message(&message).unwrap();
                }
            }
        }
    }

    if let Some(new_id) = resp.history_id {
        db.set_sync_state(config_id, "history_id", &new_id).unwrap();
    }

    let updated = db.get_sync_state(config_id, "history_id").unwrap();
    assert_eq!(updated, Some("12346".to_string()));

    let msg = db
        .get_message("test-gmail-m2")
        .unwrap()
        .expect("message should be stored");
    assert_eq!(msg.external_id, "m2");
    assert_eq!(msg.body.as_deref(), Some("New message"));
}

#[tokio::test]
async fn initial_sync_respects_max_pages() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "messages": [{"id": "m1", "threadId": "t1"}],
            "nextPageToken": "next"
        })))
        .expect(5)
        .named("list_messages pages")
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());

    let max_pages: u64 = 5;
    let mut page_token: Option<String> = None;
    let mut page_count = 0u64;

    while page_count < max_pages {
        let resp = api
            .list_messages(100, page_token.as_deref(), Some(&["INBOX"]), None)
            .await
            .unwrap();
        page_count += 1;
        page_token = resp.next_page_token;
        if page_token.is_none() {
            break;
        }
    }

    assert_eq!(page_count, 5);
    drop(server);
}

#[test]
fn parse_email_address_extracts_email() {
    assert_eq!(
        parse_email_address("Alice <alice@example.com>"),
        "alice@example.com"
    );
    assert_eq!(
        parse_email_address("\"Bob Smith\" <bob@example.com>"),
        "bob@example.com"
    );
    assert_eq!(
        parse_email_address("charlie@example.com"),
        "charlie@example.com"
    );
}

#[test]
fn parse_email_name_with_brackets() {
    assert_eq!(parse_email_name("Alice <alice@example.com>"), "Alice");
    assert_eq!(
        parse_email_name("\"Bob Smith\" <bob@example.com>"),
        "Bob Smith"
    );
    assert_eq!(
        parse_email_name("charlie@example.com"),
        "charlie@example.com"
    );
}

#[test]
fn compose_rfc2822_basic() {
    let raw = compose_rfc2822("alice@example.com", "Test Subject", "Hello, Alice!");
    assert!(raw.contains("To: alice@example.com"));
    assert!(raw.contains("Subject: Test Subject"));
    assert!(raw.contains("Hello, Alice!"));
}

#[test]
fn compose_rfc2822_with_attachment_creates_multipart() {
    let dir = std::env::temp_dir();
    let name = format!("void_gmail_test_{}.txt", uuid::Uuid::new_v4());
    let path = dir.join(&name);
    std::fs::write(&path, "test content").unwrap();
    let result =
        compose_rfc2822_with_attachment("a@b.com", "Subj", "body", &path, None, None, None)
            .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(result.contains("void_boundary_001"));
    assert!(result.contains("Content-Type: multipart/mixed"));
    assert!(result.contains("Content-Transfer-Encoding: base64"));
    assert!(result.contains("dGVzdCBjb250ZW50"));
    assert!(result.contains(&name));
    assert!(result.contains("To: a@b.com"));
    assert!(result.contains("Subject: Subj"));
    assert!(result.contains("Content-Disposition: attachment"));
}

#[test]
fn compose_rfc2822_with_attachment_uses_provided_mime_type() {
    let dir = std::env::temp_dir();
    let name = format!("void_gmail_test_{}.pdf", uuid::Uuid::new_v4());
    let path = dir.join(&name);
    std::fs::write(&path, "PDF bytes").unwrap();
    let result = compose_rfc2822_with_attachment(
        "x@y.com",
        "Doc",
        "See attached",
        &path,
        Some("application/pdf"),
        None,
        None,
    )
    .unwrap();
    std::fs::remove_file(&path).ok();
    assert!(result.contains("Content-Type: application/pdf"));
}

#[test]
fn compose_rfc2822_encodes_non_ascii_subject() {
    let raw = compose_rfc2822("a@b.com", "Séjour — Réservation", "body");
    assert!(raw.contains("Subject: =?UTF-8?B?"));
    assert!(!raw.contains("Séjour"));
}

#[test]
fn compose_rfc2822_ascii_subject_unchanged() {
    let raw = compose_rfc2822("a@b.com", "Hello World", "body");
    assert!(raw.contains("Subject: Hello World"));
}

#[test]
fn html_to_markdown_strips_tags() {
    let md = html_to_markdown("<p>Hello <b>world</b></p>");
    assert!(md.contains("Hello"));
    assert!(md.contains("**world**"));
    assert!(!md.contains("<p>"));
    assert!(!md.contains("<b>"));
}

#[test]
fn html_to_markdown_preserves_links() {
    let md = html_to_markdown(r#"<p>Click <a href="https://example.com">here</a></p>"#);
    assert!(md.contains("[here](https://example.com)"));
}

#[test]
fn html_to_markdown_preserves_headings() {
    let md = html_to_markdown("<h1>Title</h1><h2>Subtitle</h2>");
    assert!(md.contains("# Title"));
    assert!(md.contains("## Subtitle"));
}

#[test]
fn html_to_markdown_handles_lists() {
    let md = html_to_markdown("<ul><li>Item 1</li><li>Item 2</li></ul>");
    assert!(md.contains("Item 1"));
    assert!(md.contains("Item 2"));
}

#[test]
fn html_to_markdown_real_email() {
    let html = r#"
    <html>
    <body>
        <div style="font-family:Arial">
            <p>Hi Maxime,</p>
            <p>Your order <b>#12345</b> has been shipped.</p>
            <p>Track it <a href="https://track.example.com/12345">here</a>.</p>
            <br>
            <p>Thanks,<br>The Team</p>
        </div>
    </body>
    </html>
    "#;
    let md = html_to_markdown(html);
    assert!(md.contains("Hi Maxime"));
    assert!(md.contains("**#12345**"));
    assert!(md.contains("has been shipped"));
    assert!(md.contains("[here](https://track.example.com/12345)"));
    assert!(md.contains("Thanks"));
    assert!(!md.contains("<div"));
    assert!(!md.contains("font-family"));
}

#[test]
fn html_to_markdown_empty() {
    let md = html_to_markdown("");
    assert!(md.trim().is_empty());
}

#[test]
fn looks_like_html_detects_doctype() {
    assert!(looks_like_html(
        "<!DOCTYPE html><html><body>Hi</body></html>"
    ));
    assert!(looks_like_html("  <!DOCTYPE html>\n<html>"));
}

#[test]
fn looks_like_html_detects_html_tag() {
    assert!(looks_like_html("<html><body>Hello</body></html>"));
    assert!(looks_like_html("<HTML><BODY>Hello</BODY></HTML>"));
}

#[test]
fn looks_like_html_detects_div_table_body() {
    assert!(looks_like_html("<div class=\"wrapper\">Content</div>"));
    assert!(looks_like_html("<table><tr><td>cell</td></tr></table>"));
    assert!(looks_like_html("<body>hello</body>"));
}

#[test]
fn looks_like_html_plain_text_is_false() {
    assert!(!looks_like_html("Hello, this is a plain text email."));
    assert!(!looks_like_html("Hi Maxime,\n\nSee you tomorrow.\nAlice"));
    assert!(!looks_like_html(""));
}

#[test]
fn gmail_url_formats_correctly() {
    let url = GmailConnector::gmail_url("thread123");
    assert_eq!(url, "https://mail.google.com/mail/u/0/#inbox/thread123");
}

// ---------------------------------------------------------------------------
// build_reply_all_recipients
// ---------------------------------------------------------------------------

#[test]
fn reply_all_includes_sender_excludes_own() {
    let msg = make_message("alice@example.com", "me@example.com", None);
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert_eq!(result, "alice@example.com");
}

#[test]
fn reply_all_includes_all_to_recipients() {
    let msg = make_message("alice@example.com", "me@example.com, bob@example.com", None);
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert!(result.contains("alice@example.com"), "should include From");
    assert!(
        result.contains("bob@example.com"),
        "should include other To"
    );
    assert!(
        !result.contains("me@example.com"),
        "should exclude own address"
    );
}

#[test]
fn reply_all_includes_cc_recipients() {
    let msg = make_message(
        "alice@example.com",
        "me@example.com",
        Some("carol@example.com, dave@example.com"),
    );
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert!(result.contains("alice@example.com"));
    assert!(result.contains("carol@example.com"));
    assert!(result.contains("dave@example.com"));
    assert!(!result.contains("me@example.com"));
}

#[test]
fn reply_all_deduplicates_addresses() {
    // alice appears in both From and To
    let msg = make_message(
        "alice@example.com",
        "alice@example.com, me@example.com",
        None,
    );
    let result = build_reply_all_recipients(&msg, "me@example.com");
    let count = result.matches("alice@example.com").count();
    assert_eq!(count, 1, "alice should appear only once");
}

#[test]
fn reply_all_preserves_display_names() {
    let msg = make_message("Alice Smith <alice@example.com>", "me@example.com", None);
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert_eq!(result, "Alice Smith <alice@example.com>");
}

#[test]
fn reply_all_own_exclusion_is_case_insensitive() {
    let msg = make_message("alice@example.com", "ME@EXAMPLE.COM", None);
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert!(
        !result.contains("ME@EXAMPLE.COM"),
        "own address excluded regardless of case"
    );
    assert!(result.contains("alice@example.com"));
}

#[test]
fn reply_all_empty_when_only_own_address() {
    let msg = make_message("me@example.com", "me@example.com", None);
    let result = build_reply_all_recipients(&msg, "me@example.com");
    assert!(
        result.is_empty(),
        "no recipients left when own address is the only one"
    );
}

// ---------------------------------------------------------------------------
// GmailApiClient::create_draft (low-level)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn api_create_draft_posts_raw_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "draft1",
            "message": { "id": "m1", "threadId": "t1" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("To: alice@example.com\r\nSubject: Test\r\n\r\nBody".as_bytes());

    let draft = api.create_draft(&raw, Some("t1")).await.unwrap();
    assert_eq!(draft.id.as_deref(), Some("draft1"));
}

#[tokio::test]
async fn api_create_draft_without_thread_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "draft2",
            "message": { "id": "m2" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode("To: bob@example.com\r\nSubject: No thread\r\n\r\nHi".as_bytes());

    let draft = api.create_draft(&raw, None).await.unwrap();
    assert_eq!(draft.id.as_deref(), Some("draft2"));
}

// ---------------------------------------------------------------------------
// create_draft_with_api — thread_id auto-derivation
// ---------------------------------------------------------------------------

/// When --reply-to is given, the draft must be associated with the original
/// message's thread (threadId forwarded in the API request body).
#[tokio::test]
async fn create_draft_derives_thread_id_from_reply_to_message() {
    let server = MockServer::start().await;

    // Original message returned when fetching the reply-to ID.
    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/msg1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg1",
            "threadId": "thread_abc",
            "payload": {
                "headers": [
                    {"name": "From", "value": "alice@example.com"},
                    {"name": "To",   "value": "me@example.com"}
                ]
            }
        })))
        .mount(&server)
        .await;

    // Verify the draft creation request includes the derived threadId.
    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .and(wiremock::matchers::body_partial_json(serde_json::json!({
            "message": { "threadId": "thread_abc" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "draft99",
            "message": { "id": "new_msg", "threadId": "thread_abc" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    let draft = create_draft_with_api(
        &api,
        "me@example.com",
        Some("alice@example.com"),
        "Re: Hello",
        "Thanks!",
        Some("msg1"),
        None,
    )
    .await
    .unwrap();

    assert_eq!(draft.id.as_deref(), Some("draft99"));
}

/// When --to is omitted, recipients are auto-derived reply-all from the
/// original message AND the thread_id is still forwarded correctly.
#[tokio::test]
async fn create_draft_derives_thread_id_and_recipients_together() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gmail/v1/users/me/messages/msg2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg2",
            "threadId": "thread_xyz",
            "payload": {
                "headers": [
                    {"name": "From", "value": "bob@example.com"},
                    {"name": "To",   "value": "me@example.com, carol@example.com"}
                ]
            }
        })))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/gmail/v1/users/me/drafts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "draft88",
            "message": { "id": "new2", "threadId": "thread_xyz" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let api = GmailApiClient::with_base_url("test-token", &server.uri());
    // to=None → should be auto-derived from msg headers
    let draft = create_draft_with_api(
        &api,
        "me@example.com",
        None,
        "Re: Chat",
        "Got it.",
        Some("msg2"),
        None,
    )
    .await
    .unwrap();

    assert_eq!(draft.id.as_deref(), Some("draft88"));
}

/// Without --reply-to AND without --to, the call must return an error.
#[tokio::test]
async fn create_draft_errors_without_to_and_reply_to() {
    let server = MockServer::start().await;
    let api = GmailApiClient::with_base_url("test-token", &server.uri());

    let result =
        create_draft_with_api(&api, "me@example.com", None, "Subject", "Body", None, None).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("--to is required"));
}
