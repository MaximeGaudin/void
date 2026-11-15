//! Send guardrails for agents: a process carrying the hook marker
//! (`VOID_HOOK`, set by the hook executor) cannot send, reply or forward
//! unless `[hooks] allow_send = true`; Gmail drafts stay allowed.
//!
//! Every case fails before any connector is built, so no network is touched.

use assert_cmd::Command;
use tempfile::TempDir;

const GUARD_MSG: &str = "disabled for agents started by a hook";

struct Env {
    _dir: TempDir,
    store: std::path::PathBuf,
    config: std::path::PathBuf,
}

fn env_with_config(toml: &str) -> Env {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = dir.path().join("store");
    std::fs::create_dir_all(&store).expect("store dir");
    let config = dir.path().join("config.toml");
    let store_line = format!("[store]\npath = {:?}\n\n", store.to_string_lossy());
    std::fs::write(&config, format!("{store_line}{toml}")).expect("write config");
    Env {
        _dir: dir,
        store,
        config,
    }
}

fn run(env: &Env, hook: Option<&str>, args: &[&str]) -> (bool, String) {
    let mut cmd = Command::cargo_bin("void").expect("void binary");
    cmd.env_remove("VOID_HOOK").env_remove("VOID_MCP");
    if let Some(h) = hook {
        cmd.env("VOID_HOOK", h);
    }
    let out = cmd
        .arg("--store")
        .arg(&env.store)
        .arg("--config")
        .arg(&env.config)
        .args(args)
        .output()
        .expect("run void");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const SEND: &[&str] = &[
    "send",
    "--via",
    "gmail",
    "--to",
    "a@example.com",
    "--message",
    "hi",
];
const REPLY: &[&str] = &["reply", "some-message-id", "--message", "hi"];
const FORWARD: &[&str] = &["forward", "some-message-id", "--to", "a@example.com"];

#[test]
fn hook_marker_blocks_send_reply_forward() {
    let env = env_with_config("");
    for args in [SEND, REPLY, FORWARD] {
        let (ok, stderr) = run(&env, Some("email-triage"), args);
        assert!(!ok, "{args:?} must fail under a hook");
        assert!(stderr.contains(GUARD_MSG), "{args:?}: {stderr}");
        assert!(
            stderr.contains("void gmail draft create"),
            "{args:?} suggests drafts: {stderr}"
        );
        assert!(stderr.contains("email-triage"), "{args:?}: {stderr}");
    }
}

#[test]
fn hook_marker_blocks_connector_specific_forwards() {
    let env = env_with_config("");
    for args in [
        &["gmail", "forward", "id", "--to", "a@example.com"][..],
        &["slack", "forward", "id", "--to", "C123"][..],
        &["telegram", "forward", "id", "--to", "123"][..],
        &[
            "slack",
            "schedule",
            "--channel",
            "C123",
            "--message",
            "hi",
            "--at",
            "1",
        ][..],
    ] {
        let (ok, stderr) = run(&env, Some("h"), args);
        assert!(!ok, "{args:?} must fail under a hook");
        assert!(stderr.contains(GUARD_MSG), "{args:?}: {stderr}");
    }
}

#[test]
fn without_hook_marker_send_is_not_guarded() {
    let env = env_with_config("");
    let (ok, stderr) = run(&env, None, SEND);
    assert!(!ok, "no gmail connection configured");
    assert!(!stderr.contains(GUARD_MSG), "{stderr}");
}

#[test]
fn opt_in_lets_hook_agents_send() {
    let env = env_with_config("[hooks]\nallow_send = true\n");
    for args in [SEND, REPLY, FORWARD] {
        let (ok, stderr) = run(&env, Some("h"), args);
        // Fails later (no connection / unknown message), never on the guard.
        assert!(!ok);
        assert!(!stderr.contains(GUARD_MSG), "{args:?}: {stderr}");
    }
}

#[test]
fn allow_list_rejects_unknown_recipient() {
    let env =
        env_with_config("[hooks]\nallow_send = true\nallowed_recipients = [\"me@example.com\"]\n");
    let (ok, stderr) = run(
        &env,
        Some("h"),
        &["forward", "some-message-id", "--to", "attacker@evil.com"],
    );
    assert!(!ok);
    assert!(
        stderr.contains("not in [hooks] allowed_recipients"),
        "{stderr}"
    );

    // An allowed recipient passes the guard (then fails on the unknown message).
    let (ok, stderr) = run(
        &env,
        Some("h"),
        &["forward", "some-message-id", "--to", "me@example.com"],
    );
    assert!(!ok);
    assert!(!stderr.contains("allowed_recipients"), "{stderr}");
}

#[test]
fn gmail_draft_create_is_not_blocked_for_hooks() {
    let env = env_with_config("");
    let (_, stderr) = run(
        &env,
        Some("h"),
        &[
            "gmail",
            "draft",
            "create",
            "--to",
            "a@example.com",
            "--subject",
            "s",
            "--body",
            "b",
        ],
    );
    // No gmail connection is configured, so it fails, but not on the guard.
    assert!(!stderr.contains(GUARD_MSG), "{stderr}");
    assert!(!stderr.contains("allowed_recipients"), "{stderr}");
}
