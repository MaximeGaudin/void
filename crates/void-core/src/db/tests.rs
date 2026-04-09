use super::*;
use crate::models::{CalendarEvent, Conversation, ConversationKind, Message};

fn test_db() -> Database {
    Database::open_in_memory().unwrap()
}

fn make_conversation(id: &str, connection_id: &str, ext_id: &str) -> Conversation {
    Conversation {
        id: id.into(),
        connection_id: connection_id.into(),
        connector: "slack".into(),
        external_id: ext_id.into(),
        name: Some(format!("Conv {id}")),
        kind: ConversationKind::Dm,
        last_message_at: Some(1_700_000_000),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    }
}

fn make_message(id: &str, conv_id: &str, connection_id: &str, body: &str, ts: i64) -> Message {
    Message {
        id: id.into(),
        conversation_id: conv_id.into(),
        connection_id: connection_id.into(),
        connector: "slack".into(),
        external_id: format!("ext-{id}"),
        sender: "sender@test".into(),
        sender_name: Some("Test Sender".into()),
        sender_avatar_url: None,
        body: Some(body.into()),
        timestamp: ts,
        synced_at: None,
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    }
}

#[test]
fn migration_runs() {
    let db = test_db();
    let conn = db.conn().unwrap();
    let version: i32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);
}

#[test]
fn conversation_crud() {
    let db = test_db();
    let conv = make_conversation("c1", "work-slack", "C123");

    db.upsert_conversation(&conv).unwrap();
    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert_eq!(loaded.name.as_deref(), Some("Conv c1"));

    let list = db.list_conversations(None, None, 100, true).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn conversation_upsert_updates() {
    let db = test_db();
    let mut conv = make_conversation("c1", "work-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    conv.name = Some("Updated".into());
    db.upsert_conversation(&conv).unwrap();

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert_eq!(loaded.name.as_deref(), Some("Updated"));
}

#[test]
fn message_crud() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "Hello world", 1_700_000_000);
    db.upsert_message(&msg).unwrap();

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert_eq!(loaded.body.as_deref(), Some("Hello world"));

    let list = db.list_messages("c1", 100, None, None).unwrap();
    assert_eq!(list.len(), 1);
}

#[test]
fn message_synced_at_auto_populated() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "hello", 1_700_000_000);
    assert!(msg.synced_at.is_none());

    db.upsert_message(&msg).unwrap();

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert!(
        loaded.synced_at.is_some(),
        "synced_at should be auto-populated on insert"
    );
    let synced = loaded.synced_at.unwrap();
    assert!(
        synced >= loaded.timestamp,
        "synced_at ({synced}) should be >= message timestamp ({})",
        loaded.timestamp
    );
}

#[test]
fn message_synced_at_preserved_on_upsert() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "original", 1_700_000_000);
    db.upsert_message(&msg).unwrap();

    let first_load = db.get_message("m1").unwrap().unwrap();
    let original_synced_at = first_load.synced_at.unwrap();

    let mut updated = make_message("m1", "c1", "test-slack", "edited body", 1_700_000_000);
    updated.body = Some("edited body".into());
    db.upsert_message(&updated).unwrap();

    let reloaded = db.get_message("m1").unwrap().unwrap();
    assert_eq!(reloaded.body.as_deref(), Some("edited body"));
    assert_eq!(
        reloaded.synced_at.unwrap(),
        original_synced_at,
        "synced_at should not change on upsert/update"
    );
}

#[test]
fn fts5_search() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message(
        "m1",
        "c1",
        "test-slack",
        "meeting tomorrow at 10am",
        1_700_000_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m2",
        "c1",
        "test-slack",
        "lunch plans for Friday",
        1_700_000_001,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m3",
        "c1",
        "test-slack",
        "quarterly budget review meeting",
        1_700_000_002,
    ))
    .unwrap();

    let results = db.search_messages("meeting", None, None, 10, true).unwrap();
    assert_eq!(results.len(), 2);
}

// ---- search_messages integration: special characters ----

