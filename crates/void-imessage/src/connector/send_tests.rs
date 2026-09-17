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

/// Newlines, tabs and carriage returns pass through untouched.
///
/// AppleScript accepts a raw newline inside a string literal, so rewriting
/// these to backslash sequences would change the text the recipient receives
/// instead of protecting anything. Verified against the real `osascript`:
/// `osascript -e 'return "a<newline>b"'` prints the two lines and exits 0.
#[test]
fn escape_applescript_string_passes_whitespace_through_unchanged() {
    assert_eq!(escape_applescript_string("a\nb"), "a\nb");
    assert_eq!(escape_applescript_string("a\tb"), "a\tb");
    assert_eq!(escape_applescript_string("a\rb"), "a\rb");
}

/// Non-ASCII text is left alone: `osascript` reads the script as UTF-8.
#[test]
fn escape_applescript_string_leaves_unicode_intact() {
    let input = "café 你好 😀 — ok";
    assert_eq!(escape_applescript_string(input), input);
}

/// The injection payload stays inert data.
///
/// This is the property the whole escaping function exists for: the closing
/// quote is neutralized, so the `&` concatenation and the `do shell script`
/// call cannot escape the string literal and become code. Confirmed against
/// the real interpreter, which echoes the payload verbatim rather than
/// executing it.
#[test]
fn escape_applescript_string_neutralizes_an_injection_attempt() {
    let hostile = r#"x" & (do shell script "echo pwned") & "y"#;
    let escaped = escape_applescript_string(hostile);

    // Every quote in the payload is escaped, so none of them can close the
    // literal that this value is interpolated into.
    assert_eq!(escaped, r#"x\" & (do shell script \"echo pwned\") & \"y"#);

    // And in the generated script, the payload is still inside one literal.
    let content = MessageContent::Text {
        body: hostile.to_string(),
        subject: None,
        append_signature: false,
        signature_from: None,
        cc: None,
        bcc: None,
    };
    let script = build_applescript("+14155551212", &content).unwrap();
    assert!(
        script.contains(r#"send "x\" & (do shell script \"echo pwned\") & \"y" to targetBuddy"#)
    );
    // No unescaped `do shell script` sits at statement level.
    assert!(!script.contains(r#"& (do shell script "echo pwned")"#));
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

    let found = find_sent_message(&conn, "+141****1212", 1_000_000).unwrap();
    assert_eq!(found, None);
}

/// A recipient containing a SQL `LIKE` wildcard must not confirm a message that
/// went to somebody else.
///
/// `_` matches any single character in a `LIKE` pattern, so the unescaped
/// suffix clause `h.id LIKE '%' || ?2` turned the address `a_b@example.com`
/// into a pattern matching `axb@example.com`. The consequence is the worst kind
/// for a send-confirmation loop: a message that never arrived is reported sent,
/// carrying the GUID of a different conversation. Underscores are legal and
/// common in real email addresses, so this is reachable without any hostile
/// intent.
#[test]
fn find_sent_message_does_not_treat_underscore_in_recipient_as_a_wildcard() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    let conn = make_test_db(&db_path);

    // The only handle in the store is a DIFFERENT address that the naive
    // pattern `%a_b@example.com` matches because `_` is a wildcard.
    conn.execute(
        "INSERT INTO handle (ROWID, id) VALUES (1, 'axb@example.com')",
        [],
    )
    .unwrap();

    let target_apple_time = 780_000_000_000_000_000i64;

    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (1, 'guid-someone-else', 'not for you', 1, 1, ?1)",
        [target_apple_time + 10_000],
    )
    .unwrap();

    let found = find_sent_message(&conn, "a_b@example.com", target_apple_time).unwrap();
    assert_eq!(
        found, None,
        "an underscore in the recipient must not match a different handle"
    );
}

/// The percent sign gets the same treatment, and for the same reason.
#[test]
fn find_sent_message_does_not_treat_percent_in_recipient_as_a_wildcard() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    let conn = make_test_db(&db_path);

    conn.execute(
        "INSERT INTO handle (ROWID, id) VALUES (1, 'anything@example.com')",
        [],
    )
    .unwrap();

    let target_apple_time = 780_000_000_000_000_000i64;

    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (1, 'guid-someone-else', 'not for you', 1, 1, ?1)",
        [target_apple_time + 10_000],
    )
    .unwrap();

    let found = find_sent_message(&conn, "%@example.com", target_apple_time).unwrap();
    assert_eq!(
        found, None,
        "a percent sign in the recipient must not match a different handle"
    );
}

/// Suffix matching must still work: `chat.db` stores handles in a normalized
/// form that does not always carry the country code the caller used.
#[test]
fn find_sent_message_still_matches_on_phone_number_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("chat.db");
    let conn = make_test_db(&db_path);

    // Stored without the leading "+1" that the caller passes.
    conn.execute(
        "INSERT INTO handle (ROWID, id) VALUES (1, '4155551212')",
        [],
    )
    .unwrap();

    let target_apple_time = 780_000_000_000_000_000i64;

    conn.execute(
        "INSERT INTO message (ROWID, guid, text, handle_id, is_from_me, date)
         VALUES (1, 'guid-suffix-match', 'sent', 1, 1, ?1)",
        [target_apple_time + 10_000],
    )
    .unwrap();

    let found = find_sent_message(&conn, "+14155551212", target_apple_time).unwrap();
    assert_eq!(found.as_deref(), Some("guid-suffix-match"));
}
