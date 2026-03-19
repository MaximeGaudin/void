use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::HookError;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::db::Database;
use crate::models::Message;

const MAX_CONCURRENT_HOOKS: usize = 2;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookLog {
    pub id: i64,
    pub hook_name: String,
    pub trigger_type: String,
    pub started_at: i64,
    pub duration_ms: i64,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    pub message_id: Option<String>,
    pub input_prompt: Option<String>,
    pub raw_output: Option<String>,
}

/// Parameters for inserting a hook log entry. Used to avoid too many function arguments.
#[derive(Debug)]
pub struct HookLogInsert<'a> {
    pub hook_name: &'a str,
    pub trigger_type: &'a str,
    pub started_at: i64,
    pub duration_ms: i64,
    pub success: bool,
    pub result: Option<&'a str>,
    pub error: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub input_prompt: Option<&'a str>,
    pub raw_output: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    pub trigger: Trigger,
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    NewMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connector: Option<String>,
    },
    Schedule {
        cron: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub text: String,
}

fn default_true() -> bool {
    true
}

fn default_max_turns() -> usize {
    3
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

pub fn hooks_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    home.join(".config/void/hooks")
}

pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn load_hooks(dir: &Path) -> Vec<Hook> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut hooks = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str::<Hook>(&content) {
                Ok(hook) => hooks.push(hook),
                Err(e) => warn!(path = %path.display(), "invalid hook file: {e}"),
            },
            Err(e) => warn!(path = %path.display(), "cannot read hook file: {e}"),
        }
    }
    hooks
}