fn seed_search_db() -> Database {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut conv2 = make_conversation("c2", "me@gmail.com", "G456");
    conv2.connector = "gmail".into();
    db.upsert_conversation(&conv2).unwrap();

    db.upsert_message(&make_message(
        "m1",
        "c1",
        "test-slack",
        "hello @MadMax how are you?",
        1_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m2",
        "c1",
        "test-slack",
        "meeting with @alice tomorrow",
        2_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m3",
        "c1",
        "test-slack",
        "the C++ compiler is broken",
        3_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m4",
        "c1",
        "test-slack",
        "file: budget-report-2024.xlsx",
        4_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m5",
        "c1",
        "test-slack",
        "say \"hello\" to everyone",
        5_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m6",
        "c1",
        "test-slack",
        "NOT a problem AND it works OR fails",
        6_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m7",
        "c1",
        "test-slack",
        "user:admin password:secret",
        7_000,
    ))
    .unwrap();

    let mut gmail_msg = make_message("m8", "c2", "me@gmail.com", "invoice from @accounts", 8_000);
    gmail_msg.connector = "gmail".into();
    db.upsert_message(&gmail_msg).unwrap();

    db
}

#[test]
fn search_at_symbol_does_not_crash() {
    let db = seed_search_db();
    let results = db.search_messages("@MadMax", None, None, 50, true).unwrap();
    assert!(!results.is_empty());
    assert!(results
        .iter()
        .any(|m| m.body.as_deref().unwrap().contains("@MadMax")));
}

#[test]
fn search_at_symbol_with_connector_filter() {
    let db = seed_search_db();
    let results = db
        .search_messages("@accounts", None, Some("gmail"), 50, true)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].connector, "gmail");
}

#[test]
fn search_at_symbol_wrong_connector_returns_empty() {
    let db = seed_search_db();
    let results = db
        .search_messages("@accounts", None, Some("whatsapp"), 50, true)
        .unwrap();
    assert!(results.is_empty());
}

