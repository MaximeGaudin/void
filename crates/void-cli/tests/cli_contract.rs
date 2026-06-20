//! CLI contract tests: assert the command surface is stable.
//!
//! - `<cmd> --help` exits 0 and prints usage for every top-level command.
//! - `--help` / `--version` exit 0.
//! - Required-arg violations exit non-zero with a useful message.
//!
//! These tests never touch the real store/config: required-arg violations are
//! caught by clap before any IO, and the few that reach runtime are given an
//! isolated `--store`/`--config` (see helpers below).

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn void() -> Command {
    Command::cargo_bin("void").expect("void binary should be built")
}

/// Isolated store dir + config path so runtime-level argument checks never
/// read or write real user data.
struct Sandbox {
    _dir: TempDir,
    store: String,
    config: String,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = dir.path().join("store").to_string_lossy().into_owned();
        let config = dir
            .path()
            .join("config.toml")
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&store).expect("create store dir");
        Self {
            _dir: dir,
            store,
            config,
        }
    }

    fn cmd(&self) -> Command {
        let mut c = void();
        c.arg("--store")
            .arg(&self.store)
            .arg("--config")
            .arg(&self.config);
        c
    }
}

const TOP_LEVEL_COMMANDS: &[&str] = &[
    "inbox",
    "search",
    "conversations",
    "messages",
    "contacts",
    "channels",
    "calendar",
    "send",
    "reply",
    "forward",
    "archive",
    "mute",
    "gmail",
    "slack",
    "whatsapp",
    "telegram",
    "linkedin",
    "hn",
    "hook",
    "sync",
    "doctor",
    "remote",
    "mcp",
    "setup",
];

#[test]
fn every_command_help_exits_zero_and_prints_usage() {
    for cmd in TOP_LEVEL_COMMANDS {
        void()
            .args([cmd, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage:"));
    }
}

#[test]
fn top_level_help_exits_zero() {
    void()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage:"))
        .stdout(predicate::str::contains("communication CLI"));
}

#[test]
fn version_exits_zero() {
    void()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("void"));
}

// --- Required-arg violations ---

#[test]
fn send_without_recipient_fails_with_message() {
    void()
        .arg("send")
        .arg("--via")
        .arg("whatsapp")
        .arg("--message")
        .arg("hi")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"))
        .stderr(predicate::str::contains("--to"))
        .stderr(predicate::str::contains("--conversation"));
}

#[test]
fn send_without_via_fails_with_message() {
    // clap-level failure: missing required --via/--message. No IO performed.
    void()
        .arg("send")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required arguments"))
        .stderr(predicate::str::contains("--via"));
}

#[test]
fn reply_without_args_fails_with_message() {
    // clap-level failure: missing positional <MESSAGE_ID> and required --message.
    void()
        .arg("reply")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required arguments"))
        .stderr(predicate::str::contains("--message"));
}

#[test]
fn archive_without_ids_or_before_fails_with_message() {
    // Runtime-level failure (exit 1): needs an isolated store so the empty-db
    // check is reached without touching real data.
    let sb = Sandbox::new();
    sb.cmd()
        .arg("archive")
        .assert()
        .failure()
        .stderr(predicate::str::contains("message ID is required"))
        .stderr(predicate::str::contains("--before"));
}

#[test]
fn inbox_with_bogus_connector_fails_with_message() {
    let sb = Sandbox::new();
    sb.cmd()
        .args(["inbox", "--connector", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown connector"))
        .stderr(predicate::str::contains("bogus"));
}
