use crate::output::resolve_connector_filter;
use void_core::hooks::{self, ActiveWindow, Hook, PromptConfig, Trigger, Weekday};

/// Remote mode: the daemon on the remote host never reads this machine's
/// hooks. Say so on stderr, since `hook list` shows the remote set.
pub(crate) fn warn_local_hooks_ignored(local_dir: &std::path::Path) {
    let files = hooks::hook_files(local_dir);
    if let Some(message) = local_hooks_ignored_message(local_dir, &files) {
        eprintln!("{message}");
    }
}

fn local_hooks_ignored_message(
    local_dir: &std::path::Path,
    files: &[std::path::PathBuf],
) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let names = files
        .iter()
        .filter_map(|f| f.file_stem().and_then(|s| s.to_str()))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "[warn] store.mode = \"remote\": {} local hook(s) in {} are ignored ({names}).\n       \
         The remote sync daemon only loads hooks from the remote host. \
         Copy them with `void hook push`, then restart the remote daemon.",
        files.len(),
        local_dir.display()
    ))
}

pub(crate) fn cmd_push(dir: &std::path::Path, names: &[String]) -> anyhow::Result<()> {
    if !crate::context::is_remote() {
        anyhow::bail!(
            "`void hook push` only applies to store.mode = \"remote\"; hooks in {} are already used by the local daemon",
            dir.display()
        );
    }
    let files = select_hook_files(dir, names)?;
    if files.is_empty() {
        anyhow::bail!("No hook files (*.toml) in {}", dir.display());
    }
    let remote_dir = crate::context::get()
        .push_hooks(&files)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    for file in &files {
        eprintln!("Pushed {} -> {remote_dir}/", file.display());
    }
    eprintln!(
        "The remote daemon loads hooks at startup only: restart it on the remote host \
         (`void sync --stop && void sync --daemon`), then check `void hook list`."
    );
    Ok(())
}

/// Hook files to push: every `*.toml` in `dir`, or those matching `names` by slug.
fn select_hook_files(
    dir: &std::path::Path,
    names: &[String],
) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let files = hooks::hook_files(dir);
    if names.is_empty() {
        return Ok(files);
    }
    names
        .iter()
        .map(|name| {
            let slug = hooks::slugify(name);
            files
                .iter()
                .find(|f| f.file_stem().and_then(|s| s.to_str()) == Some(slug.as_str()))
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Hook '{name}' not found in {}", dir.display()))
        })
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_hooks_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("void-hooks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn select_hook_files_all_or_by_name() {
        let dir = temp_hooks_dir();
        std::fs::write(dir.join("slack-triage.toml"), "").unwrap();
        std::fs::write(dir.join("digest.toml"), "").unwrap();
        std::fs::write(dir.join("slack-triage.md"), "").unwrap();

        let all = select_hook_files(&dir, &[]).unwrap();
        assert_eq!(all.len(), 2);

        let one = select_hook_files(&dir, &["Slack Triage".into()]).unwrap();
        assert_eq!(one, vec![dir.join("slack-triage.toml")]);

        assert!(select_hook_files(&dir, &["missing".into()]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignored_message_lists_local_hooks() {
        let dir = std::path::Path::new("/tmp/hooks");
        assert!(local_hooks_ignored_message(dir, &[]).is_none());
        let msg =
            local_hooks_ignored_message(dir, &[dir.join("slack-triage.toml")]).expect("message");
        assert!(msg.contains("1 local hook(s)"));
        assert!(msg.contains("slack-triage"));
        assert!(msg.contains("void hook push"));
    }
}