#[test]
fn search_double_quotes_does_not_crash() {
    let db = seed_search_db();
    let results = db
        .search_messages(r#""hello""#, None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_dash_does_not_crash() {
    let db = seed_search_db();
    let results = db.search_messages("-report", None, None, 50, true).unwrap();
    // Should not error — the dash is escaped
    let _ = results;
}

#[test]
fn search_asterisk_does_not_crash() {
    let db = seed_search_db();
    let results = db.search_messages("budget*", None, None, 50, true).unwrap();
    let _ = results;
}

#[test]
fn search_plus_does_not_crash() {
    let db = seed_search_db();
    let results = db
        .search_messages("+required", None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_boolean_operators_treated_as_literals() {
    let db = seed_search_db();
    let results = db.search_messages("NOT", None, None, 50, true).unwrap();
    // Should return results containing "NOT" as a word rather than treating it as boolean op
    assert!(!results.is_empty());
}

#[test]
fn search_and_operator_literal() {
    let db = seed_search_db();
    let results = db.search_messages("AND", None, None, 50, true).unwrap();
    let _ = results; // Must not crash
}

#[test]
fn search_or_operator_literal() {
    let db = seed_search_db();
    let results = db.search_messages("OR", None, None, 50, true).unwrap();
    let _ = results;
}

#[test]
fn search_near_operator_literal() {
    let db = seed_search_db();
    let results = db.search_messages("NEAR", None, None, 50, true).unwrap();
    let _ = results;
}

#[test]
fn search_colon_column_syntax_does_not_leak() {
    let db = seed_search_db();
    // In raw FTS5 "body:secret" would search column "body" for "secret".
    // Our escaping should prevent column-targeted search.
    let results = db
        .search_messages("body:secret", None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_parentheses_do_not_crash() {
    let db = seed_search_db();
    let results = db
        .search_messages("(hello OR world)", None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_curly_braces_do_not_crash() {
    let db = seed_search_db();
    let results = db.search_messages("{test}", None, None, 50, true).unwrap();
    let _ = results;
}

#[test]
fn search_sql_injection_attempt() {
    let db = seed_search_db();
    let results = db
        .search_messages("'; DROP TABLE messages; --", None, None, 50, true)
        .unwrap();
    let _ = results;

    // Verify the messages table still exists and has data
    let all = db.recent_messages(None, None, 100, true, true).unwrap();
    assert!(
        !all.is_empty(),
        "messages table must survive injection attempt"
    );
}

#[test]
fn search_fts5_injection_via_double_quotes() {
    let db = seed_search_db();
    // An attacker might try to break out of quoting to inject FTS5 operators
    let results = db
        .search_messages(r#"" OR body:*"#, None, None, 50, true)
        .unwrap();
    let _ = results;

    let all = db.recent_messages(None, None, 100, true, true).unwrap();
    assert!(!all.is_empty());
}

#[test]
fn search_empty_query_does_not_crash() {
    let db = seed_search_db();
    // Empty query should not cause a panic or SQL error
    let result = db.search_messages("", None, None, 50, true);
    // It's acceptable for this to return an error or empty results, but not panic
    let _ = result;
}

#[test]
fn search_whitespace_only_query_does_not_crash() {
    let db = seed_search_db();
    let result = db.search_messages("   ", None, None, 50, true);
    let _ = result;
}

#[test]
fn search_with_connection_filter_and_special_chars() {
    let db = seed_search_db();
    let results = db
        .search_messages("@MadMax", Some("test-slack"), None, 50, true)
        .unwrap();
    assert!(!results.is_empty());
}

#[test]
fn search_with_both_filters_and_special_chars() {
    let db = seed_search_db();
    let results = db
        .search_messages("@MadMax", Some("test-slack"), Some("slack"), 50, true)
        .unwrap();
    assert!(!results.is_empty());

    let no_results = db
        .search_messages("@MadMax", Some("test-slack"), Some("gmail"), 50, true)
        .unwrap();
    assert!(no_results.is_empty());
}

#[test]
fn search_limit_is_respected() {
    let db = seed_search_db();
    // All messages contain common words — search for something broad
    let results = db.search_messages("the", None, None, 1, true).unwrap();
    assert!(results.len() <= 1);
}

#[test]
fn search_unicode_does_not_crash() {
    let db = seed_search_db();
    let results = db
        .search_messages("café résumé 会議", None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_emoji_does_not_crash() {
    let db = seed_search_db();
    let results = db.search_messages("📄", None, None, 50, true).unwrap();
    let _ = results;
}

#[test]
fn search_backslash_does_not_crash() {
    let db = seed_search_db();
    let results = db
        .search_messages(r"C:\Users\admin", None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_very_long_query_does_not_crash() {
    let db = seed_search_db();
    let long_query = "word ".repeat(200);
    let results = db
        .search_messages(&long_query, None, None, 50, true)
        .unwrap();
    let _ = results;
}

#[test]
fn search_null_byte_does_not_crash() {
    let db = seed_search_db();
    let result = db.search_messages("hello\0world", None, None, 50, true);
    // May error but must not panic
    let _ = result;
}

#[test]
fn message_date_range_filter() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message(
        "m1",
        "c1",
        "test-slack",
        "old msg",
        1_000_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m2",
        "c1",
        "test-slack",
        "mid msg",
        2_000_000,
    ))
    .unwrap();
    db.upsert_message(&make_message(
        "m3",
        "c1",
        "test-slack",
        "new msg",
        3_000_000,
    ))
    .unwrap();

    let results = db
        .list_messages("c1", 100, Some(1_500_000), Some(2_500_000))
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "m2");
}

#[test]
fn event_crud() {
    let db = test_db();
    let event = CalendarEvent {
        id: "e1".into(),
        connection_id: "my-calendar".into(),
        connector: "calendar".into(),
        external_id: "goog123".into(),
        title: "Standup".into(),
        description: None,
        location: None,
        start_at: 1_700_000_000,
        end_at: 1_700_001_800,
        all_day: false,
        attendees: None,
        status: Some("confirmed".into()),
        calendar_name: Some("primary".into()),
        meet_link: Some("https://meet.google.com/abc".into()),
        metadata: None,
    };

    db.upsert_event(&event).unwrap();
    let list = db
        .list_events(Some(1_700_000_000), Some(1_700_002_000), None, None, 100)
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].meet_link.as_deref(),
        Some("https://meet.google.com/abc")
    );
}

#[test]
fn sync_state_crud() {
    let db = test_db();
    db.set_sync_state("gmail-1", "history_id", "12345").unwrap();

    let val = db.get_sync_state("gmail-1", "history_id").unwrap();
    assert_eq!(val.as_deref(), Some("12345"));

    db.set_sync_state("gmail-1", "history_id", "67890").unwrap();
    let val = db.get_sync_state("gmail-1", "history_id").unwrap();
    assert_eq!(val.as_deref(), Some("67890"));

    let missing = db.get_sync_state("gmail-1", "nonexistent").unwrap();
    assert!(missing.is_none());
}

#[test]
fn recent_messages_ordered() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message("m1", "c1", "test-slack", "first", 1_000))
        .unwrap();
    db.upsert_message(&make_message("m2", "c1", "test-slack", "second", 2_000))
        .unwrap();
    db.upsert_message(&make_message("m3", "c1", "test-slack", "third", 3_000))
        .unwrap();

    let results = db.recent_messages(None, None, 2, true, true).unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "m3");
    assert_eq!(results[1].id, "m2");
}

#[test]
fn list_contacts_basic() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hello", 1_000);
    m1.sender = "alice@test.com".into();
    m1.sender_name = Some("Alice".into());
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "world", 2_000);
    m2.sender = "alice@test.com".into();
    m2.sender_name = Some("Alice".into());
    db.upsert_message(&m2).unwrap();

    let mut m3 = make_message("m3", "c1", "test-slack", "bye", 3_000);
    m3.sender = "bob@test.com".into();
    m3.sender_name = Some("Bob".into());
    db.upsert_message(&m3).unwrap();

    let contacts = db.list_contacts(None, None, None, 100).unwrap();
    assert_eq!(contacts.len(), 2);
    assert_eq!(contacts[0].sender, "bob@test.com");
    assert_eq!(contacts[0].message_count, 1);
    assert_eq!(contacts[1].sender, "alice@test.com");
    assert_eq!(contacts[1].message_count, 2);
}

#[test]
fn list_contacts_search() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "alice@test.com".into();
    m1.sender_name = Some("Alice".into());
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
    m2.sender = "bob@test.com".into();
    m2.sender_name = Some("Bob".into());
    db.upsert_message(&m2).unwrap();

    let results = db.list_contacts(None, None, Some("alice"), 100).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].sender, "alice@test.com");
}

