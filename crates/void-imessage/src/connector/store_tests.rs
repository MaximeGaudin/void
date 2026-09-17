//! Tests for the store reader.
//!
//! These build a synthetic `chat.db` with the real Messages schema rather than
//! touching anyone's actual database: the tests must run on CI, on Linux, and
//! without Full Disk Access.

use super::store::{apple_time_to_unix, fetch_since, open_read_only, resolve_body, StoreError};

const ASCII_BLOB: &[u8] = include_bytes!("fixtures/ts_ascii.bin");

/// Build a throwaway database with the subset of the Messages schema Void reads.
fn make_db(path: &std::path::Path) -> rusqlite::Connection {
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
         CREATE TABLE chat (ROWID INTEGER PRIMARY KEY, guid TEXT);
         CREATE TABLE chat_message_join (chat_id INTEGER, message_id INTEGER);",
    )
    .unwrap();
    conn
}

#[test]
fn prefers_text_column_when_it_has_content() {
    assert_eq!(
        resolve_body(Some("plain text"), Some(ASCII_BLOB)),
        Some("plain text".to_string())
    );
}

#[test]
fn falls_back_to_attributed_body_when_text_is_null() {
    // This is the whole reason the typedstream decoder exists: on recent macOS
    // the text column is NULL and the body lives only in attributedBody.
    assert_eq!(
        resolve_body(None, Some(ASCII_BLOB)),
        Some("Hello from a synthetic fixture".to_string())
    );
}

#[test]
fn falls_back_to_attributed_body_when_text_is_empty_string() {
    assert_eq!(
        resolve_body(Some(""), Some(ASCII_BLOB)),
        Some("Hello from a synthetic fixture".to_string())
    );
}

#[test]
fn returns_none_when_both_sources_are_absent() {
    assert_eq!(resolve_body(None, None), None);
}

#[test]
fn undecodable_attributed_body_does_not_lose_the_row() {
    // A blob we cannot parse must not take the message down with it.
    assert_eq!(resolve_body(None, Some(b"garbage")), None);
    assert_eq!(
        resolve_body(Some("fallback"), Some(b"garbage")),
        Some("fallback".to_string())
    );
}

#[test]
fn converts_nanosecond_apple_timestamps() {
    // 2026-09-14T12:00:00Z expressed as nanoseconds since 2001.
    let raw = (1_789_387_200_i64 - 978_307_200) * 1_000_000_000;
    assert_eq!(apple_time_to_unix(raw), 1_789_387_200);
}

#[test]
fn converts_legacy_second_apple_timestamps() {
    // Older rows store seconds since 2001 in the same column.
    assert_eq!(
        apple_time_to_unix(1_789_387_200 - 978_307_200),
        1_789_387_200
    );
}

#[test]
fn treats_zero_timestamp_as_zero_not_as_2001() {
    // date_delivered is 0 for a message that was never delivered. Mapping it to
    // 2001-01-01 would make undelivered messages look delivered 25 years ago.
    assert_eq!(apple_time_to_unix(0), 0);
}

#[test]
fn reads_rows_after_the_cursor_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    let conn = make_db(&path);
    conn.execute(
        "INSERT INTO handle (ROWID, id) VALUES (1, '+15551234567')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO chat (ROWID, guid) VALUES (1, 'iMessage;-;+15551234567')",
        [],
    )
    .unwrap();
    for rowid in 1..=5 {
        conn.execute(
            "INSERT INTO message
             (ROWID, guid, text, attributedBody, handle_id, service, is_from_me,
              date, date_delivered, date_read)
             VALUES (?1, ?2, ?3, NULL, 1, 'iMessage', 0, 0, 0, 0)",
            rusqlite::params![rowid, format!("guid-{rowid}"), format!("message {rowid}")],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chat_message_join (chat_id, message_id) VALUES (1, ?1)",
            rusqlite::params![rowid],
        )
        .unwrap();
    }
    drop(conn);

    let read = open_read_only(&path).unwrap();
    let rows = fetch_since(&read, 2, 10).unwrap();

    assert_eq!(rows.len(), 3, "cursor must exclude rows 1 and 2");
    assert_eq!(rows[0].rowid, 3);
    assert_eq!(rows[0].body.as_deref(), Some("message 3"));
    assert_eq!(rows[0].handle.as_deref(), Some("+15551234567"));
    assert_eq!(
        rows[0].chat_guid.as_deref(),
        Some("iMessage;-;+15551234567")
    );
    assert_eq!(rows[2].rowid, 5, "rows must come back oldest first");
}

#[test]
fn decodes_attributed_body_through_the_query() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    let conn = make_db(&path);
    conn.execute(
        "INSERT INTO message
         (ROWID, guid, text, attributedBody, handle_id, service, is_from_me,
          date, date_delivered, date_read)
         VALUES (1, 'g1', NULL, ?1, NULL, 'iMessage', 1, 0, 0, 0)",
        rusqlite::params![ASCII_BLOB],
    )
    .unwrap();
    drop(conn);

    let read = open_read_only(&path).unwrap();
    let rows = fetch_since(&read, 0, 10).unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].body.as_deref(),
        Some("Hello from a synthetic fixture"),
        "a NULL text column must not produce an empty message"
    );
    assert!(rows[0].is_from_me);
}

#[test]
fn respects_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.db");
    let conn = make_db(&path);
    for rowid in 1..=10 {
        conn.execute(
            "INSERT INTO message
             (ROWID, guid, text, attributedBody, handle_id, service, is_from_me,
              date, date_delivered, date_read)
             VALUES (?1, ?2, 'x', NULL, NULL, 'SMS', 0, 0, 0, 0)",
            rusqlite::params![rowid, format!("g{rowid}")],
        )
        .unwrap();
    }
    drop(conn);

    let read = open_read_only(&path).unwrap();
    assert_eq!(fetch_since(&read, 0, 4).unwrap().len(), 4);
}

#[test]
fn missing_database_is_reported_as_missing_not_as_denied() {
    // The two cases need different advice: one means "set up Messages", the
    // other means "grant Full Disk Access". Conflating them sends the user to
    // the wrong place.
    let dir = tempfile::tempdir().unwrap();
    let err = open_read_only(&dir.path().join("nope.db")).unwrap_err();
    assert!(
        matches!(err, StoreError::Missing(_)),
        "expected Missing, got: {err}"
    );
}

#[test]
fn access_denied_error_names_full_disk_access() {
    // The message is the entire value of this error path: a user who hits it
    // must know which switch to flip.
    let err = StoreError::AccessDenied(std::path::PathBuf::from("/tmp/chat.db"));
    let text = err.to_string();
    assert!(text.contains("Full Disk Access"), "got: {text}");
    assert!(text.contains("System Settings"), "got: {text}");
}
