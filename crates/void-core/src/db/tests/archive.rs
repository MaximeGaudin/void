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
        "m1",
        "c1",
        "test-slack",
        "old",
        1_000,
        Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "mid",
        2_000,
        Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m3",
        "c1",
        "test-slack",
        "new",
        3_000,
        Some(ctx),
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
        rows.iter()
            .all(|m| m.id != "m1" && m.id != "m2" && m.id != "m3"),
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

#[test]
fn mark_archived_with_context_is_noop_when_group_already_archived() {
    let db = test_db();
    db.upsert_conversation(&make_conversation("c1", "test-slack", "C123"))
        .unwrap();

    let ctx = "slack-group-C123-1000";
    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "old",
        1_000,
        Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "new",
        2_000,
        Some(ctx),
    ))
    .unwrap();

    assert_eq!(
        db.mark_message_archived_with_context("m2").unwrap().len(),
        2
    );
    // Second call has nothing left to archive.
    assert!(db
        .mark_message_archived_with_context("m2")
        .unwrap()
        .is_empty());
    assert!(db.get_message("m1").unwrap().unwrap().is_archived);
    assert!(db.get_message("m2").unwrap().unwrap().is_archived);
}

#[test]
fn mark_archived_with_context_is_noop_when_single_message_already_archived() {
    let db = test_db();
    db.upsert_conversation(&make_conversation("c1", "test-slack", "C123"))
        .unwrap();
    db.upsert_message(&make_message("solo", "c1", "test-slack", "hi", 1_000))
        .unwrap();

    assert_eq!(
        db.mark_message_archived_with_context("solo").unwrap().len(),
        1
    );
    assert!(db
        .mark_message_archived_with_context("solo")
        .unwrap()
        .is_empty());
}

#[test]
fn mark_archived_with_context_spans_conversations() {
    let db = test_db();
    db.upsert_conversation(&make_conversation("c1", "test-slack", "C123"))
        .unwrap();
    db.upsert_conversation(&make_conversation("c2", "test-slack", "C456"))
        .unwrap();

    let ctx = "slack-thread-1000";
    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "here",
        1_000,
        Some(ctx),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c2",
        "test-slack",
        "there",
        2_000,
        Some(ctx),
    ))
    .unwrap();

    let archived = db.mark_message_archived_with_context("m1").unwrap();
    let mut convs: Vec<_> = archived
        .iter()
        .map(|m| m.conversation_id.as_str())
        .collect();
    convs.sort();
    assert_eq!(
        convs,
        ["c1", "c2"],
        "siblings keep their own conversation id"
    );
}

#[test]
fn mark_archived_with_context_unknown_id_is_noop() {
    let db = test_db();
    assert!(db
        .mark_message_archived_with_context("nope")
        .unwrap()
        .is_empty());
}

#[test]
fn reconcile_inbox_conversations_follows_thread_state() {
    let db = test_db();
    db.upsert_conversation(&make_conversation_with_connector(
        "acct-t1", "acct", "t1", "gmail",
    ))
    .unwrap();
    db.upsert_conversation(&make_conversation_with_connector(
        "acct-t2", "acct", "t2", "gmail",
    ))
    .unwrap();

    // t1 is in the inbox: its archived first message must come back.
    let mut old = make_message_with_connector("m1", "acct-t1", "acct", "old", 1_000, "gmail");
    old.is_archived = true;
    db.upsert_message(&old).unwrap();
    db.upsert_message(&make_message_with_connector(
        "m2", "acct-t1", "acct", "reply", 2_000, "gmail",
    ))
    .unwrap();
    // t2 left the inbox.
    db.upsert_message(&make_message_with_connector(
        "m3", "acct-t2", "acct", "gone", 3_000, "gmail",
    ))
    .unwrap();

    let inbox: std::collections::HashSet<String> = ["acct-t1".to_string()].into();
    let (unarchived, archived) = db
        .reconcile_inbox_conversations("acct", "gmail", &inbox)
        .unwrap();
    assert_eq!((unarchived, archived), (1, 1));
    assert!(!db.get_message("m1").unwrap().unwrap().is_archived);
    assert!(!db.get_message("m2").unwrap().unwrap().is_archived);
    assert!(db.get_message("m3").unwrap().unwrap().is_archived);
}

#[test]
fn set_conversation_archived_touches_only_that_conversation() {
    let db = test_db();
    db.upsert_conversation(&make_conversation("c1", "test-slack", "C1"))
        .unwrap();
    db.upsert_conversation(&make_conversation("c2", "test-slack", "C2"))
        .unwrap();
    for (id, conv) in [("m1", "c1"), ("m2", "c1"), ("m3", "c2")] {
        let mut msg = make_message(id, conv, "test-slack", "x", 1_000);
        msg.is_archived = true;
        db.upsert_message(&msg).unwrap();
    }

    assert_eq!(db.set_conversation_archived("c1", false).unwrap(), 2);
    assert_eq!(db.set_conversation_archived("c1", false).unwrap(), 0);
    assert!(!db.get_message("m1").unwrap().unwrap().is_archived);
    assert!(!db.get_message("m2").unwrap().unwrap().is_archived);
    assert!(db.get_message("m3").unwrap().unwrap().is_archived);
}
