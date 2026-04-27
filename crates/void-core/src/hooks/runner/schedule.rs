use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use super::super::execute::{execute_hook_blocking, HookExecOptions};
use super::super::model::{HookLogInsert, Trigger};
use super::super::placeholders::expand_placeholders;
use super::HookRunner;

impl HookRunner {
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
                    let agent = hook.agent.clone();
                    let exec_opts = HookExecOptions {
                        extra_args: hook.extra_args.clone(),
                    };

                    eprintln!("[hook] ▶ {} (scheduled) executing", name);
                    info!(hook = %name, "executing scheduled hook");
                    let started_at = chrono::Utc::now().timestamp();
                    let start = std::time::Instant::now();

                    let outcome = tokio::task::spawn_blocking(move || {
                        execute_hook_blocking(&agent, &prompt, max_turns, &exec_opts)
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
