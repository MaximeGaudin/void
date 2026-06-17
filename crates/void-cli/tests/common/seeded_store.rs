//! Shared seeded on-disk store for read-path integration tests.

use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;
use void_core::db::Database;
use void_core::models::ConversationKind;
use void_core::test_fixtures::{make_conversation_named, make_message_with_sender};

/// An isolated store with a config file whose `store.path` points at the store
/// dir, plus a seeded `void.db`.
pub struct SeededStore {
    _dir: TempDir,
    store: String,
    config: String,
}

fn seed_db(db_path: &Path) {
    let db = Database::open(db_path).expect("open db for seeding");

    // A DM conversation (excluded from `channels`) and a channel conversation.
    let dm = make_conversation_named(
        "c-dm",
        "C-DM-EXT",
        "Direct With Alice",
        ConversationKind::Dm,
    );
    let channel = make_conversation_named(
        "c-chan",
        "C-CHAN-EXT",
        "general-announcements",
        ConversationKind::Channel,
    );
    db.upsert_conversation(&dm).expect("upsert dm");
    db.upsert_conversation(&channel).expect("upsert channel");

    // Messages. `sender != connection_id` so they surface as contacts too.
    let mut m1 = make_message_with_sender(
        "m1",
        "c-dm",
        "alice@example.com",
        "ZEBRAFISH lunch plans",
        1_700_000_100,
    );
    m1.synced_at = Some(1_700_000_110);
    db.upsert_message(&m1).expect("upsert m1");
    let mut m2 = make_message_with_sender(
        "m2",
        "c-chan",
        "bob@example.com",
        "QUOKKA deploy is live",
        1_700_000_200,
    );
    m2.synced_at = Some(1_700_000_210);
    db.upsert_message(&m2).expect("upsert m2");
}

impl SeededStore {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store_dir = dir.path().join("store");
        std::fs::create_dir_all(&store_dir).expect("create store dir");
        let store = store_dir.to_string_lossy().into_owned();
        let config = dir
            .path()
            .join("config.toml")
            .to_string_lossy()
            .into_owned();

        // Config in local mode with store.path pinned to our tempdir so any
        // code path that reloads the config (e.g. doctor) stays isolated.
        // Escape backslashes so a Windows path is a valid TOML basic string
        // (the unescaped `store` is still what we pass to `--store`).
        let store_toml = store.replace('\\', "\\\\");
        let config_contents = format!("[store]\nmode = \"local\"\npath = \"{store_toml}\"\n");
        std::fs::write(&config, config_contents).expect("write config");

        seed_db(&store_dir.join("void.db"));

        Self {
            _dir: dir,
            store,
            config,
        }
    }

    pub fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("void").expect("void binary");
        c.arg("--store")
            .arg(&self.store)
            .arg("--config")
            .arg(&self.config);
        c
    }
}
