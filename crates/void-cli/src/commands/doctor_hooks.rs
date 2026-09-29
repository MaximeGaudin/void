//! `void doctor` Hooks section: configured hooks, last execution, staleness.

use std::str::FromStr;

use void_core::db::Database;
use void_core::hooks::{Hook, HookRunSummary, Trigger};

/// An enabled hook is stale when it has not run for this long while its
/// trigger fired (new messages arrived, or a cron slot passed).
const STALE_AFTER_SECS: i64 = 24 * 3600;
/// Hooks restricted to an active window may legitimately skip days
/// (weekends, nights): be more lenient before flagging them.
const STALE_AFTER_WINDOWED_SECS: i64 = 72 * 3600;
/// Grace period after a missed cron slot before a schedule hook is stale.
const SCHEDULE_GRACE_SECS: i64 = 3600;
/// How far back to look for a cron slot when a schedule hook never ran.
const NEVER_RAN_LOOKBACK_SECS: i64 = 7 * 24 * 3600;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ok,
    Disabled,
    Stale(String),
}

/// Print the Hooks section. Returns the number of issues found.
///
/// `hooks` is `Err` when the hook list could not be read (e.g. remote SSH
/// failure). `db` is the store holding `hook_logs` (in remote mode, the
/// synced snapshot of the remote daemon's database).
pub(crate) fn report(
    hooks: Result<Vec<Hook>, String>,
    source: &str,
    db: Option<&Database>,
) -> usize {
    eprintln!();
    let hooks = match hooks {
        Ok(hooks) => hooks,
        Err(e) => {
            eprintln!("[!!] Hooks: cannot list hooks from {source}: {e}");
            return 1;
        }
    };
    if hooks.is_empty() {
        eprintln!("[--] Hooks: none configured in {source}");
        return 0;
    }

    let runs = db
        .and_then(|db| db.hook_last_runs().ok())
        .unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let enabled = hooks.iter().filter(|h| h.enabled).count();
    eprintln!(
        "Hooks: {} configured ({enabled} enabled) in {source}:",
        hooks.len()
    );

    let mut issues = 0;
    for hook in &hooks {
        let last = runs.iter().find(|r| r.hook_name == hook.name);
        let verdict = assess(hook, last, now, |connector, since| {
            db.and_then(|db| db.count_messages_synced_since(connector, since).ok())
                .unwrap_or(0)
        });
        let tag = match verdict {
            Verdict::Ok => "[OK]",
            Verdict::Disabled => "[--]",
            Verdict::Stale(_) => {
                issues += 1;
                "[!!]"
            }
        };
        eprintln!(
            "  {tag} {} ({}) — {}",
            hook.name,
            trigger_label(&hook.trigger),
            last_run_label(last)
        );
        match verdict {
            Verdict::Stale(reason) => eprintln!("       {reason}"),
            Verdict::Disabled => eprintln!("       disabled"),
            Verdict::Ok => {}
        }
    }
    issues
}

pub(crate) fn assess(
    hook: &Hook,
    last: Option<&HookRunSummary>,
    now: i64,
    messages_since: impl Fn(Option<&str>, i64) -> i64,
) -> Verdict {
    if !hook.enabled {
        return Verdict::Disabled;
    }
    let stale_after = if hook.active_window.is_some() {
        STALE_AFTER_WINDOWED_SECS
    } else {
        STALE_AFTER_SECS
    };
    let last_at = last.map(|r| r.last_started_at);

    match &hook.trigger {
        Trigger::NewMessage { connector } => {
            if last_at.is_some_and(|t| now - t < stale_after) {
                return Verdict::Ok;
            }
            let since = last_at.unwrap_or(now - stale_after);
            let pending = messages_since(connector.as_deref(), since);
            if pending == 0 {
                return Verdict::Ok;
            }
            let scope = connector.as_deref().unwrap_or("any connector");
            Verdict::Stale(match last_at {
                Some(t) => format!(
                    "no run for {}h while {pending} new {scope} message(s) arrived — is the sync daemon loading this hook?",
                    (now - t) / 3600
                ),
                None => format!(
                    "never ran while {pending} new {scope} message(s) arrived in the last {}h — is the sync daemon loading this hook?",
                    stale_after / 3600
                ),
            })
        }
        Trigger::Schedule { cron } => {
            let parsed = match croner::Cron::from_str(cron) {
                Ok(c) => c,
                Err(e) => return Verdict::Stale(format!("invalid cron expression '{cron}': {e}")),
            };
            let reference = last_at.unwrap_or(now - NEVER_RAN_LOOKBACK_SECS);
            let Some(reference) = chrono::DateTime::from_timestamp(reference, 0) else {
                return Verdict::Ok;
            };
            let Ok(expected) = parsed.find_next_occurrence(&reference, false) else {
                return Verdict::Ok;
            };
            let grace = if hook.active_window.is_some() {
                stale_after
            } else {
                SCHEDULE_GRACE_SECS
            };
            if expected.timestamp() + grace >= now {
                return Verdict::Ok;
            }
            Verdict::Stale(format!(
                "missed scheduled run due {} — is the sync daemon loading this hook?",
                expected.format("%Y-%m-%d %H:%M UTC")
            ))
        }
    }
}