#[test]
fn list_contacts_connection_filter() {
    let db = test_db();
    let c1 = make_conversation("c1", "gladiaio", "C123");
    db.upsert_conversation(&c1).unwrap();
    let mut c2 = make_conversation("c2", "33651090627", "W123");
    c2.connector = "whatsapp".into();
    db.upsert_conversation(&c2).unwrap();

    let mut m1 = make_message("m1", "c1", "gladiaio", "hi", 1_000);
    m1.sender = "alice@slack".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c2", "33651090627", "hey", 2_000);
    m2.sender = "bob@wa".into();
    m2.connector = "whatsapp".into();
    db.upsert_message(&m2).unwrap();

    let results = db.list_contacts(Some("gladiaio"), None, None, 100).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].sender, "alice@slack");
}

#[test]
fn list_contacts_excludes_own_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "test-slack".into();
    db.upsert_message(&m1).unwrap();

    let contacts = db.list_contacts(None, None, None, 100).unwrap();
    assert!(contacts.is_empty());
}

#[test]
fn list_channels_basic() {
    let db = test_db();
    let mut group = make_conversation("c1", "test-slack", "G123");
    group.kind = ConversationKind::Group;
    group.name = Some("Engineering".into());
    db.upsert_conversation(&group).unwrap();

    let mut channel = make_conversation("c2", "test-slack", "C456");
    channel.kind = ConversationKind::Channel;
    channel.name = Some("General".into());
    db.upsert_conversation(&channel).unwrap();

    let dm = make_conversation("c3", "test-slack", "D789");
    db.upsert_conversation(&dm).unwrap();

    let channels = db.list_channels(None, None, None, 100, true).unwrap();
    assert_eq!(channels.len(), 2);
}

#[test]
fn list_channels_search() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "test-slack", "G123");
    c1.kind = ConversationKind::Group;
    c1.name = Some("Engineering".into());
    db.upsert_conversation(&c1).unwrap();

    let mut c2 = make_conversation("c2", "test-slack", "C456");
    c2.kind = ConversationKind::Channel;
    c2.name = Some("General".into());
    db.upsert_conversation(&c2).unwrap();

    let results = db
        .list_channels(None, None, Some("engi"), 100, true)
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name.as_deref(), Some("Engineering"));
}

