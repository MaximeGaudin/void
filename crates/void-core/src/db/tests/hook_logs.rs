use super::fixtures::*;
use crate::hooks::HookLogInsert;

fn log(db: &crate::db::Database, name: &str, started_at: i64, success: bool) {
    db.insert_hook_log(&HookLogInsert {
        hook_name: name,
        trigger_type: "new_message",
        started_at,
        duration_ms: 10,
        success,
        result: None,
        error: None,
        message_id: None,
        input_prompt: None,
        raw_output: None,
    })
    .unwrap();
}

#[test]
fn hook_last_runs_reports_latest_entry_per_hook() {
    let db = test_db();
    assert!(db.hook_last_runs().unwrap().is_empty());

    log(&db, "slack-triage", 100, true);
    log(&db, "slack-triage", 200, false);
    log(&db, "digest", 150, true);

    let runs = db.hook_last_runs().unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].hook_name, "digest");
    assert_eq!(runs[0].runs, 1);
    assert_eq!(runs[1].hook_name, "slack-triage");
    assert_eq!(runs[1].last_started_at, 200);
    assert!(!runs[1].last_success);
    assert_eq!(runs[1].runs, 2);
}

#[test]
fn count_messages_synced_since_filters_by_connector_and_time() {
    let db = test_db();
    db.upsert_conversation(&make_conversation_with_connector(
        "c1", "acct", "e1", "slack",
    ))
    .unwrap();
    let mut old = make_message_with_connector("m1", "c1", "acct", "old", 1, "slack");
    old.synced_at = Some(1_000);
    let mut new = make_message_with_connector("m2", "c1", "acct", "new", 2, "slack");
    new.synced_at = Some(2_000);
    let mut mail = make_message_with_connector("m3", "c1", "acct", "mail", 3, "gmail");
    mail.external_id = "ext-m3".into();
    mail.synced_at = Some(2_000);
    for m in [&old, &new, &mail] {
        db.upsert_message(m).unwrap();
    }

    assert_eq!(
        db.count_messages_synced_since(Some("slack"), 1_500)
            .unwrap(),
        1
    );
    assert_eq!(db.count_messages_synced_since(None, 1_500).unwrap(), 2);
    assert_eq!(db.count_messages_synced_since(Some("slack"), 0).unwrap(), 2);
    assert_eq!(
        db.count_messages_synced_since(Some("slack"), 2_000)
            .unwrap(),
        0
    );
}
