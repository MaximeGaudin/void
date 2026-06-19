use std::collections::HashSet;

use super::fixtures::*;

#[test]
fn reconcile_saved_marks_correct_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "saved one", 1_000);
    m1.external_id = "ts-1".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "not saved", 2_000);
    m2.external_id = "ts-2".into();
    db.upsert_message(&m2).unwrap();

    let saved = HashSet::from(["ts-1".to_string()]);
    let (newly_saved, newly_unsaved) = db.reconcile_saved("test-slack", "slack", &saved).unwrap();
    assert_eq!(newly_saved, 1);
    assert_eq!(newly_unsaved, 0);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert!(loaded.is_saved);
    let other = db.get_message("m2").unwrap().unwrap();
    assert!(!other.is_saved);
}

#[test]
fn reconcile_saved_clears_previously_saved() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "was saved", 1_000);
    m1.external_id = "ts-1".into();
    db.upsert_message(&m1).unwrap();

    let saved = HashSet::from(["ts-1".to_string()]);
    db.reconcile_saved("test-slack", "slack", &saved).unwrap();

    let empty = HashSet::new();
    let (newly_saved, newly_unsaved) = db.reconcile_saved("test-slack", "slack", &empty).unwrap();
    assert_eq!(newly_saved, 0);
    assert_eq!(newly_unsaved, 1);

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert!(!loaded.is_saved);
}

#[test]
fn list_saved_returns_only_saved() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message("m1", "c1", "test-slack", "saved", 1_000);
    m1.external_id = "ts-1".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c1", "test-slack", "not saved", 2_000);
    m2.external_id = "ts-2".into();
    db.upsert_message(&m2).unwrap();

    db.reconcile_saved("test-slack", "slack", &HashSet::from(["ts-1".to_string()]))
        .unwrap();

    let (rows, total) = db.list_saved_messages(None, None, 50, 0).unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "m1");
}

#[test]
fn list_saved_respects_connection_filter() {
    let db = test_db();
    let conv1 = make_conversation("c1", "work-slack", "C1");
    let conv2 = make_conversation("c2", "home-slack", "C2");
    db.upsert_conversation(&conv1).unwrap();
    db.upsert_conversation(&conv2).unwrap();

    let mut m1 = make_message("m1", "c1", "work-slack", "work saved", 1_000);
    m1.external_id = "ts-1".into();
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message("m2", "c2", "home-slack", "home saved", 2_000);
    m2.external_id = "ts-2".into();
    db.upsert_message(&m2).unwrap();

    db.reconcile_saved("work-slack", "slack", &HashSet::from(["ts-1".to_string()]))
        .unwrap();
    db.reconcile_saved("home-slack", "slack", &HashSet::from(["ts-2".to_string()]))
        .unwrap();

    let (rows, total) = db.list_saved_messages(Some("work"), None, 50, 0).unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].connection_id, "work-slack");
}

#[test]
fn count_saved_matches_list() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    for (id, ext, ts) in [("m1", "ts-1", 1_000), ("m2", "ts-2", 2_000)] {
        let mut msg = make_message(id, "c1", "test-slack", "saved", ts);
        msg.external_id = ext.into();
        db.upsert_message(&msg).unwrap();
    }

    db.reconcile_saved(
        "test-slack",
        "slack",
        &HashSet::from(["ts-1".to_string(), "ts-2".to_string()]),
    )
    .unwrap();

    let (rows, total) = db.list_saved_messages(None, None, 50, 0).unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.len(), 2);
}