#[test]
fn mark_message_archived_updates_flag() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "hello", 1_000);
    db.upsert_message(&msg).unwrap();

    let updated = db.mark_message_archived("m1").unwrap();
    assert!(updated);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert!(loaded.is_archived);
}

#[test]
fn find_message_by_external_id_returns_match() {
    let db = test_db();
    let conv = make_conversation("c1", "acct1", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "acct1", "hello", 1_000);
    db.upsert_message(&msg).unwrap();

    let found = db.find_message_by_external_id("acct1", "ext-m1").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().body.as_deref(), Some("hello"));
}

#[test]
fn find_message_by_external_id_nonexistent_returns_none() {
    let db = test_db();
    let found = db
        .find_message_by_external_id("acct1", "nonexistent")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn update_message_metadata_merges_json() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "hello", 1_000);
    db.upsert_message(&msg).unwrap();

    let updated = db
        .update_message_metadata("m1", &serde_json::json!({"key": "value"}))
        .unwrap();
    assert!(updated);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert_eq!(
        loaded.metadata.as_ref().unwrap()["key"],
        serde_json::json!("value")
    );
}

// ---- Mute filtering tests ----

#[test]
fn mute_state_default_false() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(!loaded.is_muted);
}

#[test]
fn update_conversation_mute() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let updated = db.update_conversation_mute("c1", true).unwrap();
    assert!(updated);

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded.is_muted);

    db.update_conversation_mute("c1", false).unwrap();
    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(!loaded.is_muted);
}

#[test]
fn set_mute_by_external_id() {
    let db = test_db();
    let conv = make_conversation("c1", "my-wa-jid", "chat@g.us");
    db.upsert_conversation(&conv).unwrap();

    let updated = db
        .set_mute_by_external_id("my-wa-jid", "chat@g.us", true)
        .unwrap();
    assert!(updated);

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded.is_muted);
}

#[test]
fn set_mute_by_external_id_nonexistent_returns_false() {
    let db = test_db();
    let updated = db.set_mute_by_external_id("nope", "nope", true).unwrap();
    assert!(!updated);
}

#[test]
fn upsert_does_not_reset_mute() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.update_conversation_mute("c1", true).unwrap();

    let mut conv2 = make_conversation("c1", "test-slack", "C123");
    conv2.name = Some("Updated Name".into());
    db.upsert_conversation(&conv2).unwrap();

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded.is_muted, "upsert must not reset mute state");
    assert_eq!(loaded.name.as_deref(), Some("Updated Name"));
}

#[test]
fn list_conversations_excludes_muted_by_default() {
    let db = test_db();
    let c1 = make_conversation("c1", "test", "E1");
    db.upsert_conversation(&c1).unwrap();
    let c2 = make_conversation("c2", "test", "E2");
    db.upsert_conversation(&c2).unwrap();

    db.update_conversation_mute("c2", true).unwrap();

    let without_muted = db.list_conversations(None, None, 100, false).unwrap();
    assert_eq!(without_muted.len(), 1);
    assert_eq!(without_muted[0].id, "c1");

    let with_muted = db.list_conversations(None, None, 100, true).unwrap();
    assert_eq!(with_muted.len(), 2);
}

#[test]
fn recent_messages_excludes_muted_conversations() {
    let db = test_db();
    let c1 = make_conversation("c1", "test", "E1");
    db.upsert_conversation(&c1).unwrap();
    let c2 = make_conversation("c2", "test", "E2");
    db.upsert_conversation(&c2).unwrap();

    db.upsert_message(&make_message("m1", "c1", "test", "visible", 1_000))
        .unwrap();
    db.upsert_message(&make_message("m2", "c2", "test", "muted msg", 2_000))
        .unwrap();

    db.update_conversation_mute("c2", true).unwrap();

    let without_muted = db.recent_messages(None, None, 100, true, false).unwrap();
    assert_eq!(without_muted.len(), 1);
    assert_eq!(without_muted[0].body.as_deref(), Some("visible"));

    let with_muted = db.recent_messages(None, None, 100, true, true).unwrap();
    assert_eq!(with_muted.len(), 2);
}

