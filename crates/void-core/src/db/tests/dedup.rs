use super::fixtures::*;

#[test]
fn dedup_context_keeps_most_recent_per_group() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "old ctx msg",
        1_000,
        Some("ctx-A"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "new ctx msg",
        2_000,
        Some("ctx-A"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m3",
        "c1",
        "test-slack",
        "standalone",
        3_000,
        None,
    ))
    .unwrap();

    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, true, true, true)
        .unwrap();
    assert_eq!(total, 2, "count should collapse ctx-A group to 1");
    assert_eq!(rows.len(), 2);
    let ids: Vec<&str> = rows.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"m2"), "most recent in ctx-A kept");
    assert!(ids.contains(&"m3"), "NULL context_id always kept");
    assert!(!ids.contains(&"m1"), "older ctx-A member removed");
}

#[test]
fn dedup_context_disabled_returns_all() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "old",
        1_000,
        Some("ctx-A"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "new",
        2_000,
        Some("ctx-A"),
    ))
    .unwrap();

    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, true, true, false)
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.len(), 2);
}

#[test]
fn dedup_context_pagination_metadata_matches_data() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    // 3 context groups of 2 + 2 standalone = 8 messages, 5 after dedup
    for (id, ts, ctx) in [
        ("m1", 1_000, Some("ctx-A")),
        ("m2", 2_000, Some("ctx-A")),
        ("m3", 3_000, Some("ctx-B")),
        ("m4", 4_000, Some("ctx-B")),
        ("m5", 5_000, Some("ctx-C")),
        ("m6", 6_000, Some("ctx-C")),
        ("m7", 7_000, None),
        ("m8", 8_000, None),
    ] {
        db.upsert_message(&make_message_with_context(
            id,
            "c1",
            "test-slack",
            "body",
            ts,
            ctx,
        ))
        .unwrap();
    }

    let (page1, total1) = db
        .recent_messages_paginated(None, None, 3, 0, true, true, true)
        .unwrap();
    let (page2, total2) = db
        .recent_messages_paginated(None, None, 3, 3, true, true, true)
        .unwrap();

    assert_eq!(total1, 5, "total should reflect deduped count");
    assert_eq!(total2, 5);
    assert_eq!(page1.len(), 3);
    assert_eq!(page2.len(), 2);

    let all_ids: Vec<&str> = page1
        .iter()
        .chain(page2.iter())
        .map(|m| m.id.as_str())
        .collect();
    assert!(!all_ids.contains(&"m1"), "older ctx-A removed");
    assert!(!all_ids.contains(&"m3"), "older ctx-B removed");
    assert!(!all_ids.contains(&"m5"), "older ctx-C removed");
}

#[test]
fn dedup_context_conversation_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "older",
        1_000,
        Some("ctx-X"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "newer",
        2_000,
        Some("ctx-X"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m3",
        "c1",
        "test-slack",
        "solo",
        3_000,
        None,
    ))
    .unwrap();

    let (rows, total) = db
        .list_messages_paginated("c1", 50, 0, None, None, true)
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.len(), 2);
    let ids: Vec<&str> = rows.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"m2"));
    assert!(ids.contains(&"m3"));
}

#[test]
fn dedup_context_search_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "meeting old",
        1_000,
        Some("ctx-Y"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "meeting new",
        2_000,
        Some("ctx-Y"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m3",
        "c1",
        "test-slack",
        "meeting solo",
        3_000,
        None,
    ))
    .unwrap();

    let (rows, total) = db
        .search_messages_paginated("meeting", None, None, 50, 0, true, true)
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows.len(), 2);
    let ids: Vec<&str> = rows.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"m2"), "most recent ctx-Y kept");
    assert!(ids.contains(&"m3"), "NULL context kept");
}

#[test]
fn dedup_context_same_timestamp_uses_id_tiebreak() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m-aaa",
        "c1",
        "test-slack",
        "first",
        1_000,
        Some("ctx-T"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m-zzz",
        "c1",
        "test-slack",
        "second",
        1_000,
        Some("ctx-T"),
    ))
    .unwrap();

    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, true, true, true)
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "m-zzz", "highest id wins on timestamp tie");
}

#[test]
fn dedup_inbox_shows_thread_when_latest_message_is_archived() {
    let db = test_db();
    let conv = make_conversation("c1", "test-gmail", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message_with_context(
        "m1",
        "c1",
        "test-gmail",
        "unarchived msg",
        1_000,
        Some("thread-1"),
    );
    m1.is_archived = false;
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message_with_context(
        "m2",
        "c1",
        "test-gmail",
        "archived reply",
        2_000,
        Some("thread-1"),
    );
    m2.is_archived = true;
    db.upsert_message(&m2).unwrap();

    // Inbox view (include_archived=false, dedup=true): should show m1 as thread representative
    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, false, true, true)
        .unwrap();
    assert_eq!(total, 1, "thread should appear once in inbox");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].id, "m1",
        "latest unarchived message should represent the thread"
    );
}

