use crate::output::resolve_connector_filter;
use void_core::hooks::{self, ActiveWindow, Hook, PromptConfig, Trigger, Weekday};

pub(crate) fn cmd_list(dir: &std::path::Path) -> anyhow::Result<()> {
    let hooks = hooks::load_hooks(dir);
    let output = serde_json::json!({ "data": hooks });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_create(
    dir: &std::path::Path,
    name: &str,
    trigger: &str,
    connector: Option<&str>,
    cron: Option<&str>,
    prompt: Option<&str>,
    prompt_file: Option<&str>,
    max_turns: usize,
    agent: &str,
    active_days: Option<&str>,
    active_start: Option<&str>,
    active_end: Option<&str>,
    active_utc_offset: Option<i32>,
) -> anyhow::Result<()> {
    let prompt_text = match (prompt, prompt_file) {
        (Some(text), _) => text.to_string(),
        (_, Some(path)) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read prompt file '{}': {}", path, e))?,
        _ => anyhow::bail!("Provide --prompt or --prompt-file"),
    };

    let resolved_connector = resolve_connector_filter(connector)?;

    let trigger = match trigger.to_lowercase().as_str() {
        "new_message" | "new-message" | "message" => Trigger::NewMessage {
            connector: resolved_connector,
        },
        "schedule" | "cron" => {
            let cron_expr =
                cron.ok_or_else(|| anyhow::anyhow!("--cron is required for schedule triggers"))?;
            std::str::FromStr::from_str(cron_expr)
                .map(|_: croner::Cron| ())
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

    let active_window = if let Some(days_str) = active_days {
        let days: Vec<Weekday> = days_str
            .split(',')
            .map(|s| {
                Weekday::parse(s.trim()).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid day '{}'. Use: mon,tue,wed,thu,fri,sat,sun",
                        s.trim()
                    )
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        if days.is_empty() {
            anyhow::bail!("--active-days must contain at least one day");
        }

        let start = active_start.unwrap_or("00:00").to_string();
        let end = active_end.unwrap_or("23:59").to_string();

        validate_time_format(&start)?;
        validate_time_format(&end)?;

        Some(ActiveWindow {
            days,
            start,
            end,
            utc_offset_hours: active_utc_offset,
        })
    } else {
        None
    };

    let hook = Hook {
        name: name.to_string(),
        enabled: true,
        max_turns,
        agent: agent.to_string(),
        extra_args: Vec::new(),
        active_window,
        trigger,
        prompt: PromptConfig { text: prompt_text },
    };

    hooks::save_hook(dir, &hook)?;
    let slug = hooks::slugify(name);
    eprintln!("Hook '{}' created: {}/{}.toml", name, dir.display(), slug);
    Ok(())
}

fn validate_time_format(time: &str) -> anyhow::Result<()> {
    let parts: Vec<&str> = time.split(':').collect();
    if parts.len() != 2 {
        anyhow::bail!("Invalid time format '{}'. Expected HH:MM", time);
    }
    let h: u32 = parts[0]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid hour in '{}'", time))?;
    let m: u32 = parts[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid minute in '{}'", time))?;
    if h > 23 || m > 59 {
        anyhow::bail!("Time '{}' out of range (00:00 - 23:59)", time);
    }
    Ok(())
}

pub(crate) fn cmd_show(dir: &std::path::Path, name: &str) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)?;
    println!("{}", serde_json::to_string_pretty(&hook)?);
    Ok(())
}

pub(crate) fn cmd_delete(dir: &std::path::Path, name: &str) -> anyhow::Result<()> {
    if hooks::delete_hook(dir, name)? {
        eprintln!("Hook '{}' deleted.", name);
    } else {
        anyhow::bail!("Hook '{}' not found", name);
    }
    Ok(())
}

pub(crate) fn cmd_toggle(dir: &std::path::Path, name: &str, enabled: bool) -> anyhow::Result<()> {
    if hooks::update_hook_enabled(dir, name, enabled)? {
        let state = if enabled { "enabled" } else { "disabled" };
        eprintln!("Hook '{}' {}.", name, state);
    } else {
        anyhow::bail!("Hook '{}' not found", name);
    }
    Ok(())
}

pub(crate) fn cmd_test(
    dir: &std::path::Path,
    name: &str,
    message_id: Option<&str>,
) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)?;

    let msg = match (&hook.trigger, message_id) {
        (Trigger::NewMessage { .. }, Some(mid)) => {
            let db = crate::context::open_db()?;
            let msg = super::super::resolve::resolve_message(&db, mid)?;
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
    eprintln!(
        "Executing hook '{}' (agent: {}, max_turns: {})...\n",
        hook.name, hook.agent, hook.max_turns
    );

    let exec_opts = hooks::HookExecOptions {
        extra_args: hook.extra_args.clone(),
    };
    let exec = hooks::execute_hook_public(&hook.agent, &prompt, hook.max_turns, &exec_opts)?;
    if exec.success {
        println!("{}", exec.result_summary);
    } else {
        eprintln!(
            "Hook failed: {}",
            exec.error.as_deref().unwrap_or("unknown error")
        );
        println!("{}", exec.raw_output);
    }
    Ok(())
}

pub(crate) fn cmd_log(
    limit: usize,
    hook_filter: Option<&str>,
    detail_id: Option<i64>,
) -> anyhow::Result<()> {
    let db = crate::context::open_db()?;
    let mut logs = db.list_hook_logs(limit)?;

    if let Some(filter) = hook_filter {
        let filter_lower = filter.to_lowercase();
        logs.retain(|l| l.hook_name.to_lowercase().contains(&filter_lower));
    }

    if let Some(id) = detail_id {
        let entry = logs.iter().find(|l| l.id == id);
        return match entry {
            Some(log) => print_log_detail(log),
            None => {
                anyhow::bail!(
                    "Log entry #{id} not found. Run `void hook log` to list available entries."
                );
            }
        };
    }

    let output = serde_json::json!({ "data": logs });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_log_detail(log: &hooks::HookLog) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(log)?);
    Ok(())
}
