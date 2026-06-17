//! JSON layout snapshots for read-path commands.
//!
//! Complements the substring smoke tests in `read_paths.rs` by asserting the
//! full parsed JSON envelope (keys, pagination, field layout).

mod common;

use common::SeededStore;

fn command_json(store: &SeededStore, args: &[&str]) -> serde_json::Value {
    let output = store
        .cmd()
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("run {}: {e}", args.join(" ")));
    assert!(
        output.status.success(),
        "{} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("parse {} json: {e}\nstdout:\n{stdout}", args.join(" ")))
}

#[test]
fn inbox_json_layout_snapshot() {
    let store = SeededStore::new();
    let json = command_json(&store, &["inbox"]);
    insta::assert_json_snapshot!("inbox", json);
}

#[test]
fn conversations_json_layout_snapshot() {
    let store = SeededStore::new();
    let json = command_json(&store, &["conversations"]);
    insta::assert_json_snapshot!("conversations", json);
}