#[test]
fn dedup_inbox_hides_fully_archived_thread() {
    let db = test_db();
    let conv = make_conversation("c1", "test-gmail", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message_with_context(
        "m1",
        "c1",
        "test-gmail",
        "old archived",
        1_000,
        Some("thread-1"),
    );
    m1.is_archived = true;
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message_with_context(
        "m2",
        "c1",
        "test-gmail",
        "new archived",
        2_000,
        Some("thread-1"),
    );
    m2.is_archived = true;
    db.upsert_message(&m2).unwrap();

    // Both messages archived → thread should not appear in inbox
    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, false, true, true)
        .unwrap();
    assert_eq!(total, 0);
    assert!(rows.is_empty());
}

#[test]
fn dedup_all_view_shows_latest_regardless_of_archive() {
    let db = test_db();
    let conv = make_conversation("c1", "test-gmail", "C123");
    db.upsert_conversation(&conv).unwrap();

    let mut m1 = make_message_with_context(
        "m1",
        "c1",
        "test-gmail",
        "unarchived",
        1_000,
        Some("thread-1"),
    );
    m1.is_archived = false;
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message_with_context(
        "m2",
        "c1",
        "test-gmail",
        "archived newer",
        2_000,
        Some("thread-1"),
    );
    m2.is_archived = true;
    db.upsert_message(&m2).unwrap();

    // All view (include_archived=true, dedup=true): should show m2 (globally latest)
    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, true, true, true)
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].id, "m2", "--all view picks globally latest message");
}

#[test]
fn dedup_inbox_multiple_threads_mixed_archive_state() {
    let db = test_db();
    let conv = make_conversation("c1", "test-gmail", "C123");
    db.upsert_conversation(&conv).unwrap();

    // Thread A: latest is archived, older is unarchived
    let mut m1 = make_message_with_context(
        "m1",
        "c1",
        "test-gmail",
        "thread-A old",
        1_000,
        Some("thread-A"),
    );
    m1.is_archived = false;
    db.upsert_message(&m1).unwrap();

    let mut m2 = make_message_with_context(
        "m2",
        "c1",
        "test-gmail",
        "thread-A new",
        2_000,
        Some("thread-A"),
    );
    m2.is_archived = true;
    db.upsert_message(&m2).unwrap();

    // Thread B: latest is archived, older is unarchived
    let mut m3 = make_message_with_context(
        "m3",
        "c1",
        "test-gmail",
        "thread-B old",
        3_000,
        Some("thread-B"),
    );
    m3.is_archived = false;
    db.upsert_message(&m3).unwrap();

    let mut m4 = make_message_with_context(
        "m4",
        "c1",
        "test-gmail",
        "thread-B new",
        4_000,
        Some("thread-B"),
    );
    m4.is_archived = true;
    db.upsert_message(&m4).unwrap();

    // Thread C: all unarchived (normal case)
    let m5 = make_message_with_context(
        "m5",
        "c1",
        "test-gmail",
        "thread-C old",
        5_000,
        Some("thread-C"),
    );
    db.upsert_message(&m5).unwrap();

    let m6 = make_message_with_context(
        "m6",
        "c1",
        "test-gmail",
        "thread-C new",
        6_000,
        Some("thread-C"),
    );
    db.upsert_message(&m6).unwrap();

    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, false, true, true)
        .unwrap();
    assert_eq!(total, 3, "all 3 threads should appear");
    assert_eq!(rows.len(), 3);

    let ids: Vec<&str> = rows.iter().map(|m| m.id.as_str()).collect();
    assert!(
        ids.contains(&"m1"),
        "thread-A represented by latest unarchived"
    );
    assert!(
        ids.contains(&"m3"),
        "thread-B represented by latest unarchived"
    );
    assert!(
        ids.contains(&"m6"),
        "thread-C represented by latest overall (all unarchived)"
    );
}

#[test]
fn dedup_inbox_count_matches_rows() {
    let db = test_db();
    let conv = make_conversation("c1", "test-gmail", "C123");
    db.upsert_conversation(&conv).unwrap();

    // Create a thread where the latest is archived
    let mut m1 =
        make_message_with_context("m1", "c1", "test-gmail", "visible", 1_000, Some("thread-1"));
    m1.is_archived = false;
    db.upsert_message(&m1).unwrap();

    let mut m2 =
        make_message_with_context("m2", "c1", "test-gmail", "hidden", 2_000, Some("thread-1"));
    m2.is_archived = true;
    db.upsert_message(&m2).unwrap();

    // Standalone unarchived message (no context)
    let m3 = make_message_with_context("m3", "c1", "test-gmail", "standalone", 3_000, None);
    db.upsert_message(&m3).unwrap();

    let (rows, total) = db
        .recent_messages_paginated(None, None, 50, 0, false, true, true)
        .unwrap();
    assert_eq!(
        total as usize,
        rows.len(),
        "total count must match actual rows returned"
    );
    assert_eq!(total, 2);
}
