use clap::{Args, Subcommand};

use void_core::hooks::{self, Hook, PromptConfig, Trigger};

#[derive(Debug, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub command: HookCommand,
}

#[derive(Debug, Subcommand)]
pub enum HookCommand {
    /// List all hooks
    List,
    /// Create a new hook
    Create {
        /// Hook name
        #[arg(long)]
        name: String,
        /// Trigger type: new_message or schedule
        #[arg(long)]
        trigger: String,
        /// Connector filter (only for new_message triggers)
        #[arg(long)]
        connector: Option<String>,
        /// Cron expression (only for schedule triggers)
        #[arg(long)]
        cron: Option<String>,
        /// Prompt text (inline)
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        /// Read prompt from a file
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<String>,
        /// Max agent turns
        #[arg(long, default_value = "3")]
        max_turns: usize,
    },
    /// Show a hook's full configuration
    Show {
        /// Hook name (or slug)
        name: String,
    },
    /// Delete a hook
    Delete {
        /// Hook name (or slug)
        name: String,
    },
    /// Enable a hook
    Enable {
        /// Hook name (or slug)
        name: String,
    },
    /// Disable a hook
    Disable {
        /// Hook name (or slug)
        name: String,
    },
    /// Test a hook (dry-run): execute it against a specific message or immediately for schedules
    Test {
        /// Hook name (or slug)
        name: String,
        /// Message ID to test against (for new_message hooks)
        #[arg(long)]
        message_id: Option<String>,
    },
    /// Show recent hook execution logs
    Log {
        /// Number of log entries to show
        #[arg(long, short = 'n', default_value = "100")]
        limit: usize,
        /// Filter by hook name
        #[arg(long)]
        hook: Option<String>,
        /// Show full detail for a specific log entry ID
        #[arg(long)]
        id: Option<i64>,
    },
}

pub fn run(args: &HookArgs, json: bool) -> anyhow::Result<()> {
    let dir = hooks::hooks_dir();

    match &args.command {
        HookCommand::List => cmd_list(&dir, json),
        HookCommand::Create {
            name,
            trigger,
            connector,
            cron,
            prompt,
            prompt_file,
            max_turns,
        } => cmd_create(
            &dir,
            name,
            trigger,
            connector.as_deref(),
            cron.as_deref(),
            prompt.as_deref(),
            prompt_file.as_deref(),
            *max_turns,
        ),
        HookCommand::Show { name } => cmd_show(&dir, name, json),
        HookCommand::Delete { name } => cmd_delete(&dir, name),
        HookCommand::Enable { name } => cmd_toggle(&dir, name, true),
        HookCommand::Disable { name } => cmd_toggle(&dir, name, false),
        HookCommand::Test { name, message_id } => cmd_test(&dir, name, message_id.as_deref()),
        HookCommand::Log { limit, hook, id } => cmd_log(*limit, hook.as_deref(), *id, json),
    }
}

fn cmd_list(dir: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let hooks = hooks::load_hooks(dir);

    if hooks.is_empty() {
        eprintln!("No hooks configured. Create one with: void hook create --name <name> --trigger <type> --prompt \"...\"");
        if json {
            println!("{{\"data\":[]}}");
        }
        return Ok(());
    }

    if json {
        let output = serde_json::json!({ "data": hooks });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for hook in &hooks {
            let status = if hook.enabled { "enabled" } else { "disabled" };
            let trigger_desc = match &hook.trigger {
                Trigger::NewMessage { connector: Some(c) } => format!("new_message ({})", c),
                Trigger::NewMessage { connector: None } => "new_message (all)".into(),
                Trigger::Schedule { cron } => format!("schedule ({})", cron),
            };
            println!(
                "  {} [{}] — {} (max_turns: {})",
                hook.name, status, trigger_desc, hook.max_turns
            );
        }
    }
    Ok(())
}

