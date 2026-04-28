use std::sync::Arc;

use tracing::{error, info};

use crate::models::Message;

use super::super::execute::{execute_hook_blocking, HookExecOptions};
use super::super::model::{HookLogInsert, Trigger};
use super::super::placeholders::expand_placeholders;
use super::HookRunner;

impl HookRunner {
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
            let agent = hook.agent.clone();
            let exec_opts = HookExecOptions {
                extra_args: hook.extra_args.clone(),
            };
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

                let outcome = tokio::task::spawn_blocking(move || {
                    execute_hook_blocking(&agent, &prompt, max_turns, &exec_opts)
                })
                .await;

                let duration_ms = start.elapsed().as_millis() as i64;

                match outcome {
                    Ok(Ok(ref exec)) => {
                        let summary: String = exec.result_summary.chars().take(200).collect();
                        if exec.success {
                            crate::status!(
                                "[hook] ✓ {} completed in {:.1}s — {}",
                                hook_name,
                                duration_ms as f64 / 1000.0,
                                summary
                            );
                            info!(hook = %hook_name, duration_ms, "hook completed: {summary}");
                        } else {
                            let err = exec.error.as_deref().unwrap_or("unknown error");
                            crate::status!(
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
                        crate::status!("[hook] ✗ {} crashed — {}", hook_name, e);
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
                        crate::status!("[hook] ✗ {} panicked — {}", hook_name, e);
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
}