fn trigger_label(trigger: &Trigger) -> String {
    match trigger {
        Trigger::NewMessage { connector: Some(c) } => format!("new_message:{c}"),
        Trigger::NewMessage { connector: None } => "new_message".into(),
        Trigger::Schedule { cron } => format!("schedule '{cron}'"),
    }
}

fn last_run_label(last: Option<&HookRunSummary>) -> String {
    let Some(run) = last else {
        return "never ran".into();
    };
    let when = chrono::DateTime::from_timestamp(run.last_started_at, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| run.last_started_at.to_string());
    let status = if run.last_success { "ok" } else { "failed" };
    format!("last run {when} ({status}, {} run(s) logged)", run.runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use void_core::hooks::{ActiveWindow, PromptConfig, Weekday};

    const NOW: i64 = 1_790_000_000;

    fn hook(trigger: Trigger) -> Hook {
        Hook {
            name: "slack-triage".into(),
            enabled: true,
            max_turns: 3,
            agent: "claude".into(),
            extra_args: Vec::new(),
            active_window: None,
            trigger,
            prompt: PromptConfig { text: "x".into() },
        }
    }

    fn slack() -> Trigger {
        Trigger::NewMessage {
            connector: Some("slack".into()),
        }
    }

    fn run_at(t: i64) -> HookRunSummary {
        HookRunSummary {
            hook_name: "slack-triage".into(),
            last_started_at: t,
            last_success: true,
            runs: 4,
        }
    }

    #[test]
    fn disabled_hook_is_not_assessed() {
        let mut h = hook(slack());
        h.enabled = false;
        assert_eq!(assess(&h, None, NOW, |_, _| 100), Verdict::Disabled);
    }

    #[test]
    fn message_hook_recent_run_is_ok() {
        let last = run_at(NOW - 3600);
        assert_eq!(
            assess(&hook(slack()), Some(&last), NOW, |_, _| 500),
            Verdict::Ok
        );
    }

    #[test]
    fn message_hook_stale_when_messages_pile_up() {
        let last = run_at(NOW - 12 * 24 * 3600);
        let seen = std::cell::Cell::new(None);
        let verdict = assess(&hook(slack()), Some(&last), NOW, |connector, since| {
            seen.set(Some((connector.map(str::to_string), since)));
            1551
        });
        assert!(matches!(verdict, Verdict::Stale(ref r) if r.contains("1551 new slack")));
        assert_eq!(
            seen.take(),
            Some((Some("slack".to_string()), NOW - 12 * 24 * 3600))
        );
    }

    #[test]
    fn message_hook_never_ran_with_traffic_is_stale() {
        let verdict = assess(&hook(slack()), None, NOW, |_, _| 3);
        assert!(matches!(verdict, Verdict::Stale(ref r) if r.contains("never ran")));
    }

    #[test]
    fn message_hook_idle_without_messages_is_ok() {
        let last = run_at(NOW - 5 * 24 * 3600);
        assert_eq!(
            assess(&hook(slack()), Some(&last), NOW, |_, _| 0),
            Verdict::Ok
        );
    }

    #[test]
    fn windowed_hook_gets_longer_grace() {
        let mut h = hook(slack());
        h.active_window = Some(ActiveWindow {
            days: vec![Weekday::Mon],
            start: "08:00".into(),
            end: "18:00".into(),
            utc_offset_hours: Some(0),
        });
        let last = run_at(NOW - 48 * 3600);
        assert_eq!(assess(&h, Some(&last), NOW, |_, _| 10), Verdict::Ok);
    }

    #[test]
    fn schedule_hook_missed_slot_is_stale() {
        let h = hook(Trigger::Schedule {
            cron: "0 * * * *".into(),
        });
        let last = run_at(NOW - 5 * 3600);
        assert!(matches!(
            assess(&h, Some(&last), NOW, |_, _| 0),
            Verdict::Stale(_)
        ));
        let recent = run_at(NOW - 30 * 60);
        assert_eq!(assess(&h, Some(&recent), NOW, |_, _| 0), Verdict::Ok);
    }

    #[test]
    fn schedule_hook_invalid_cron_is_stale() {
        let h = hook(Trigger::Schedule {
            cron: "not a cron".into(),
        });
        assert!(matches!(assess(&h, None, NOW, |_, _| 0), Verdict::Stale(_)));
    }

    #[test]
    fn report_counts_stale_hooks_and_handles_empty() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(report(Ok(Vec::new()), "test", Some(&db)), 0);
        assert_eq!(report(Err("ssh down".into()), "test", Some(&db)), 1);
        let hourly = hook(Trigger::Schedule {
            cron: "0 * * * *".into(),
        });
        assert_eq!(report(Ok(vec![hourly]), "test", Some(&db)), 1);
    }
}
