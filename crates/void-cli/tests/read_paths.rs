//! Read-path smoke tests against a real, seeded on-disk store.
//!
//! We seed `<store>/void.db` using void-core's public `Database::open` +
//! `upsert_conversation` / `upsert_message`, then run the read commands and
//! assert exit 0 and that seeded content appears in stdout (JSON output).

mod common;

use common::SeededStore;
use predicates::prelude::*;

#[test]
fn inbox_shows_seeded_messages() {
    let sb = SeededStore::new();
    sb.cmd()
        .arg("inbox")
        .assert()
        .success()
        .stdout(predicate::str::contains("ZEBRAFISH"))
        .stdout(predicate::str::contains("QUOKKA"));
}

#[test]
fn search_finds_seeded_message() {
    let sb = SeededStore::new();
    sb.cmd()
        .args(["search", "ZEBRAFISH"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ZEBRAFISH"))
        .stdout(predicate::str::contains("QUOKKA").not());
}

#[test]
fn conversations_lists_seeded_conversations() {
    let sb = SeededStore::new();
    sb.cmd()
        .arg("conversations")
        .assert()
        .success()
        .stdout(predicate::str::contains("Direct With Alice"))
        .stdout(predicate::str::contains("general-announcements"));
}

#[test]
fn messages_shows_messages_for_conversation() {
    let sb = SeededStore::new();
    sb.cmd()
        .args(["messages", "c-dm"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ZEBRAFISH"));
}

#[test]
fn contacts_lists_seeded_senders() {
    let sb = SeededStore::new();
    sb.cmd()
        .arg("contacts")
        .assert()
        .success()
        .stdout(predicate::str::contains("alice@example.com"));
}

#[test]
fn channels_lists_only_channel_conversations() {
    let sb = SeededStore::new();
    // `channels` excludes DMs (kind = dm), includes group/channel.
    sb.cmd()
        .arg("channels")
        .assert()
        .success()
        .stdout(predicate::str::contains("general-announcements"))
        .stdout(predicate::str::contains("Direct With Alice").not());
}
