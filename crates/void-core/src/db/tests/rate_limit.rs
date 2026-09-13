use std::time::Duration;

use super::fixtures::*;

#[test]
fn take_rate_token_waits_when_empty() {
    let db = test_db();
    assert!(db.take_rate_token("g1", "k", 1.0, 0.01).unwrap().is_zero());
    let wait = db.take_rate_token("g1", "k", 1.0, 0.01).unwrap();
    assert!(wait > Duration::from_secs(1));
}

#[test]
fn take_rate_token_refills_from_stale_timestamp() {
    let db = test_db();
    assert!(db.take_rate_token("g1", "k", 1.0, 1.0).unwrap().is_zero());
    let past = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
        - 10.0;
    db.set_sync_state("g1", "k", &format!(r#"{{"tokens":0.0,"updated":{past}}}"#))
        .unwrap();
    assert!(db.take_rate_token("g1", "k", 1.0, 1.0).unwrap().is_zero());
}

#[test]
fn take_rate_token_is_per_connection() {
    let db = test_db();
    assert!(db.take_rate_token("g1", "k", 1.0, 0.01).unwrap().is_zero());
    assert!(db.take_rate_token("g2", "k", 1.0, 0.01).unwrap().is_zero());
}

#[test]
fn take_rate_token_is_shared_across_db_handles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("void.db");
    let db1 = crate::db::Database::open(&path).unwrap();
    let db2 = crate::db::Database::open(&path).unwrap();
    assert!(db1.take_rate_token("g", "k", 1.0, 0.01).unwrap().is_zero());
    let wait = db2.take_rate_token("g", "k", 1.0, 0.01).unwrap();
    assert!(wait > Duration::from_secs(1));
}

#[test]
fn take_rate_token_corrupt_json_fails_open_as_full_bucket() {
    let db = test_db();
    db.set_sync_state("g1", "k", "not-json").unwrap();
    assert!(db.take_rate_token("g1", "k", 2.0, 1.0).unwrap().is_zero());
    assert!(db.take_rate_token("g1", "k", 2.0, 1.0).unwrap().is_zero());
    let wait = db.take_rate_token("g1", "k", 2.0, 1.0).unwrap();
    assert!(wait > Duration::ZERO);
}
