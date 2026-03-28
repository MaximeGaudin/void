use std::collections::HashMap;

use void_core::connector::Connector;
use void_core::models::{Conversation, ConversationKind, Message};

use super::*;
use crate::api::{SlackConversation, SlackReaction};

#[test]
fn map_conversation_dm() {
    let conv = SlackConversation {
        id: "D123".into(),
        name: None,
        is_channel: Some(false),
        is_group: Some(false),
        is_im: Some(true),
        is_mpim: Some(false),
        is_private: Some(true),
        user: Some("U456".into()),
        updated: None,
    };
    let mut cache = HashMap::new();
    cache.insert("U456".to_string(), "Alice".to_string());
    let result = map_conversation(&conv, "work-slack", &cache);
    assert_eq!(result.kind, ConversationKind::Dm);
    assert_eq!(result.connector, "slack");
    assert_eq!(result.name.as_deref(), Some("Alice"));
    assert_eq!(result.external_id, "D123");
}

#[test]
fn map_conversation_channel() {
    let conv = SlackConversation {
        id: "C789".into(),
        name: Some("general".into()),
        is_channel: Some(true),
        is_group: Some(false),
        is_im: Some(false),
        is_mpim: Some(false),
        is_private: Some(false),
        user: None,
        updated: None,
    };
    let result = map_conversation(&conv, "work-slack", &HashMap::new());
    assert_eq!(result.kind, ConversationKind::Channel);
    assert_eq!(result.connector, "slack");
    assert_eq!(result.name.as_deref(), Some("general"));
}

#[test]
fn parse_slack_ts() {
    assert_eq!(parse_ts("1700000000.123456"), Some(1_700_000_000));
    assert_eq!(parse_ts("invalid"), None);
}

#[test]
fn build_metadata_channel_no_reactions() {
    let conv = SlackConversation {
        id: "C789".into(),
        name: Some("general".into()),
        is_channel: Some(true),
        is_group: Some(false),
        is_im: Some(false),
        is_mpim: Some(false),
        is_private: Some(false),
        user: None,
        updated: None,
    };
    let meta = build_metadata(&conv, &[], &HashMap::new()).unwrap();
    assert_eq!(meta["channel_id"], "C789");
    assert_eq!(meta["channel_name"], "general");
    assert_eq!(meta["channel_kind"], "channel");
    assert_eq!(meta["is_private"], false);
    assert!(meta.get("reactions").is_none());
}

#[test]
fn build_metadata_dm_with_reactions() {
    let conv = SlackConversation {
        id: "D123".into(),
        name: None,
        is_channel: Some(false),
        is_group: Some(false),
        is_im: Some(true),
        is_mpim: Some(false),
        is_private: Some(true),
        user: Some("U456".into()),
        updated: None,
    };
    let reactions = vec![
        SlackReaction {
            name: "thumbsup".into(),
            count: 3,
            users: vec![],
        },
        SlackReaction {
            name: "heart".into(),
            count: 1,
            users: vec![],
        },
    ];
    let mut cache = HashMap::new();
    cache.insert("U456".to_string(), "Bob".to_string());
    let meta = build_metadata(&conv, &reactions, &cache).unwrap();
    assert_eq!(meta["channel_id"], "D123");
    assert_eq!(meta["channel_name"], "Bob");
    assert_eq!(meta["channel_kind"], "dm");
    assert_eq!(meta["is_private"], true);
    let r = meta["reactions"].as_array().unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0]["name"], "thumbsup");
    assert_eq!(r[0]["count"], 3);
    assert_eq!(r[1]["name"], "heart");
}

#[test]
fn build_metadata_private_channel() {
    let conv = SlackConversation {
        id: "G111".into(),
        name: Some("secret-project".into()),
        is_channel: Some(false),
        is_group: Some(true),
        is_im: Some(false),
        is_mpim: Some(false),
        is_private: Some(true),
        user: None,
        updated: None,
    };
    let meta = build_metadata(&conv, &[], &HashMap::new()).unwrap();
    assert_eq!(meta["channel_kind"], "private_channel");
    assert_eq!(meta["is_private"], true);
    assert_eq!(meta["channel_name"], "secret-project");
}

// --- Integration tests (wiremock) ---

