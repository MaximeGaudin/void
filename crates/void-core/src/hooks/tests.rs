use std::str::FromStr;

use crate::hooks::execute::extract_error_from_stream;
use crate::hooks::hook_fs::{
    delete_hook, find_hook, load_hooks, save_hook, slugify, update_hook_enabled,
};
use crate::hooks::model::{ActiveWindow, Hook, PromptConfig, Trigger, Weekday};
use crate::hooks::placeholders::expand_placeholders;
use crate::models::Message;

#[test]
fn load_hooks_returns_empty_for_nonexistent_dir() {
    let dir = std::env::temp_dir().join(format!("void-hooks-nonexistent-{}", uuid::Uuid::new_v4()));
    assert!(!dir.exists(), "dir should not exist");
    let hooks = load_hooks(&dir);
    assert!(hooks.is_empty());
}

#[test]
fn slugify_basic() {
    assert_eq!(slugify("Gmail Auto-Archive"), "gmail-auto-archive");
    assert_eq!(slugify("  Daily  Digest  "), "daily-digest");
    assert_eq!(slugify("foo_bar__baz"), "foo-bar-baz");
}

#[test]
fn hook_roundtrip() {
    let hook = Hook {
        name: "Test Hook".into(),
        enabled: true,
        max_turns: 5,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage {
            connector: Some("gmail".into()),
        },
        prompt: PromptConfig {
            text: "Hello {message_id}".into(),
        },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    assert_eq!(parsed.name, "Test Hook");
    assert_eq!(parsed.max_turns, 5);
    assert!(
        matches!(parsed.trigger, Trigger::NewMessage { connector: Some(ref c) } if c == "gmail")
    );
    assert!(parsed.extra_args.is_empty());
}

#[test]
fn schedule_hook_roundtrip() {
    let hook = Hook {
        name: "Daily Digest".into(),
        enabled: true,
        max_turns: 10,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::Schedule {
            cron: "0 9 * * 1-5".into(),
        },
        prompt: PromptConfig {
            text: "Run digest for {today}".into(),
        },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    assert!(matches!(parsed.trigger, Trigger::Schedule { ref cron } if cron == "0 9 * * 1-5"));
}

#[test]
fn hook_extra_args_roundtrip() {
    let hook = Hook {
        name: "WithArgs".into(),
        enabled: true,
        max_turns: 3,
        agent: "claude".into(),
        extra_args: vec![
            "--model".into(),
            "sonnet".into(),
            "--dangerously-skip-permissions".into(),
        ],
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    assert!(
        toml_str.contains("extra_args"),
        "extra_args must be present:\n{toml_str}"
    );
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    assert_eq!(
        parsed.extra_args,
        vec![
            "--model".to_string(),
            "sonnet".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ]
    );
}

#[test]
fn extract_error_from_stream_rate_limit_result() {
    let stream = r#"{"type":"system","subtype":"init"}
{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour"}}
{"type":"result","subtype":"success","is_error":true,"api_error_status":429,"result":"You've hit your limit · resets 6:20pm","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour"}}
"#;
    let err = extract_error_from_stream(stream).expect("should extract error");
    assert!(err.contains("HTTP 429"), "missing status tag: {err}");
    assert!(
        err.contains("rate_limit=five_hour"),
        "missing rate_limit tag: {err}"
    );
    assert!(err.contains("resets 6:20pm"), "missing body: {err}");
}

#[test]
fn extract_error_from_stream_no_error() {
    let stream = r#"{"type":"system","subtype":"init"}
{"type":"result","subtype":"success","is_error":false,"result":"all good"}
"#;
    assert!(extract_error_from_stream(stream).is_none());
}

#[test]
fn extract_error_from_stream_rate_limit_event_fallback() {
    let stream = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour"}}
"#;
    let err = extract_error_from_stream(stream).expect("should extract fallback");
    assert!(err.contains("rate limited"), "missing prefix: {err}");
    assert!(err.contains("five_hour"), "missing type: {err}");
}

#[test]
fn extract_error_from_stream_empty() {
    assert!(extract_error_from_stream("").is_none());
    assert!(extract_error_from_stream("not json\n").is_none());
}

#[test]
fn hook_extra_args_omitted_when_empty() {
    let hook = Hook {
        name: "Default".into(),
        enabled: true,
        max_turns: 1,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    assert!(
        !toml_str.contains("extra_args"),
        "expected extra_args to be omitted when empty, got:\n{toml_str}"
    );
}

#[test]
fn expand_placeholders_no_message() {
    let result = expand_placeholders("Today is {today}, now is {now}", None);
    assert!(!result.contains("{today}"));
    assert!(!result.contains("{now}"));
}

#[test]
fn expand_placeholders_keeps_message_tokens_when_no_message() {
    let result = expand_placeholders(
        "before {message_id} after {connector} {connection_id}",
        None,
    );
    assert_eq!(
        result, "before {message_id} after {connector} {connection_id}",
        "message placeholders must remain literal when no Message is supplied"
    );
}

#[test]
fn expand_placeholders_with_message() {
    let msg = Message {
        id: "msg-123".into(),
        conversation_id: "c1".into(),
        connection_id: "acc1".into(),
        connector: "gmail".into(),
        external_id: "ext1".into(),
        sender: "alice@example.com".into(),
        sender_name: None,
        sender_avatar_url: None,
        body: Some("Hello".into()),
        timestamp: 1_700_000_000,
        synced_at: None,
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: None,
        context_id: None,
        context: None,
    };
    let result = expand_placeholders("ID={message_id} CONN={connector}", Some(&msg));
    assert_eq!(result, "ID=msg-123 CONN=gmail");
}

#[test]
fn save_and_load_hook() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "My Test Hook".into(),
        enabled: true,
        max_turns: 3,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "test".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    let loaded = load_hooks(&dir);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "My Test Hook");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn delete_hook_works() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "To Delete".into(),
        enabled: true,
        max_turns: 3,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "test".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    assert!(delete_hook(&dir, "To Delete").unwrap());
    assert!(!delete_hook(&dir, "To Delete").unwrap());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn find_hook_works() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let hook = Hook {
        name: "Find Me".into(),
        enabled: true,
        max_turns: 2,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig {
            text: "prompt".into(),
        },
    };
    save_hook(&dir, &hook).unwrap();
    let found = find_hook(&dir, "Find Me").expect("hook should exist");
    assert_eq!(found.name, "Find Me");
    assert_eq!(found.max_turns, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn active_window_roundtrip() {
    let hook = Hook {
        name: "Windowed Hook".into(),
        enabled: true,
        max_turns: 3,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: Some(ActiveWindow {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
            ],
            start: "08:00".into(),
            end: "21:00".into(),
            utc_offset_hours: Some(2),
        }),
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    assert!(
        toml_str.contains("[active_window]"),
        "expected active_window section:\n{toml_str}"
    );
    assert!(
        toml_str.contains("08:00"),
        "expected start time:\n{toml_str}"
    );
    let parsed: Hook = toml::from_str(&toml_str).unwrap();
    let window = parsed
        .active_window
        .expect("active_window should be present");
    assert_eq!(window.days.len(), 5);
    assert_eq!(window.start, "08:00");
    assert_eq!(window.end, "21:00");
    assert_eq!(window.utc_offset_hours, Some(2));
}

#[test]
fn active_window_omitted_when_none() {
    let hook = Hook {
        name: "NoWindow".into(),
        enabled: true,
        max_turns: 1,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    let toml_str = toml::to_string_pretty(&hook).unwrap();
    assert!(
        !toml_str.contains("active_window"),
        "expected active_window to be omitted:\n{toml_str}"
    );
}

#[test]
fn active_window_is_active_checks_time_range() {
    let window = ActiveWindow {
        days: vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ],
        start: "00:00".into(),
        end: "23:59".into(),
        utc_offset_hours: None,
    };
    assert!(window.is_active_now(), "all-day window should be active");

    let window_never = ActiveWindow {
        days: vec![],
        start: "00:00".into(),
        end: "23:59".into(),
        utc_offset_hours: None,
    };
    assert!(
        !window_never.is_active_now(),
        "no-days window should never be active"
    );
}

#[test]
fn weekday_from_str_variants() {
    assert_eq!(Weekday::parse("mon"), Some(Weekday::Mon));
    assert_eq!(Weekday::parse("Monday"), Some(Weekday::Mon));
    assert_eq!(Weekday::parse("FRI"), Some(Weekday::Fri));
    assert_eq!(Weekday::parse("invalid"), None);
}

#[test]
fn update_hook_enabled_toggles() {
    let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
    let hook = Hook {
        name: "Toggle Test".into(),
        enabled: true,
        max_turns: 1,
        agent: "claude".into(),
        extra_args: Vec::new(),
        active_window: None,
        trigger: Trigger::NewMessage { connector: None },
        prompt: PromptConfig { text: "x".into() },
    };
    save_hook(&dir, &hook).unwrap();
    assert!(update_hook_enabled(&dir, "Toggle Test", false).unwrap());
    let loaded = find_hook(&dir, "Toggle Test").unwrap();
    assert!(!loaded.enabled);
    assert!(update_hook_enabled(&dir, "Toggle Test", true).unwrap());
    let loaded = find_hook(&dir, "Toggle Test").unwrap();
    assert!(loaded.enabled);
    assert!(!update_hook_enabled(&dir, "Nonexistent", true).unwrap());
    std::fs::remove_dir_all(&dir).ok();
}

// ---- Area D(a): new_message trigger connector-filter matching ----

/// Mirror of the predicate `HookRunner::on_new_message` uses to decide whether a
/// `NewMessage` hook fires for a given message connector. A `None` connector
/// filter matches everything; `Some(x)` matches only when the message connector
/// equals `x`.
fn new_message_hook_fires(trigger: &Trigger, msg_connector: &str) -> bool {
    matches!(trigger, Trigger::NewMessage { connector }
        if connector.is_none() || connector.as_deref() == Some(msg_connector))
}

#[test]
fn trigger_no_connector_filter_matches_any_connector() {
    let t = Trigger::NewMessage { connector: None };
    assert!(new_message_hook_fires(&t, "gmail"));
    assert!(new_message_hook_fires(&t, "slack"));
    assert!(new_message_hook_fires(&t, "whatsapp"));
}

#[test]
fn trigger_connector_filter_matches_same_connector() {
    let t = Trigger::NewMessage {
        connector: Some("gmail".into()),
    };
    assert!(new_message_hook_fires(&t, "gmail"));
}

#[test]
fn trigger_connector_filter_rejects_other_connector() {
    let t = Trigger::NewMessage {
        connector: Some("gmail".into()),
    };
    assert!(!new_message_hook_fires(&t, "slack"));
    assert!(!new_message_hook_fires(&t, "whatsapp"));
}

#[test]
fn trigger_schedule_never_matches_new_message() {
    let t = Trigger::Schedule {
        cron: "0 9 * * *".into(),
    };
    assert!(!new_message_hook_fires(&t, "gmail"));
}

#[test]
fn trigger_new_message_connector_deserializes_from_toml() {
    // type-tagged enum: `type = "new_message"` with an optional `connector`.
    let with: Trigger = toml::from_str(
        r#"
        type = "new_message"
        connector = "slack"
        "#,
    )
    .unwrap();
    assert!(matches!(with, Trigger::NewMessage { connector: Some(ref c) } if c == "slack"));

    let without: Trigger = toml::from_str(r#"type = "new_message""#).unwrap();
    assert!(matches!(without, Trigger::NewMessage { connector: None }));
}

// ---- Area D(b): cron scheduling (croner) ----

#[test]
fn cron_daily_9am_next_occurrence_from_fixed_now() {
    use chrono::TimeZone;

    // Same construction the scheduler uses: croner::Cron::from_str(expr).
    let cron = croner::Cron::from_str("0 9 * * *").unwrap();

    // Fixed `now`: 2026-06-11 08:00:00 UTC (no Utc::now()).
    let now = chrono::Utc.with_ymd_and_hms(2026, 6, 11, 8, 0, 0).unwrap();
    let next = cron.find_next_occurrence(&now, false).unwrap();

    let expected = chrono::Utc.with_ymd_and_hms(2026, 6, 11, 9, 0, 0).unwrap();
    assert_eq!(next, expected, "next 9am is later the same day");
}

#[test]
fn cron_daily_9am_rolls_to_next_day_when_past() {
    use chrono::TimeZone;

    let cron = croner::Cron::from_str("0 9 * * *").unwrap();
    // now is 10:00, already past today's 9am → next is tomorrow 9am.
    let now = chrono::Utc.with_ymd_and_hms(2026, 6, 11, 10, 0, 0).unwrap();
    let next = cron.find_next_occurrence(&now, false).unwrap();

    let expected = chrono::Utc.with_ymd_and_hms(2026, 6, 12, 9, 0, 0).unwrap();
    assert_eq!(next, expected);
}

#[test]
fn cron_weekday_only_skips_weekend() {
    use chrono::{Datelike, TimeZone, Weekday as CWeekday};

    // Weekdays 1-5 (Mon-Fri) at 09:00. 2026-06-12 is a Friday; from Fri 10:00
    // the next occurrence must be Monday 2026-06-15 09:00 (skips Sat/Sun).
    let cron = croner::Cron::from_str("0 9 * * 1-5").unwrap();
    let friday_now = chrono::Utc.with_ymd_and_hms(2026, 6, 12, 10, 0, 0).unwrap();
    assert_eq!(friday_now.weekday(), CWeekday::Fri);

    let next = cron.find_next_occurrence(&friday_now, false).unwrap();
    assert_eq!(next.weekday(), CWeekday::Mon);
    assert_eq!(
        next,
        chrono::Utc.with_ymd_and_hms(2026, 6, 15, 9, 0, 0).unwrap()
    );
}

#[test]
fn cron_invalid_expression_is_error_not_panic() {
    let result = croner::Cron::from_str("not a cron");
    assert!(result.is_err(), "garbage cron must yield Err, not panic");
}

// ---- Area D(c): execute_hook_blocking against stub agents (unix only) ----

#[cfg(unix)]
mod stub_agent {
    use crate::hooks::execute::{execute_hook_blocking, HookExecOptions};

    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Write an executable shell script into `dir` and return its path.
    fn write_stub(dir: &std::path::Path, name: &str, script: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(script.as_bytes()).unwrap();
        f.flush().unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn stub_success_extracts_result_summary() {
        let dir = tempfile::tempdir().unwrap();
        // Emit a Claude-style stream-json `result` line on stdout, exit 0.
        let stub = write_stub(
            dir.path(),
            "agent-ok.sh",
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"result\",\"result\":\"all done summary\",\"is_error\":false}'\nexit 0\n",
        );

        let out = execute_hook_blocking(
            stub.to_str().unwrap(),
            "do the thing",
            3,
            &HookExecOptions::default(),
        )
        .unwrap();

        assert!(out.success, "exit 0 with clean result → success");
        assert_eq!(out.result_summary, "all done summary");
        assert!(out.error.is_none());
        assert_eq!(out.input_prompt, "do the thing");
    }

    #[test]
    fn stub_nonzero_exit_surfaces_error_from_stream() {
        let dir = tempfile::tempdir().unwrap();
        // Non-zero exit AND a stream-json error result with an api_error_status.
        let stub = write_stub(
            dir.path(),
            "agent-fail.sh",
            "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"result\",\"result\":\"rate limited\",\"is_error\":true,\"api_error_status\":429}'\nexit 1\n",
        );

        let out =
            execute_hook_blocking(stub.to_str().unwrap(), "p", 3, &HookExecOptions::default())
                .unwrap();

        assert!(!out.success, "exit 1 → failure");
        let err = out.error.expect("error should be surfaced");
        // extract_error_from_stream prefixes HTTP status tags and includes the body.
        assert!(
            err.contains("HTTP 429"),
            "error should include HTTP status: {err}"
        );
        assert!(
            err.contains("rate limited"),
            "error should include result body: {err}"
        );
        assert!(out.result_summary.is_empty());
    }

    #[test]
    fn stub_nonzero_exit_with_stderr_only_falls_back_to_stderr() {
        let dir = tempfile::tempdir().unwrap();
        // No structured stdout; just a stderr message and non-zero exit.
        let stub = write_stub(
            dir.path(),
            "agent-stderr.sh",
            "#!/bin/sh\necho 'boom on stderr' 1>&2\nexit 2\n",
        );

        let out =
            execute_hook_blocking(stub.to_str().unwrap(), "p", 1, &HookExecOptions::default())
                .unwrap();

        assert!(!out.success);
        let err = out.error.expect("error surfaced");
        assert!(
            err.contains("boom on stderr"),
            "stderr fallback used: {err}"
        );
    }

    #[test]
    fn stub_receives_framework_flags_in_argv() {
        let dir = tempfile::tempdir().unwrap();
        // Echo our own argv as a JSON result so we can assert the framework flags.
        // The script writes the joined args into the result field.
        let stub = write_stub(
            dir.path(),
            "agent-argv.sh",
            "#!/bin/sh\nargs=\"$*\"\nprintf '{\"type\":\"result\",\"result\":\"%s\",\"is_error\":false}\\n' \"$args\"\nexit 0\n",
        );

        let out = execute_hook_blocking(
            stub.to_str().unwrap(),
            "PROMPTBODY",
            7,
            &HookExecOptions {
                extra_args: vec!["--model".into(), "sonnet".into()],
            },
        )
        .unwrap();

        assert!(out.success);
        let argv = out.result_summary;
        // Framework-managed flags, then the extra args appended verbatim.
        assert!(argv.contains("-p PROMPTBODY"), "prompt flag: {argv}");
        assert!(argv.contains("--verbose"), "verbose flag: {argv}");
        assert!(
            argv.contains("--output-format stream-json"),
            "output-format: {argv}"
        );
        assert!(argv.contains("--max-turns 7"), "max-turns: {argv}");
        assert!(
            argv.contains("--model sonnet"),
            "extra args appended: {argv}"
        );
    }
}
