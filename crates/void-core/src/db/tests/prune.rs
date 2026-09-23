use super::fixtures::*;

#[test]
fn prune_messages_before_drops_old_rows_and_keeps_saved() {
    let db = test_db();
    let old_conv = make_conversation("old", "test-slack", "OLD");
    db.upsert_conversation(&old_conv).unwrap();
    let live_conv = make_conversation("live", "test-slack", "LIVE");
    db.upsert_conversation(&live_conv).unwrap();
    let mut gmail_conv = make_conversation("gmail", "me@gmail.com", "G1");
    gmail_conv.connector = "gmail".into();
    db.upsert_conversation(&gmail_conv).unwrap();

    let mut old = make_message("old", "old", "test-slack", "stale", 1_000);
    old.metadata = Some(serde_json::json!({
        "files": [{ "local_path": "/tmp/void-old.jpg" }]
    }));
    db.upsert_message(&old).unwrap();

    let mut saved = make_message("saved", "old", "test-slack", "pin", 1_000);
    saved.is_saved = true;
    saved.metadata = Some(serde_json::json!({
        "files": [{ "local_path": "/tmp/void-saved.jpg" }]
    }));
    db.upsert_message(&saved).unwrap();

    db.upsert_message(&make_message(
        "boundary",
        "live",
        "test-slack",
        "edge",
        2_000,
    ))
    .unwrap();
    db.upsert_message(&make_message("fresh", "live", "test-slack", "new", 3_000))
        .unwrap();

    let mut gmail_old = make_message("gmail-old", "gmail", "me@gmail.com", "newsletter", 500);
    gmail_old.connector = "gmail".into();
    db.upsert_message(&gmail_old).unwrap();

    let pruned = db.prune_messages_before(2_000).unwrap();

    assert_eq!(pruned.messages_deleted, 2);
    assert_eq!(pruned.conversations_deleted, 1);
    assert_eq!(pruned.file_paths, vec!["/tmp/void-old.jpg".to_string()]);

    assert!(db.get_message("old").unwrap().is_none());
    assert!(db.get_message("gmail-old").unwrap().is_none());
    assert!(db.get_conversation("gmail").unwrap().is_none());

    assert!(
        db.get_message("saved").unwrap().is_some(),
        "saved rows stay"
    );
    assert!(db.get_conversation("old").unwrap().is_some());
    assert!(db.get_message("boundary").unwrap().is_some());
    assert!(db.get_message("fresh").unwrap().is_some());
    assert!(db.get_conversation("live").unwrap().is_some());
}