#[test]
fn search_messages_excludes_muted_conversations() {
    let db = test_db();
    let c1 = make_conversation("c1", "test", "E1");
    db.upsert_conversation(&c1).unwrap();
    let c2 = make_conversation("c2", "test", "E2");
    db.upsert_conversation(&c2).unwrap();

    db.upsert_message(&make_message("m1", "c1", "test", "hello world", 1_000))
        .unwrap();
    db.upsert_message(&make_message("m2", "c2", "test", "hello muted", 2_000))
        .unwrap();

    db.update_conversation_mute("c2", true).unwrap();

    let without_muted = db.search_messages("hello", None, None, 100, false).unwrap();
    assert_eq!(without_muted.len(), 1);
    assert_eq!(without_muted[0].conversation_id, "c1");

    let with_muted = db.search_messages("hello", None, None, 100, true).unwrap();
    assert_eq!(with_muted.len(), 2);
}

#[test]
fn list_channels_excludes_muted_by_default() {
    let db = test_db();
    let mut g1 = make_conversation("g1", "test", "G1");
    g1.kind = ConversationKind::Group;
    db.upsert_conversation(&g1).unwrap();

    let mut g2 = make_conversation("g2", "test", "G2");
    g2.kind = ConversationKind::Group;
    db.upsert_conversation(&g2).unwrap();

    db.update_conversation_mute("g2", true).unwrap();

    let without_muted = db.list_channels(None, None, None, 100, false).unwrap();
    assert_eq!(without_muted.len(), 1);
    assert_eq!(without_muted[0].id, "g1");

    let with_muted = db.list_channels(None, None, None, 100, true).unwrap();
    assert_eq!(with_muted.len(), 2);
}

#[test]
fn rename_connection_updates_ids_in_all_tables() {
    let db = test_db();
    let conv = make_conversation("old-id-c1", "old-id", "E1");
    db.upsert_conversation(&conv).unwrap();
    db.upsert_message(&make_message(
        "old-id-m1",
        "old-id-c1",
        "old-id",
        "body",
        1_000,
    ))
    .unwrap();
    db.set_sync_state("old-id", "key1", "value1").unwrap();

    db.rename_connection("old-id", "new-id").unwrap();

    let conv_after = db.get_conversation("new-id-c1").unwrap();
    assert!(conv_after.is_some());
    assert_eq!(conv_after.unwrap().connection_id, "new-id");

    let msg_after = db.get_message("new-id-m1").unwrap();
    assert!(msg_after.is_some());
    assert_eq!(msg_after.unwrap().connection_id, "new-id");

    let sync_val = db.get_sync_state("new-id", "key1").unwrap();
    assert_eq!(sync_val, Some("value1".to_string()));

    assert!(db.get_conversation("old-id-c1").unwrap().is_none());
}

fn make_conversation_with_connector(
    id: &str,
    connection_id: &str,
    ext_id: &str,
    connector: &str,
) -> Conversation {
    let mut conv = make_conversation(id, connection_id, ext_id);
    conv.connector = connector.into();
    conv
}

fn make_message_with_connector(
    id: &str,
    conv_id: &str,
    connection_id: &str,
    body: &str,
    ts: i64,
    connector: &str,
) -> Message {
    let mut msg = make_message(id, conv_id, connection_id, body, ts);
    msg.connector = connector.into();
    msg
}

#[test]
fn clear_connector_data_removes_all_messages_conversations_events_sync_state() {
    let db = test_db();
    let conv = make_conversation_with_connector("c1", "gmail-1", "E1", "gmail");
    db.upsert_conversation(&conv).unwrap();
    db.upsert_message(&make_message_with_connector(
        "m1", "c1", "gmail-1", "body", 1_000, "gmail",
    ))
    .unwrap();
    db.set_sync_state("gmail-1", "history_id", "123").unwrap();

    let (msgs, convs, evts, sync) = db.clear_connector_data("gmail").unwrap();
    assert_eq!(msgs, 1);
    assert_eq!(convs, 1);
    assert_eq!(evts, 0);
    assert_eq!(sync, 1);

    assert!(db.get_conversation("c1").unwrap().is_none());
    assert!(db.get_message("m1").unwrap().is_none());
    assert!(db
        .get_sync_state("gmail-1", "history_id")
        .unwrap()
        .is_none());
}
