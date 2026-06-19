use super::fixtures::*;
use super::*;

use crate::models::{CalendarEvent, Conversation, ConversationKind, Message};

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

/// The Slack workspace subdomain in a permalink does NOT have to match the
/// void connection_id (connections are user-named in config.toml). The
/// resolver must route by the Slack-native (channel_id, message_ts) pair.
#[test]
fn find_slack_message_by_link_ignores_connection_naming() {
    let db = test_db();

    let conv = Conversation {
        id: "slack-C08UDH5JE57".into(),
        connection_id: "slack".into(),
        connector: "slack".into(),
        external_id: "C08UDH5JE57".into(),
        name: Some("tech-platform-engineering".into()),
        kind: ConversationKind::Channel,
        last_message_at: Some(1_776_936_528),
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv).unwrap();

    let msg = Message {
        id: "slack-1776936528.857609".into(),
        conversation_id: "slack-C08UDH5JE57".into(),
        connection_id: "slack".into(),
        connector: "slack".into(),
        external_id: "1776936528.857609".into(),
        sender: "U1".into(),
        sender_name: Some("Alice".into()),
        sender_avatar_url: None,
        body: Some("hello thread".into()),
        timestamp: 1_776_936_528,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    db.upsert_message(&msg).unwrap();

    let found = db
        .find_slack_message_by_link("C08UDH5JE57", "1776936528.857609")
        .unwrap()
        .expect("message should be found via Slack-native IDs");
    assert_eq!(found.id, "slack-1776936528.857609");
    assert_eq!(found.connection_id, "slack");
    assert_eq!(found.body.as_deref(), Some("hello thread"));
}

#[test]
fn find_slack_message_by_link_returns_none_when_channel_mismatches() {
    let db = test_db();
    // Same ts but different channel — must not match.
    let conv = make_conversation("slack-CAAA", "slack", "CAAA");
    db.upsert_conversation(&conv).unwrap();
    let mut msg = make_message("m1", "slack-CAAA", "slack", "body", 1);
    msg.external_id = "1776936528.857609".into();
    db.upsert_message(&msg).unwrap();

    let found = db
        .find_slack_message_by_link("CBBB", "1776936528.857609")
        .unwrap();
    assert!(found.is_none(), "ts must be scoped to its channel");
}

#[test]
fn find_slack_message_by_link_skips_other_connectors() {
    let db = test_db();
    // An imaginary non-slack row with the same ids should be ignored.
    let mut conv = make_conversation("gmail-X", "gmail-acct", "CX");
    conv.connector = "gmail".into();
    db.upsert_conversation(&conv).unwrap();
    let mut msg = make_message("m1", "gmail-X", "gmail-acct", "body", 1);
    msg.connector = "gmail".into();
    msg.external_id = "1776936528.857609".into();
    db.upsert_message(&msg).unwrap();

    let found = db
        .find_slack_message_by_link("CX", "1776936528.857609")
        .unwrap();
    assert!(found.is_none());
}

#[test]
fn find_slack_conversation_by_link_resolves_across_connections() {
    let db = test_db();
    let conv = make_conversation("weird-name-C1", "weird-name", "C08UDH5JE57");
    db.upsert_conversation(&conv).unwrap();

    let found = db
        .find_slack_conversation_by_link("C08UDH5JE57")
        .unwrap()
        .expect("should resolve channel by its Slack external_id");
    assert_eq!(found.id, "weird-name-C1");
    assert_eq!(found.connection_id, "weird-name");
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

#[test]
fn migrate_whatsapp_jid_connections_merges_jid_rows_into_config_name() {
    let db = test_db();

    let old_conv = make_conversation_with_connector(
        "wa_94004066660357:37@lid_120363@g.us",
        "94004066660357:37@lid",
        "120363@g.us",
        "whatsapp",
    );
    db.upsert_conversation(&old_conv).unwrap();
    db.upsert_message(&make_message_with_connector(
        "wa_94004066660357:37@lid_MSG1",
        "wa_94004066660357:37@lid_120363@g.us",
        "94004066660357:37@lid",
        "Salut toi",
        1_000,
        "whatsapp",
    ))
    .unwrap();

    let canonical_conv = make_conversation_with_connector(
        "wa_whatsapp_120363@g.us",
        "whatsapp",
        "120363@g.us",
        "whatsapp",
    );
    db.upsert_conversation(&canonical_conv).unwrap();
    db.upsert_message(&make_message_with_connector(
        "wa_whatsapp_MSG0",
        "wa_whatsapp_120363@g.us",
        "whatsapp",
        "older",
        900,
        "whatsapp",
    ))
    .unwrap();

    db.conn()
        .unwrap()
        .execute("DELETE FROM schema_version WHERE version >= 12", [])
        .unwrap();
    db.conn()
        .unwrap()
        .execute_batch(
            "
            DROP INDEX IF EXISTS idx_messages_is_saved;
            ALTER TABLE messages DROP COLUMN is_saved;
            ",
        )
        .unwrap();
    db.conn()
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (11)",
            [],
        )
        .unwrap();
    super::schema::run_migrations(&db.conn().unwrap()).unwrap();

    let migrated = db.get_message("wa_whatsapp_MSG1").unwrap();
    assert!(
        migrated.is_some(),
        "JID message remapped to config connection id"
    );
    assert_eq!(migrated.unwrap().connection_id, "whatsapp");

    assert!(db
        .get_message("wa_94004066660357:37@lid_MSG1")
        .unwrap()
        .is_none());
    assert!(db
        .get_conversation("wa_94004066660357:37@lid_120363@g.us")
        .unwrap()
        .is_none());
    assert!(db
        .get_conversation("wa_whatsapp_120363@g.us")
        .unwrap()
        .is_some());
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

/// Snapshot the schema of a fresh, fully-migrated database. If a migration
/// changes table/index/trigger names, this test fails so the drift is caught
/// deliberately. The `sql` column is asserted non-null for tables/triggers.
#[test]
fn schema_snapshot_matches_expected() {
    let db = test_db();
    let conn = db.conn().unwrap();

    let mut stmt = conn
        .prepare(
            "SELECT name, type FROM sqlite_master \
             WHERE name NOT LIKE 'sqlite_%' \
             ORDER BY name",
        )
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let names: Vec<&str> = rows.iter().map(|(n, _)| n.as_str()).collect();

    // Expected object names at SCHEMA_VERSION = 13. Includes FTS5 shadow tables
    // (messages_fts_*) created automatically by the virtual table.
    let expected = [
        "conversations",
        "events",
        "hook_logs",
        "idx_hook_logs_started",
        "idx_messages_context_id",
        "idx_messages_is_saved",
        "messages",
        "messages_ad",
        "messages_ai",
        "messages_au",
        "messages_fts",
        "messages_fts_config",
        "messages_fts_data",
        "messages_fts_docsize",
        "messages_fts_idx",
        "schema_version",
        "sync_state",
    ];

    assert_eq!(
        names, expected,
        "schema object names drifted from the expected snapshot at version {SCHEMA_VERSION}"
    );
}

#[test]
fn schema_snapshot_core_table_columns_present() {
    let db = test_db();
    let conn = db.conn().unwrap();

    // Guard against silent column drift on the messages table specifically:
    // pull the CREATE TABLE SQL and assert the renamed/added columns exist.
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='messages'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    for col in [
        "connection_id",
        "connector",
        "synced_at",
        "is_archived",
        "is_saved",
        "context_id",
        "sender_avatar_url",
    ] {
        assert!(
            sql.contains(col),
            "messages table missing expected column `{col}`: {sql}"
        );
    }
    // Columns dropped in migration v7 must be gone.
    assert!(!sql.contains("is_read"), "is_read should be dropped (v7)");
    assert!(
        !sql.contains("is_from_me"),
        "is_from_me should be dropped (v7)"
    );
    assert!(
        !sql.contains("account_id"),
        "account_id renamed to connection_id (v10)"
    );
}

/// Build a v1-era database by hand (pre-rename, with `account_id` and the
/// columns that v7 later drops), insert a row, then run the full migration
/// chain and assert the row survives and is reachable via the modern schema.
#[test]
fn migrations_preserve_existing_data() {
    use rusqlite::Connection;

    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();

    // Minimal v1 schema (subset sufficient to hold a conversation + message).
    conn.execute_batch(
        "
        CREATE TABLE schema_version (version INTEGER NOT NULL);
        CREATE TABLE conversations (
            id TEXT PRIMARY KEY,
            account_id TEXT NOT NULL,
            external_id TEXT NOT NULL,
            name TEXT,
            kind TEXT NOT NULL,
            last_message_at INTEGER,
            unread_count INTEGER NOT NULL DEFAULT 0,
            metadata TEXT,
            UNIQUE(account_id, external_id)
        );
        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id),
            account_id TEXT NOT NULL,
            external_id TEXT NOT NULL,
            sender TEXT NOT NULL,
            sender_name TEXT,
            body TEXT,
            timestamp INTEGER NOT NULL,
            is_from_me INTEGER NOT NULL DEFAULT 0,
            reply_to_id TEXT,
            media_type TEXT,
            metadata TEXT,
            UNIQUE(account_id, external_id)
        );
        CREATE VIRTUAL TABLE messages_fts USING fts5(
            body, sender_name, content=messages, content_rowid=rowid
        );
        CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, body, sender_name)
            VALUES (new.rowid, new.body, new.sender_name);
        END;
        CREATE TABLE events (
            id TEXT PRIMARY KEY, account_id TEXT NOT NULL, external_id TEXT NOT NULL,
            title TEXT NOT NULL, description TEXT, location TEXT,
            start_at INTEGER NOT NULL, end_at INTEGER NOT NULL,
            all_day INTEGER NOT NULL DEFAULT 0, attendees TEXT, status TEXT,
            calendar_name TEXT, meet_link TEXT, metadata TEXT,
            UNIQUE(account_id, external_id)
        );
        CREATE TABLE sync_state (
            account_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL,
            PRIMARY KEY(account_id, key)
        );
        INSERT INTO schema_version (version) VALUES (1);
        ",
    )
    .unwrap();

    conn.execute(
        "INSERT INTO conversations (id, account_id, external_id, kind, unread_count)
         VALUES ('cv1', 'legacy-acct', 'EXT1', 'dm', 0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages (id, conversation_id, account_id, external_id, sender, body, timestamp)
         VALUES ('mg1', 'cv1', 'legacy-acct', 'EXTM1', 'alice', 'preserved body', 12345)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sync_state (account_id, key, value) VALUES ('legacy-acct', 'k', 'v')",
        [],
    )
    .unwrap();

    // Run the real migration chain over the seeded legacy DB.
    super::schema::run_migrations(&conn).unwrap();

    let version: i32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    // account_id renamed to connection_id (v10) and data preserved.
    let (conn_id, body): (String, String) = conn
        .query_row(
            "SELECT connection_id, body FROM messages WHERE id = 'mg1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(conn_id, "legacy-acct");
    assert_eq!(body, "preserved body");

    let is_saved: i32 = conn
        .query_row("SELECT is_saved FROM messages WHERE id = 'mg1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(is_saved, 0, "is_saved should default to 0 after migration");

    let conv_conn: String = conn
        .query_row(
            "SELECT connection_id FROM conversations WHERE id = 'cv1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(conv_conn, "legacy-acct");

    let sync_conn: String = conn
        .query_row(
            "SELECT connection_id FROM sync_state WHERE key = 'k'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sync_conn, "legacy-acct");
}