fn cmd_create(
    dir: &std::path::Path,
    name: &str,
    trigger: &str,
    connector: Option<&str>,
    cron: Option<&str>,
    prompt: Option<&str>,
    prompt_file: Option<&str>,
    max_turns: usize,
) -> anyhow::Result<()> {
    let prompt_text = match (prompt, prompt_file) {
        (Some(text), _) => text.to_string(),
        (_, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read prompt file '{}': {}", path, e))?,
        _ => anyhow::bail!("Provide --prompt or --prompt-file"),
    };

    let trigger = match trigger.to_lowercase().as_str() {
        "new_message" | "new-message" | "message" => Trigger::NewMessage {
            connector: connector.map(|s| s.to_string()),
        },
        "schedule" | "cron" => {
            let cron_expr = cron
                .ok_or_else(|| anyhow::anyhow!("--cron is required for schedule triggers"))?;
            croner::Cron::new(cron_expr)
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", cron_expr, e))?;
            Trigger::Schedule {
                cron: cron_expr.to_string(),
            }
        }
        other => anyhow::bail!(
            "Unknown trigger type '{}'. Supported: new_message, schedule",
            other
        ),
    };

    let hook = Hook {
        name: name.to_string(),
        enabled: true,
        max_turns,
        trigger,
        prompt: PromptConfig { text: prompt_text },
    };

    hooks::save_hook(dir, &hook)?;
    let slug = hooks::slugify(name);
    eprintln!(
        "Hook '{}' created: {}/{}.toml",
        name,
        dir.display(),
        slug
    );
    Ok(())
}

fn cmd_show(dir: &std::path::Path, name: &str, json: bool) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", name))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&hook)?);
    } else {
        println!("{}", toml::to_string_pretty(&hook)?);
    }
    Ok(())
}

fn cmd_delete(dir: &std::path::Path, name: &str) -> anyhow::Result<()> {
    if hooks::delete_hook(dir, name)? {
        eprintln!("Hook '{}' deleted.", name);
    } else {
        anyhow::bail!("Hook '{}' not found", name);
    }
    Ok(())
}

fn cmd_toggle(dir: &std::path::Path, name: &str, enabled: bool) -> anyhow::Result<()> {
    if hooks::update_hook_enabled(dir, name, enabled)? {
        let state = if enabled { "enabled" } else { "disabled" };
        eprintln!("Hook '{}' {}.", name, state);
    } else {
        anyhow::bail!("Hook '{}' not found", name);
    }
    Ok(())
}

fn cmd_test(
    dir: &std::path::Path,
    name: &str,
    message_id: Option<&str>,
) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", name))?;

    let msg = match (&hook.trigger, message_id) {
        (Trigger::NewMessage { .. }, Some(mid)) => {
            let config_path = void_core::config::default_config_path();
            let cfg = void_core::config::VoidConfig::load_or_default(&config_path);
            let db = void_core::db::Database::open(&cfg.db_path())?;
            let msg = db
                .get_message(mid)?
                .ok_or_else(|| anyhow::anyhow!("Message '{}' not found in database", mid))?;
            Some(msg)
        }
        (Trigger::NewMessage { .. }, None) => {
            anyhow::bail!(
                "new_message hooks require --message-id for testing.\n\
                 Example: void hook test {} --message-id <id>",
                name
            );
        }
        (Trigger::Schedule { .. }, _) => None,
    };

    let prompt = hooks::expand_placeholders_public(&hook.prompt.text, msg.as_ref());
    eprintln!("Executing hook '{}' (max_turns: {})...\n", hook.name, hook.max_turns);

    let exec = hooks::execute_hook_public(&prompt, hook.max_turns)?;
    if exec.success {
        println!("{}", exec.result_summary);
    } else {
        eprintln!("Hook failed: {}", exec.error.as_deref().unwrap_or("unknown error"));
        println!("{}", exec.raw_output);
    }
    Ok(())
}

