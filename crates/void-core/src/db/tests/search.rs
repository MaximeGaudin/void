use super::fixtures::*;

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
fn search_without_dedup_returns_all_matching_context_messages() {
    let db = test_db();
    let conv = make_conversation("c1", "test-slack", "C123");
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "trop bien old",
        1_000,
        Some("ctx-Y"),
    ))
    .unwrap();
    db.upsert_message(&make_message_with_context(
        "m2",
        "c1",
        "test-slack",
        "reply without phrase",
        2_000,
        Some("ctx-Y"),
    ))
    .unwrap();

    let (rows, total) = db
        .search_messages_paginated("trop bien", None, None, 50, 0, true, false)
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "m1");
}

#[test]
fn search_matches_conversation_name() {
    let db = test_db();
    let mut conv = make_conversation("c1", "test-slack", "C123");
    conv.name = Some("Aubin Rioufol".into());
    db.upsert_conversation(&conv).unwrap();

    db.upsert_message(&make_message_with_context(
        "m1",
        "c1",
        "test-slack",
        "see you soon",
        1_000,
        None,
    ))
    .unwrap();

    let (rows, total) = db
        .search_messages_paginated("Aubin", None, None, 50, 0, true, false)
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(rows[0].id, "m1");
}

// ---- Area C: FTS5 search robustness (property test) ----

proptest::proptest! {
    /// For ARBITRARY user input — including FTS5 operators, quotes, unicode and
    /// control bytes — search_messages must never surface an FTS5 syntax error
    /// or panic. It must return Ok (possibly empty). Guards query-injection crashes.
    #[test]
    fn search_never_errors_on_arbitrary_input(
        query in proptest::string::string_regex(
            r#"[a-zA-Z0-9 @:"*(){}\^\-+~/\\.café会議📄NEARANDORNOT]{0,40}"#
        ).unwrap()
    ) {
        let db = seed_search_db();
        let result = db.search_messages(&query, None, None, 50, true);
        proptest::prop_assert!(
            result.is_ok(),
            "search returned an error for input {:?}: {:?}",
            query,
            result.err()
        );
    }

    /// Same guarantee with both connection and connector filters engaged, since
    /// the filter clauses extend the SQL and could interact with the MATCH escaping.
    #[test]
    fn search_never_errors_with_filters(
        query in proptest::string::string_regex(
            r#"[a-zA-Z0-9 @:"*()\-+]{0,30}"#
        ).unwrap()
    ) {
        let db = seed_search_db();
        let result = db.search_messages(&query, Some("test-slack"), Some("slack"), 50, false);
        proptest::prop_assert!(
            result.is_ok(),
            "filtered search errored for input {:?}: {:?}",
            query,
            result.err()
        );
    }
}
