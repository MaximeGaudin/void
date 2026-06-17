use super::fixtures::*;

use crate::models::ConversationKind;

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
fn sync_ignore_matches_by_name() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "my-wa", "111@g.us");
    c1.name = Some("Family Group".into());
    db.upsert_conversation(&c1).unwrap();

    let mut c2 = make_conversation("c2", "my-wa", "222@g.us");
    c2.name = Some("Work Updates".into());
    db.upsert_conversation(&c2).unwrap();

    let patterns = vec!["family".to_string()];
    let (muted, unmuted) = db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert_eq!(muted, 1);
    assert_eq!(unmuted, 0);

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded.is_muted, "Family Group should be muted");

    let loaded2 = db.get_conversation("c2").unwrap().unwrap();
    assert!(!loaded2.is_muted, "Work Updates should not be muted");
}

#[test]
fn sync_ignore_matches_by_external_id() {
    let db = test_db();
    let c1 = make_conversation("c1", "my-wa", "spam-group@g.us");
    db.upsert_conversation(&c1).unwrap();

    let patterns = vec!["spam-group".to_string()];
    let (muted, _) = db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert_eq!(muted, 1);

    let loaded = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded.is_muted);
}

#[test]
fn sync_ignore_case_insensitive() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "my-wa", "111@g.us");
    c1.name = Some("NOISY GROUP".into());
    db.upsert_conversation(&c1).unwrap();

    let patterns = vec!["noisy".to_string()];
    let (muted, _) = db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert_eq!(muted, 1);
}

#[test]
fn sync_ignore_is_idempotent() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "my-wa", "111@g.us");
    c1.name = Some("Spam".into());
    db.upsert_conversation(&c1).unwrap();
    db.update_conversation_mute("c1", true).unwrap();

    let patterns = vec!["spam".to_string()];
    let (muted, unmuted) = db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert_eq!(muted, 0);
    assert_eq!(unmuted, 0);
}

#[test]
fn sync_ignore_scoped_to_connection() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "wa-1", "111@g.us");
    c1.name = Some("Random".into());
    db.upsert_conversation(&c1).unwrap();

    let mut c2 = make_conversation("c2", "wa-2", "222@g.us");
    c2.name = Some("Random".into());
    db.upsert_conversation(&c2).unwrap();

    let patterns = vec!["random".to_string()];
    let (muted, _) = db.sync_ignore_conversations("wa-1", &patterns).unwrap();
    assert_eq!(muted, 1);

    let loaded1 = db.get_conversation("c1").unwrap().unwrap();
    assert!(loaded1.is_muted);

    let loaded2 = db.get_conversation("c2").unwrap().unwrap();
    assert!(!loaded2.is_muted, "other connection should not be affected");
}

#[test]
fn sync_ignore_multiple_patterns() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "my-wa", "111@g.us");
    c1.name = Some("Random Chat".into());
    db.upsert_conversation(&c1).unwrap();

    let mut c2 = make_conversation("c2", "my-wa", "222@g.us");
    c2.name = Some("Social Club".into());
    db.upsert_conversation(&c2).unwrap();

    let mut c3 = make_conversation("c3", "my-wa", "333@g.us");
    c3.name = Some("Important Work".into());
    db.upsert_conversation(&c3).unwrap();

    let patterns = vec!["random".to_string(), "social".to_string()];
    let (muted, _) = db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert_eq!(muted, 2);

    assert!(db.get_conversation("c1").unwrap().unwrap().is_muted);
    assert!(db.get_conversation("c2").unwrap().unwrap().is_muted);
    assert!(!db.get_conversation("c3").unwrap().unwrap().is_muted);
}

#[test]
fn sync_ignore_empty_patterns_unmutes_all() {
    let db = test_db();
    let c1 = make_conversation("c1", "my-wa", "111@g.us");
    db.upsert_conversation(&c1).unwrap();
    db.update_conversation_mute("c1", true).unwrap();

    let (muted, unmuted) = db.sync_ignore_conversations("my-wa", &[]).unwrap();
    assert_eq!(muted, 0);
    assert_eq!(unmuted, 1);
    assert!(!db.get_conversation("c1").unwrap().unwrap().is_muted);
}

#[test]
fn sync_ignore_unmutes_when_pattern_removed() {
    let db = test_db();
    let mut c1 = make_conversation("c1", "my-wa", "111@g.us");
    c1.name = Some("Family Group".into());
    db.upsert_conversation(&c1).unwrap();

    let patterns = vec!["family".to_string()];
    db.sync_ignore_conversations("my-wa", &patterns).unwrap();
    assert!(db.get_conversation("c1").unwrap().unwrap().is_muted);

    let (muted, unmuted) = db.sync_ignore_conversations("my-wa", &[]).unwrap();
    assert_eq!(muted, 0);
    assert_eq!(unmuted, 1);
    assert!(!db.get_conversation("c1").unwrap().unwrap().is_muted);
}
