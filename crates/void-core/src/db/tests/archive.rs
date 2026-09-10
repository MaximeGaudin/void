use super::fixtures::*;

fn with_synced_at(mut msg: crate::models::Message, synced_at: i64) -> crate::models::Message {
    msg.synced_at = Some(synced_at);
    msg
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
fn bulk_archive_before_archives_strictly_older_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&with_synced_at(
        make_message("m1", "c1", "test-slack", "old", 1_000),
        1_000,
    ))
    .unwrap();
    db.upsert_message(&with_synced_at(
        make_message("m2", "c1", "test-slack", "boundary", 2_000),
        2_000,
    ))
    .unwrap();
    db.upsert_message(&with_synced_at(
        make_message("m3", "c1", "test-slack", "new", 3_000),
        3_000,
    ))
    .unwrap();

    // cutoff is exclusive: timestamp < 2000 → only m1.
    let archived = db.bulk_archive_before(2_000, None).unwrap();
    let archived_ids: Vec<&str> = archived.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(archived_ids, ["m1"], "only strictly-older message archived");

    assert!(db.get_message("m1").unwrap().unwrap().is_archived);
    assert!(
        !db.get_message("m2").unwrap().unwrap().is_archived,
        "boundary timestamp (==cutoff) is NOT archived"
    );
    assert!(!db.get_message("m3").unwrap().unwrap().is_archived);
}

#[test]
fn bulk_archive_before_respects_connector_filter() {
    let db = test_db();
    let slack_conv = make_conversation("c1", "test-slack", "C1");
    db.upsert_conversation(&slack_conv).unwrap();
    let mut gmail_conv = make_conversation("c2", "test-gmail", "G1");
    gmail_conv.connector = "gmail".into();
    db.upsert_conversation(&gmail_conv).unwrap();

    db.upsert_message(&with_synced_at(
        make_message("s1", "c1", "test-slack", "slack old", 1_000),
        1_000,
    ))
    .unwrap();
    db.upsert_message(&with_synced_at(
        make_message_with_connector("g1", "c2", "test-gmail", "gmail old", 1_000, "gmail"),
        1_000,
    ))
    .unwrap();

    let archived = db.bulk_archive_before(5_000, Some("gmail")).unwrap();
    let ids: Vec<&str> = archived.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["g1"], "only gmail messages archived");

    assert!(db.get_message("g1").unwrap().unwrap().is_archived);
    assert!(
        !db.get_message("s1").unwrap().unwrap().is_archived,
        "slack message untouched by gmail filter"
    );
}

#[test]
fn bulk_archive_before_skips_already_archived() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = with_synced_at(
        make_message("m1", "c1", "test-slack", "already", 1_000),
        1_000,
    );
    m1.is_archived = true;
    db.upsert_message(&m1).unwrap();
    db.upsert_message(&with_synced_at(
        make_message("m2", "c1", "test-slack", "fresh", 1_500),
        1_500,
    ))
    .unwrap();

    let archived = db.bulk_archive_before(2_000, None).unwrap();
    let ids: Vec<&str> = archived.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["m2"],
        "returned set excludes messages already archived"
    );
}

#[test]
fn bulk_archive_before_empty_result_when_nothing_matches() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();
    db.upsert_message(&make_message("m1", "c1", "test-slack", "new", 5_000))
        .unwrap();

    let archived = db.bulk_archive_before(1_000, None).unwrap();
    assert!(archived.is_empty(), "no message older than cutoff");
    assert!(!db.get_message("m1").unwrap().unwrap().is_archived);
}

#[test]
fn bulk_archive_before_skips_recently_synced_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&with_synced_at(
        make_message("m1", "c1", "test-slack", "old send, fresh sync", 1_000),
        5_000,
    ))
    .unwrap();

    let archived = db.bulk_archive_before(3_000, None).unwrap();
    assert!(
        archived.is_empty(),
        "recently synced message must not be bulk-archived"
    );
    assert!(
        !db.get_message("m1").unwrap().unwrap().is_archived,
        "message with old timestamp but recent synced_at stays unarchived"
    );
}

#[test]
fn upsert_preserves_user_archived_flag() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let msg = make_message("m1", "c1", "test-slack", "hello", 1_000);
    db.upsert_message(&msg).unwrap();
    assert!(db.mark_message_archived("m1").unwrap());

    let mut resync = make_message("m1", "c1", "test-slack", "hello edited", 1_000);
    resync.is_archived = false;
    resync.body = Some("hello edited".into());
    db.upsert_message(&resync).unwrap();

    let loaded = db.get_message("m1").unwrap().unwrap();
    assert!(
        loaded.is_archived,
        "re-sync with is_archived=false must not un-archive user decision"
    );
    assert_eq!(loaded.body.as_deref(), Some("hello edited"));
}

#[test]
fn mark_archived_with_context_archives_all_siblings() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    let ctx = "slack-group-C123-1000";
    db.upsert_message(&make_message_with_context(
        "m1", "c1", "test-slack", "old", 1_000, Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2", "c1", "test-slack", "mid", 2_000, Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m3", "c1", "test-slack", "new", 3_000, Some(ctx),
    ))
    .unwrap();
    // Different context must stay unarchived.
    db.upsert_message(&make_message_with_context(
        "m4",
        "c1",
        "test-slack",
        "other",
        4_000,
        Some("slack-group-other"),
    ))
    .unwrap();

    let archived = db.mark_message_archived_with_context("m3").unwrap();
    let mut ids: Vec<_> = archived.iter().map(|m| m.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["m1", "m2", "m3"]);

    assert!(db.get_message("m1").unwrap().unwrap().is_archived);
    assert!(db.get_message("m2").unwrap().unwrap().is_archived);
    assert!(db.get_message("m3").unwrap().unwrap().is_archived);
    assert!(
        !db.get_message("m4").unwrap().unwrap().is_archived,
        "other context untouched"
    );

    // Inbox must not promote a sibling from the archived group.
    let (rows, _) = db
        .recent_messages_paginated(None, Some("slack"), 50, 0, false, true, true)
        .unwrap();
    assert!(
        rows.iter().all(|m| m.id != "m1" && m.id != "m2" && m.id != "m3"),
        "archived context group must leave inbox"
    );
}

#[test]
fn mark_archived_with_context_single_message_without_context() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message("solo", "c1", "test-slack", "hi", 1_000))
        .unwrap();

    let archived = db.mark_message_archived_with_context("solo").unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "solo");
    assert!(db.get_message("solo").unwrap().unwrap().is_archived);
}