#[tokio::test]
async fn backfill_stores_conversations_and_messages() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({
        "ok": true,
        "members": [
            {
                "id": "U1",
                "name": "alice",
                "real_name": "Alice",
                "profile": {"display_name": "Alice", "real_name": "Alice"}
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let channels = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "general",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
        .mount(&server)
        .await;

    let history = serde_json::json!({
        "ok": true,
        "messages": [
            {
                "ts": "1741700000.000100",
                "user": "U1",
                "text": "Hello world"
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .and(wiremock::matchers::query_param("channel", "C1"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
        .mount(&server)
        .await;

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let db = void_core::db::Database::open_in_memory().unwrap();
    connector.backfill(&db).await.unwrap();

    let conv = db.get_conversation("test-slack-C1").unwrap().unwrap();
    assert_eq!(conv.name.as_deref(), Some("general"));

    let msg = db
        .get_message("test-slack-1741700000.000100")
        .unwrap()
        .unwrap();
    assert_eq!(msg.body.as_deref(), Some("Hello world"));
}

#[tokio::test]
async fn backfill_saves_done_state() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({"ok": true, "members": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let channels = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "general",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
        .mount(&server)
        .await;

    let history = serde_json::json!({"ok": true, "messages": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
        .mount(&server)
        .await;

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let db = void_core::db::Database::open_in_memory().unwrap();
    connector.backfill(&db).await.unwrap();
    db.set_sync_state("test-slack", "backfill_done", "1")
        .unwrap();

    assert_eq!(
        db.get_sync_state("test-slack", "backfill_done").unwrap(),
        Some("1".to_string())
    );
}

#[tokio::test]
async fn start_sync_skips_backfill_when_already_done() {
    let server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .expect(0)
        .named("users.list")
        .mount(&server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .expect(0)
        .named("conversations.list")
        .mount(&server)
        .await;

    let db = void_core::db::Database::open_in_memory().unwrap();
    db.set_sync_state("test-slack", "backfill_done", "1")
        .unwrap();

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    connector
        .start_sync(std::sync::Arc::new(db), cancel)
        .await
        .unwrap();
}

#[tokio::test]
async fn backfill_paginates_conversations() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({"ok": true, "members": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let page1 = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "ch1",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ],
        "response_metadata": {"next_cursor": "cursor2"}
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .and(wiremock::matchers::query_param("cursor", "cursor2"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "channels": [
                    {
                        "id": "C2",
                        "name": "ch2",
                        "is_channel": true,
                        "is_group": false,
                        "is_im": false,
                        "is_mpim": false,
                        "is_private": false
                    }
                ]
            })),
        )
        .with_priority(1)
        .mount(&server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(page1))
        .with_priority(2)
        .mount(&server)
        .await;

    let history_empty = serde_json::json!({"ok": true, "messages": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history_empty))
        .mount(&server)
        .await;

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let db = void_core::db::Database::open_in_memory().unwrap();
    connector.backfill(&db).await.unwrap();

    assert!(db.get_conversation("test-slack-C1").unwrap().is_some());
    assert!(db.get_conversation("test-slack-C2").unwrap().is_some());
}

#[tokio::test]
async fn backfill_syncs_all_channels() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({"ok": true, "members": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let channels = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "general",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            },
            {
                "id": "C2",
                "name": "random",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
        .mount(&server)
        .await;

    let history = serde_json::json!({"ok": true, "messages": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
        .mount(&server)
        .await;

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let db = void_core::db::Database::open_in_memory().unwrap();
    connector.backfill(&db).await.unwrap();

    assert!(db.get_conversation("test-slack-C1").unwrap().is_some());
    assert!(db.get_conversation("test-slack-C2").unwrap().is_some());
}

#[tokio::test]
async fn upload_file_calls_three_step_flow() {
    let server = wiremock::MockServer::start().await;

    let file_content = b"hello world";
    let upload_path = format!("/upload-{}", std::process::id());
    let upload_url = format!("{}{}", server.uri(), upload_path);

    let get_upload_url_resp = serde_json::json!({
        "ok": true,
        "upload_url": upload_url,
        "file_id": "F12345"
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path_regex(
            r"^/files\.getUploadURLExternal",
        ))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(get_upload_url_resp))
        .mount(&server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path(upload_path))
        .respond_with(wiremock::ResponseTemplate::new(200))
        .mount(&server)
        .await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/files.completeUploadExternal"))
        .and(wiremock::matchers::body_json(serde_json::json!({
            "files": [{"id": "F12345", "title": "test.txt"}],
            "channel_id": "C1",
            "initial_comment": "my caption",
            "thread_ts": "123.456"
        })))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
        )
        .mount(&server)
        .await;

    let temp_dir = std::env::temp_dir().join(format!("void-slack-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("test.txt");
    std::fs::write(&file_path, file_content).unwrap();

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let file_id = connector
        .upload_file(
            "C1",
            file_path.to_str().unwrap(),
            Some("my caption"),
            Some("123.456"),
        )
        .await
        .unwrap();

    assert_eq!(file_id, "F12345");
}

#[tokio::test]
async fn catch_up_fetches_messages_since_latest() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({
        "ok": true,
        "members": [
            {
                "id": "U1",
                "name": "alice",
                "real_name": "Alice",
                "profile": {"display_name": "Alice", "real_name": "Alice"}
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let channels = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "general",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
        .mount(&server)
        .await;

    let history = serde_json::json!({
        "ok": true,
        "messages": [
            {
                "ts": "1741800000.000200",
                "user": "U1",
                "text": "Caught up message"
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .and(wiremock::matchers::query_param("channel", "C1"))
        .and(wiremock::matchers::query_param("oldest", "1741700000"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
        .mount(&server)
        .await;

    let db = void_core::db::Database::open_in_memory().unwrap();

    let existing_conv = Conversation {
        id: "test-slack-C1".into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: "C1".into(),
        name: Some("general".into()),
        kind: ConversationKind::Channel,
        last_message_at: Some(1_741_700_000),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&existing_conv).unwrap();

    let existing_msg = Message {
        id: "test-slack-1741700000.000100".into(),
        conversation_id: "test-slack-C1".into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: "1741700000.000100".into(),
        sender: "U1".into(),
        sender_name: Some("Alice".into()),
        body: Some("Old message".into()),
        timestamp: 1_741_700_000,
        synced_at: None,
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    db.upsert_message(&existing_msg).unwrap();

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    connector.catch_up(&db).await.unwrap();

    let new_msg = db
        .get_message("test-slack-1741800000.000200")
        .unwrap()
        .unwrap();
    assert_eq!(new_msg.body.as_deref(), Some("Caught up message"));
    assert_eq!(new_msg.sender_name.as_deref(), Some("Alice"));
}

#[tokio::test]
async fn start_sync_runs_catch_up_when_backfill_done() {
    let server = wiremock::MockServer::start().await;

    let users = serde_json::json!({"ok": true, "members": []});
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/users.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(users))
        .mount(&server)
        .await;

    let channels = serde_json::json!({
        "ok": true,
        "channels": [
            {
                "id": "C1",
                "name": "general",
                "is_channel": true,
                "is_group": false,
                "is_im": false,
                "is_mpim": false,
                "is_private": false
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.list"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(channels))
        .mount(&server)
        .await;

    let history = serde_json::json!({
        "ok": true,
        "messages": [
            {
                "ts": "1741800000.000200",
                "user": "U1",
                "text": "New message after restart"
            }
        ]
    });
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/conversations.history"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(history))
        .mount(&server)
        .await;

    let db = std::sync::Arc::new(void_core::db::Database::open_in_memory().unwrap());
    db.set_sync_state("test-slack", "backfill_done", "1")
        .unwrap();

    let existing_conv = Conversation {
        id: "test-slack-C1".into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: "C1".into(),
        name: Some("general".into()),
        kind: ConversationKind::Channel,
        last_message_at: Some(1_741_700_000),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&existing_conv).unwrap();

    let existing_msg = Message {
        id: "test-slack-1741700000.000100".into(),
        conversation_id: "test-slack-C1".into(),
        connection_id: "test-slack".into(),
        connector: "slack".into(),
        external_id: "1741700000.000100".into(),
        sender: "U1".into(),
        sender_name: Some("Alice".into()),
        body: Some("Old message".into()),
        timestamp: 1_741_700_000,
        synced_at: None,
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    db.upsert_message(&existing_msg).unwrap();

    let connector = SlackConnector {
        connection_id: "test-slack".to_string(),
        api: crate::api::SlackApiClient::with_base_url("test-token", &server.uri()).unwrap(),
        app_token: "xapp-test".to_string(),
        app_id: None,
        store_path: std::env::temp_dir(),
    };

    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    connector.start_sync(db.clone(), cancel).await.unwrap();

    let new_msg = db
        .get_message("test-slack-1741800000.000200")
        .unwrap()
        .unwrap();
    assert_eq!(new_msg.body.as_deref(), Some("New message after restart"));
}