pub fn save_hook(dir: &Path, hook: &Hook) -> Result<(), HookError> {
    std::fs::create_dir_all(dir)?;
    let filename = format!("{}.toml", slugify(&hook.name));
    let path = dir.join(filename);
    let content = toml::to_string_pretty(hook).map_err(|e| HookError::Other(e.to_string()))?;
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn delete_hook(dir: &Path, name: &str) -> Result<bool, HookError> {
    let filename = format!("{}.toml", slugify(name));
    let path = dir.join(&filename);
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

pub fn find_hook(dir: &Path, name: &str) -> Result<Hook, HookError> {
    load_hooks(dir)
        .into_iter()
        .find(|h| slugify(&h.name) == slugify(name))
        .ok_or_else(|| HookError::NotFound(name.to_string()))
}

pub fn update_hook_enabled(dir: &Path, name: &str, enabled: bool) -> Result<bool, HookError> {
    match find_hook(dir, name) {
        Ok(mut hook) => {
            hook.enabled = enabled;
            save_hook(dir, &hook)?;
            Ok(true)
        }
        Err(HookError::NotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Placeholder expansion
// ---------------------------------------------------------------------------

pub fn expand_placeholders_public(template: &str, msg: Option<&Message>) -> String {
    expand_placeholders(template, msg)
}

fn expand_placeholders(template: &str, msg: Option<&Message>) -> String {
    let now = chrono::Utc::now();
    let mut result = template
        .replace(
            "{now}",
            &now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )
        .replace("{today}", &now.format("%Y-%m-%d").to_string());

    if let Some(msg) = msg {
        result = result.replace("{message_id}", &msg.id);
        result = result.replace("{connector}", &msg.connector);
        result = result.replace("{connection_id}", &msg.connection_id);
        if let Ok(json) = serde_json::to_string_pretty(msg) {
            result = result.replace("{message}", &json);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Executor (shared between event and schedule hooks)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HookExecResult {
    pub input_prompt: String,
    pub raw_output: String,
    pub result_summary: String,
    pub success: bool,
    pub error: Option<String>,
}

pub fn execute_hook_public(prompt: &str, max_turns: usize) -> anyhow::Result<HookExecResult> {
    execute_hook_blocking(prompt, max_turns)
}

fn execute_hook_blocking(prompt: &str, max_turns: usize) -> anyhow::Result<HookExecResult> {
    let mut cmd = std::process::Command::new("claude");
    cmd.args(["-p", prompt]);
    cmd.args(["--verbose"]);
    cmd.args(["--output-format", "stream-json"]);
    cmd.args(["--max-turns", &max_turns.to_string()]);
    cmd.args(["--allowedTools", "Bash(void *),Bash(date *),Bash(echo *)"]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Ok(HookExecResult {
            input_prompt: prompt.to_string(),
            raw_output: if stdout.is_empty() {
                stderr.clone()
            } else {
                stdout
            },
            result_summary: String::new(),
            success: false,
            error: Some(format!(
                "claude exited with {}: {}",
                output.status,
                stderr.trim()
            )),
        });
    }

    let result_summary = extract_result_from_stream(&stdout);

    Ok(HookExecResult {
        input_prompt: prompt.to_string(),
        raw_output: stdout,
        result_summary,
        success: true,
        error: None,
    })
}

fn extract_result_from_stream(stream: &str) -> String {
    let mut result = String::new();
    for line in stream.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if json.get("type").and_then(|t| t.as_str()) == Some("result") {
                if let Some(r) = json.get("result").and_then(|v| v.as_str()) {
                    return r.to_string();
                }
            }
            if json.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(content) = json.pointer("/message/content") {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        result = text.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// HookRunner
// ---------------------------------------------------------------------------

pub struct HookRunner {
    hooks: Vec<Hook>,
    semaphore: Arc<Semaphore>,
    db: Option<Arc<Database>>,
}

impl HookRunner {
    pub fn new(hooks: Vec<Hook>) -> Self {
        Self {
            hooks,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_HOOKS)),
            db: None,
        }
    }

    pub fn with_db(mut self, db: Arc<Database>) -> Self {
        self.db = Some(db);
        self
    }

    pub fn hooks(&self) -> &[Hook] {
        &self.hooks
    }

    /// Called by the database layer when a new message is inserted.
    pub fn on_new_message(&self, msg: &Message) {
        let event_hooks: Vec<_> = self
            .hooks
            .iter()
            .filter(|h| h.enabled)
            .filter(|h| {
                matches!(&h.trigger, Trigger::NewMessage { connector } if
                connector.is_none() || connector.as_deref() == Some(&msg.connector))
            })
            .cloned()
            .collect();

        if event_hooks.is_empty() {
            return;
        }

        let sem = Arc::clone(&self.semaphore);

        for hook in event_hooks {
            let prompt = expand_placeholders(&hook.prompt.text, Some(msg));
            let max_turns = hook.max_turns;
            let hook_name = hook.name.clone();
            let msg_id = msg.id.clone();
            let connector = msg.connector.clone();
            let sem = Arc::clone(&sem);
            let db = self.db.clone();

            tokio::spawn(async move {
                let _permit = match sem.acquire().await {
                    Ok(p) => p,
                    Err(_) => return,
                };

                eprintln!(
                    "[hook] ▶ {} triggered by {}/{}",
                    hook_name, connector, msg_id
                );
                info!(hook = %hook_name, message_id = %msg_id, "executing event hook");
                let started_at = chrono::Utc::now().timestamp();
                let start = std::time::Instant::now();

                let outcome =
                    tokio::task::spawn_blocking(move || execute_hook_blocking(&prompt, max_turns))
                        .await;

                let duration_ms = start.elapsed().as_millis() as i64;

                match outcome {
                    Ok(Ok(ref exec)) => {
                        let summary: String = exec.result_summary.chars().take(200).collect();
                        if exec.success {
                            eprintln!(
                                "[hook] ✓ {} completed in {:.1}s — {}",
                                hook_name,
                                duration_ms as f64 / 1000.0,
                                summary
                            );
                            info!(hook = %hook_name, duration_ms, "hook completed: {summary}");
                        } else {
                            let err = exec.error.as_deref().unwrap_or("unknown error");
                            eprintln!(
                                "[hook] ✗ {} failed in {:.1}s — {}",
                                hook_name,
                                duration_ms as f64 / 1000.0,
                                err
                            );
                            error!(hook = %hook_name, duration_ms, "hook failed: {err}");
                        }
                        if let Some(ref db) = db {
                            db.insert_hook_log(&HookLogInsert {
                                hook_name: &hook_name,
                                trigger_type: "new_message",
                                started_at,
                                duration_ms,
                                success: exec.success,
                                result: Some(&exec.result_summary),
                                error: exec.error.as_deref(),
                                message_id: Some(&msg_id),
                                input_prompt: Some(&exec.input_prompt),
                                raw_output: Some(&exec.raw_output),
                            })
                            .ok();
                        }
                    }
                    Ok(Err(ref e)) => {
                        eprintln!("[hook] ✗ {} crashed — {}", hook_name, e);
                        error!(hook = %hook_name, "hook execution error: {e}");
                        if let Some(ref db) = db {
                            let err_str = e.to_string();
                            db.insert_hook_log(&HookLogInsert {
                                hook_name: &hook_name,
                                trigger_type: "new_message",
                                started_at,
                                duration_ms,
                                success: false,
                                result: None,
                                error: Some(&err_str),
                                message_id: Some(&msg_id),
                                input_prompt: None,
                                raw_output: None,
                            })
                            .ok();
                        }
                    }
                    Err(ref e) => {
                        eprintln!("[hook] ✗ {} panicked — {}", hook_name, e);
                        error!(hook = %hook_name, "hook task panicked: {e}");
                        if let Some(ref db) = db {
                            let err_str = e.to_string();
                            db.insert_hook_log(&HookLogInsert {
                                hook_name: &hook_name,
                                trigger_type: "new_message",
                                started_at,
                                duration_ms,
                                success: false,
                                result: None,
                                error: Some(&err_str),
                                message_id: Some(&msg_id),
                                input_prompt: None,
                                raw_output: None,
                            })
                            .ok();
                        }
                    }
                }
            });
        }
    }

    /// Spawn scheduler tasks for all cron-based hooks.
    pub fn start_schedules(self: &Arc<Self>, cancel: CancellationToken) {
        let schedule_hooks: Vec<_> = self
            .hooks
            .iter()
            .filter(|h| h.enabled && matches!(h.trigger, Trigger::Schedule { .. }))
            .cloned()
            .collect();

        for hook in schedule_hooks {
            let cancel = cancel.clone();
            let sem = Arc::clone(&self.semaphore);
            let hook_name = hook.name.clone();
            let db = self.db.clone();

            let cron_expr = match &hook.trigger {
                Trigger::Schedule { cron } => cron.clone(),
                _ => unreachable!(),
            };

            let cron = match croner::Cron::new(&cron_expr).parse() {
                Ok(c) => c,
                Err(e) => {
                    error!(hook = %hook_name, cron = %cron_expr, "invalid cron expression: {e}");
                    continue;
                }
            };

            info!(hook = %hook_name, cron = %cron_expr, "scheduled hook registered");

            tokio::spawn(async move {
                loop {
                    let now = chrono::Utc::now();
                    let next = match cron.find_next_occurrence(&now, false) {
                        Ok(next) => next,
                        Err(e) => {
                            error!(hook = %hook_name, "cannot find next cron occurrence: {e}");
                            break;
                        }
                    };

                    let delay = (next - now)
                        .to_std()
                        .unwrap_or(std::time::Duration::from_secs(60));
                    info!(hook = %hook_name, next = %next, "next execution in {}s", delay.as_secs());

                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = cancel.cancelled() => {
                            info!(hook = %hook_name, "scheduler cancelled");
                            break;
                        }
                    }

                    if cancel.is_cancelled() {
                        break;
                    }

                    let _permit = match sem.acquire().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let prompt = expand_placeholders(&hook.prompt.text, None);
                    let max_turns = hook.max_turns;
                    let name = hook_name.clone();

                    eprintln!("[hook] ▶ {} (scheduled) executing", name);
                    info!(hook = %name, "executing scheduled hook");
                    let started_at = chrono::Utc::now().timestamp();
                    let start = std::time::Instant::now();

                    let outcome = tokio::task::spawn_blocking(move || {
                        execute_hook_blocking(&prompt, max_turns)
                    })
                    .await;

                    let duration_ms = start.elapsed().as_millis() as i64;

                    match outcome {
                        Ok(Ok(ref exec)) => {
                            let summary: String = exec.result_summary.chars().take(200).collect();
                            if exec.success {
                                eprintln!(
                                    "[hook] ✓ {} completed in {:.1}s — {}",
                                    hook_name,
                                    duration_ms as f64 / 1000.0,
                                    summary
                                );
                                info!(hook = %hook_name, duration_ms, "scheduled hook completed: {summary}");
                            } else {
                                let err = exec.error.as_deref().unwrap_or("unknown error");
                                eprintln!(
                                    "[hook] ✗ {} failed in {:.1}s — {}",
                                    hook_name,
                                    duration_ms as f64 / 1000.0,
                                    err
                                );
                                error!(hook = %hook_name, duration_ms, "scheduled hook failed: {err}");
                            }
                            if let Some(ref db) = db {
                                db.insert_hook_log(&HookLogInsert {
                                    hook_name: &hook_name,
                                    trigger_type: "schedule",
                                    started_at,
                                    duration_ms,
                                    success: exec.success,
                                    result: Some(&exec.result_summary),
                                    error: exec.error.as_deref(),
                                    message_id: None,
                                    input_prompt: Some(&exec.input_prompt),
                                    raw_output: Some(&exec.raw_output),
                                })
                                .ok();
                            }
                        }
                        Ok(Err(ref e)) => {
                            eprintln!("[hook] ✗ {} crashed — {}", hook_name, e);
                            error!(hook = %hook_name, "scheduled hook error: {e}");
                            if let Some(ref db) = db {
                                let err_str = e.to_string();
                                db.insert_hook_log(&HookLogInsert {
                                    hook_name: &hook_name,
                                    trigger_type: "schedule",
                                    started_at,
                                    duration_ms,
                                    success: false,
                                    result: None,
                                    error: Some(&err_str),
                                    message_id: None,
                                    input_prompt: None,
                                    raw_output: None,
                                })
                                .ok();
                            }
                        }
                        Err(ref e) => {
                            eprintln!("[hook] ✗ {} panicked — {}", hook_name, e);
                            error!(hook = %hook_name, "scheduled hook panicked: {e}");
                            if let Some(ref db) = db {
                                let err_str = e.to_string();
                                db.insert_hook_log(&HookLogInsert {
                                    hook_name: &hook_name,
                                    trigger_type: "schedule",
                                    started_at,
                                    duration_ms,
                                    success: false,
                                    result: None,
                                    error: Some(&err_str),
                                    message_id: None,
                                    input_prompt: None,
                                    raw_output: None,
                                })
                                .ok();
                            }
                        }
                    }
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_hooks_returns_empty_for_nonexistent_dir() {
        let dir =
            std::env::temp_dir().join(format!("void-hooks-nonexistent-{}", uuid::Uuid::new_v4()));
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
    }

    #[test]
    fn schedule_hook_roundtrip() {
        let hook = Hook {
            name: "Daily Digest".into(),
            enabled: true,
            max_turns: 10,
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
    fn expand_placeholders_no_message() {
        let result = expand_placeholders("Today is {today}, now is {now}", None);
        assert!(!result.contains("{today}"));
        assert!(!result.contains("{now}"));
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
            body: Some("Hello".into()),
            timestamp: 1_700_000_000,
            synced_at: None,
            is_archived: false,
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
    fn update_hook_enabled_toggles() {
        let dir = std::env::temp_dir().join(format!("void-hooks-test-{}", uuid::Uuid::new_v4()));
        let hook = Hook {
            name: "Toggle Test".into(),
            enabled: true,
            max_turns: 1,
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
}