fn cmd_log(limit: usize, hook_filter: Option<&str>, detail_id: Option<i64>, json: bool) -> anyhow::Result<()> {
    let config_path = void_core::config::default_config_path();
    let cfg = void_core::config::VoidConfig::load_or_default(&config_path);
    let db = void_core::db::Database::open(&cfg.db_path())?;
    let mut logs = db.list_hook_logs(limit)?;

    if let Some(filter) = hook_filter {
        let filter_lower = filter.to_lowercase();
        logs.retain(|l| l.hook_name.to_lowercase().contains(&filter_lower));
    }

    if let Some(id) = detail_id {
        let entry = logs.iter().find(|l| l.id == id);
        return match entry {
            Some(log) => print_log_detail(log, json),
            None => {
                anyhow::bail!("Log entry #{id} not found. Run `void hook log` to list available entries.");
            }
        };
    }

    if logs.is_empty() {
        eprintln!("No hook execution logs found.");
        if json {
            println!("{{\"data\":[]}}");
        }
        return Ok(());
    }

    if json {
        let output = serde_json::json!({ "data": logs });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for log in &logs {
            let ts = chrono::DateTime::from_timestamp(log.started_at, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| log.started_at.to_string());

            let status = if log.success { "OK" } else { "FAIL" };
            let duration = format_duration(log.duration_ms);

            println!(
                "  #{:<4} {} [{:>4}] {} — {} ({}, {})",
                log.id, ts, status, log.hook_name, log.trigger_type, duration,
                log.message_id.as_deref().unwrap_or("-")
            );

            if let Some(ref err) = log.error {
                println!("         error: {}", err);
            }
            if let Some(ref result) = log.result {
                let preview: String = result.chars().take(120).collect();
                if !preview.is_empty() {
                    println!("         result: {}", preview);
                }
            }
        }
        eprintln!("\nShowing {} entries. Use `void hook log --id <ID>` for full detail.", logs.len());
    }
    Ok(())
}

fn print_log_detail(log: &hooks::HookLog, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(log)?);
        return Ok(());
    }

    let ts = chrono::DateTime::from_timestamp(log.started_at, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| log.started_at.to_string());
    let status = if log.success { "OK" } else { "FAIL" };

    println!("═══ Hook Execution #{} ═══", log.id);
    println!("  Hook:      {}", log.hook_name);
    println!("  Trigger:   {}", log.trigger_type);
    println!("  Status:    {}", status);
    println!("  Started:   {}", ts);
    println!("  Duration:  {}", format_duration(log.duration_ms));
    if let Some(ref mid) = log.message_id {
        println!("  Message:   {}", mid);
    }

    if let Some(ref err) = log.error {
        println!("\n── Error ──");
        println!("{}", err);
    }

    if let Some(ref result) = log.result {
        if !result.is_empty() {
            println!("\n── Result ──");
            println!("{}", result);
        }
    }

    if let Some(ref prompt) = log.input_prompt {
        println!("\n── Input Prompt ──");
        println!("{}", prompt);
    }

    if let Some(ref raw) = log.raw_output {
        println!("\n── Agent Session (stream-json) ──");
        for line in raw.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                print_stream_event(&val);
            } else {
                println!("  {}", line);
            }
        }
    }

    println!("\n═══ End ═══");
    Ok(())
}

fn print_stream_event(event: &serde_json::Value) {
    let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("?");
    match event_type {
        "assistant" => {
            if let Some(content) = event.pointer("/message/content").and_then(|c| c.as_array()) {
                for block in content {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                println!("  [assistant] {}", text);
                            }
                        }
                        Some("tool_use") => {
                            let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                            let input = block.get("input")
                                .map(|i| serde_json::to_string(i).unwrap_or_default())
                                .unwrap_or_default();
                            println!("  [tool_call] {} → {}", name, input);
                        }
                        _ => {}
                    }
                }
            }
        }
        "tool" | "tool_result" => {
            let content = event.get("content")
                .or_else(|| event.pointer("/content"))
                .and_then(|c| {
                    if let Some(s) = c.as_str() {
                        Some(s.to_string())
                    } else if let Some(arr) = c.as_array() {
                        let texts: Vec<_> = arr.iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect();
                        if texts.is_empty() { None } else { Some(texts.join("\n")) }
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            if !content.is_empty() {
                let preview: String = content.chars().take(500).collect();
                println!("  [tool_result] {}", preview);
            }
        }
        "result" => {
            let subtype = event.get("subtype").and_then(|s| s.as_str()).unwrap_or("?");
            let result = event.get("result").and_then(|r| r.as_str()).unwrap_or("");
            let turns = event.get("num_turns").and_then(|n| n.as_u64());
            let cost = event.get("cost_usd").and_then(|c| c.as_f64());
            print!("  [result] status={}", subtype);
            if let Some(t) = turns { print!(", turns={}", t); }
            if let Some(c) = cost { print!(", cost=${:.4}", c); }
            println!();
            if !result.is_empty() {
                println!("           {}", result);
            }
        }
        _ => {}
    }
}

fn format_duration(ms: i64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{}m{}s", mins, secs)
    }
}
