use super::fixtures::*;

use crate::models::ConversationKind;

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
fn self_chat_conversation_kind_round_trip() {
    let db = test_db();
    let mut conv = make_conversation("c-self", "test-whatsapp", "94004066660357@lid");
    conv.connector = "whatsapp".into();
    conv.kind = ConversationKind::SelfChat;
    conv.name = Some("Message yourself".into());
    db.upsert_conversation(&conv).unwrap();

    let loaded = db
        .get_conversation("c-self")
        .unwrap()
        .expect("conversation");
    assert_eq!(loaded.kind, ConversationKind::SelfChat);
    assert_eq!(loaded.name.as_deref(), Some("Message yourself"));
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
fn backfill_avatar_urls_updates_null_rows() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "U001".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
    m2.sender = "U001".into();
    db.upsert_message(&m2).unwrap();

    let mut m3 = make_message("m3", "c1", "test-slack", "yo", 3_000);
    m3.sender = "U002".into();
    db.upsert_message(&m3).unwrap();

    let avatars: std::collections::HashMap<String, String> = [
        ("U001".into(), "https://example.com/u1.jpg".into()),
        ("U002".into(), "https://example.com/u2.jpg".into()),
    ]
    .into();

    let updated = db
        .backfill_avatar_urls("test-slack", "slack", &avatars)
        .unwrap();
    assert_eq!(updated, 3);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert_eq!(
        loaded.sender_avatar_url.as_deref(),
        Some("https://example.com/u1.jpg")
    );
    let loaded3 = db.get_message("m3").unwrap().unwrap();
    assert_eq!(
        loaded3.sender_avatar_url.as_deref(),
        Some("https://example.com/u2.jpg")
    );
}

#[test]
fn backfill_avatar_urls_skips_already_set() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "U001".into();
    m1.sender_avatar_url = Some("https://existing.com/old.jpg".into());
    db.upsert_message(&m1).unwrap();

    let avatars: std::collections::HashMap<String, String> =
        [("U001".into(), "https://new.com/new.jpg".into())].into();

    let updated = db
        .backfill_avatar_urls("test-slack", "slack", &avatars)
        .unwrap();
    assert_eq!(updated, 0);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert_eq!(
        loaded.sender_avatar_url.as_deref(),
        Some("https://existing.com/old.jpg"),
    );
}

#[test]
fn backfill_avatar_urls_scoped_to_connection() {
    let db = test_db();
    let c1 = make_conversation("c1", "slack-a", "C1");
    db.upsert_conversation(&c1).unwrap();
    let c2 = make_conversation("c2", "slack-b", "C2");
    db.upsert_conversation(&c2).unwrap();

    let mut m1 = make_message("m1", "c1", "slack-a", "hi", 1_000);
    m1.sender = "U001".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c2", "slack-b", "hey", 2_000);
    m2.sender = "U001".into();
    db.upsert_message(&m2).unwrap();

    let avatars: std::collections::HashMap<String, String> =
        [("U001".into(), "https://example.com/a.jpg".into())].into();

    let updated = db
        .backfill_avatar_urls("slack-a", "slack", &avatars)
        .unwrap();
    assert_eq!(updated, 1);

    let loaded_a = db.get_message("m1").unwrap().unwrap();
    assert_eq!(
        loaded_a.sender_avatar_url.as_deref(),
        Some("https://example.com/a.jpg")
    );
    let loaded_b = db.get_message("m2").unwrap().unwrap();
    assert!(loaded_b.sender_avatar_url.is_none());
}

#[test]
fn senders_missing_avatar_returns_distinct_ids() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "U001".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
    m2.sender = "U001".into();
    db.upsert_message(&m2).unwrap();

    let mut m3 = make_message("m3", "c1", "test-slack", "yo", 3_000);
    m3.sender = "U002".into();
    m3.sender_avatar_url = Some("https://example.com/u2.jpg".into());
    db.upsert_message(&m3).unwrap();

    let missing = db.senders_missing_avatar("test-slack", "slack").unwrap();
    assert_eq!(missing, vec!["U001".to_string()]);
}

#[test]
fn senders_missing_avatar_empty_when_all_set() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "U001".into();
    m1.sender_avatar_url = Some("https://example.com/u1.jpg".into());
    db.upsert_message(&m1).unwrap();

    let missing = db.senders_missing_avatar("test-slack", "slack").unwrap();
    assert!(missing.is_empty());
}

#[test]
fn list_contacts_includes_avatar_url() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "hi", 1_000);
    m1.sender = "alice@test.com".into();
    m1.sender_name = Some("Alice".into());
    m1.sender_avatar_url = Some("https://example.com/alice.jpg".into());
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "hey", 2_000);
    m2.sender = "bob@test.com".into();
    m2.sender_name = Some("Bob".into());
    db.upsert_message(&m2).unwrap();

    let contacts = db.list_contacts(None, None, None, 100).unwrap();
    assert_eq!(contacts.len(), 2);

    let alice = contacts
        .iter()
        .find(|c| c.sender == "alice@test.com")
        .unwrap();
    assert_eq!(
        alice.avatar_url.as_deref(),
        Some("https://example.com/alice.jpg")
    );

    let bob = contacts
        .iter()
        .find(|c| c.sender == "bob@test.com")
        .unwrap();
    assert!(bob.avatar_url.is_none());
}

#[test]
fn list_contacts_avatar_picks_non_null_across_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "old msg", 1_000);
    m1.sender = "U001".into();
    m1.sender_name = Some("User1".into());
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "new msg", 2_000);
    m2.sender = "U001".into();
    m2.sender_name = Some("User1".into());
    m2.sender_avatar_url = Some("https://example.com/u1.jpg".into());
    db.upsert_message(&m2).unwrap();

    let contacts = db.list_contacts(None, None, None, 100).unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(
        contacts[0].avatar_url.as_deref(),
        Some("https://example.com/u1.jpg"),
    );
}
