//! Tests for the iMessage sending and confirmation flow.

use std::path::PathBuf;
use void_core::models::MessageContent;

use super::send::{build_applescript, escape_applescript_string, find_sent_message};

fn make_test_db(path: &std::path::Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE message (
            ROWID INTEGER PRIMARY KEY,
            guid TEXT,
            text TEXT,
            attributedBody BLOB,
            handle_id INTEGER,
            service TEXT,
            is_from_me INTEGER,
            date INTEGER,
            date_delivered INTEGER,
            date_read INTEGER
         );
         CREATE TABLE handle (ROWID INTEGER PRIMARY KEY, id TEXT);
         CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT, chat_identifier TEXT);
         CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);",
    )
    .unwrap();
    conn
}

#[test]
fn escape_applescript_string_handles_quotes_and_backslashes() {
    let input = r#"Hello "world" \ test"#;
    let escaped = escape_applescript_string(input);
    assert_eq!(escaped, r#"Hello \"world\" \\ test"#);
}

#[test]
fn build_applescript_text_message() {
    let content = MessageContent::Text {
        body: "Hello from void!".to_string(),
        subject: None,
        append_signature: false,
        signature_from: None,
        cc: None,
        bcc: None,
    };
    let script = build_applescript("+14155551212", &content).unwrap();
    assert!(script.contains(r#"buddy "+14155551212""#));
    assert!(script.contains(r#"send "Hello from void!" to targetBuddy"#));
}

#[test]
fn build_applescript_file_message_without_caption() {
    let content = MessageContent::File {
        path: PathBuf::from("/tmp/photo.jpg"),
        caption: None,
        mime_type: None,
        subject: None,
        append_signature: false,
        signature_from: None,
        cc: None,
        bcc: None,
    };
    let script = build_applescript("+14155551212", &content).unwrap();
    assert!(script.contains(r#"send (POSIX file "/tmp/photo.jpg") to targetBuddy"#));
    assert!(!script.contains("delay 0.5"));
}

#[test]
fn build_applescript_file_message_with_caption() {
    let content = MessageContent::File {
        path: PathBuf::from("/tmp/photo.jpg"),
        caption: Some("Look at this!".to_string()),
        mime_type: None,
        subject: None,
        append_signature: false,
        signature_from: None,
        cc: None,
        bcc: None,
    };
    let script = build_applescript("+14155551212", &content).unwrap();
    assert!(script.contains(r#"send (POSIX file "/tmp/photo.jpg") to targetBuddy"#));
    assert!(script.contains("delay 0.5"));
    assert!(script.contains(r#"send "Look at this!" to targetBuddy"#));
}

#[test]
fn find_sent_message_finds_matching_outgoing_row() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    let conn = make_test_db(&db_path);

    conn.execute(
        "INSERT INTO handle (ROWID, id) VALUES (1, '+14155551212')",
        [],
    )
    .unwrap();

    let target_apple_time = 780_000_000_000_000_000i64; // arbitrary nanoseconds since 2001

    // Incoming message from other person -> should NOT match
    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (1, 'guid-inbound', 'hey', 1, 0, ?1)",
        [target_apple_time + 100],
    )
    .unwrap();

    // Outgoing message before threshold -> should NOT match
    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (2, 'guid-old', 'old sent', 1, 1, ?1)",
        [target_apple_time - 1_000],
    )
    .unwrap();

    // Outgoing message after threshold -> should MATCH
    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (3, 'guid-confirmed-123', 'new sent', 1, 1, ?1)",
        [target_apple_time + 10_000],
    )
    .unwrap();

    let found = find_sent_message(&conn, "+14155551212", target_apple_time).unwrap();
    assert_eq!(found.as_deref(), Some("guid-confirmed-123"));
}

#[test]
fn find_sent_message_returns_none_if_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    let conn = make_test_db(&db_path);

    let found = find_sent_message(&conn, "+14155551212", 1_000_000).unwrap();
    assert_eq!(found, None);
}
