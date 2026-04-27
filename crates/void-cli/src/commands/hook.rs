use clap::{Args, Subcommand};

use crate::output::resolve_connector_filter;
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
        /// The agent to execute the hook (e.g. "claude", "cursor")
        #[arg(long, default_value = "claude")]
        agent: String,
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

pub fn run(args: &HookArgs) -> anyhow::Result<()> {
    let dir = hooks::hooks_dir();

    match &args.command {
        HookCommand::List => cmd_list(&dir),
        HookCommand::Create {
            name,
            trigger,
            connector,
            cron,
            prompt,
            prompt_file,
            max_turns,
            agent,
        } => cmd_create(
            &dir,
            name,
            trigger,
            connector.as_deref(),
            cron.as_deref(),
            prompt.as_deref(),
            prompt_file.as_deref(),
            *max_turns,
            agent,
        ),
        HookCommand::Show { name } => cmd_show(&dir, name),
        HookCommand::Delete { name } => cmd_delete(&dir, name),
        HookCommand::Enable { name } => cmd_toggle(&dir, name, true),
        HookCommand::Disable { name } => cmd_toggle(&dir, name, false),
        HookCommand::Test { name, message_id } => cmd_test(&dir, name, message_id.as_deref()),
        HookCommand::Log { limit, hook, id } => cmd_log(*limit, hook.as_deref(), *id),
    }
}

fn cmd_list(dir: &std::path::Path) -> anyhow::Result<()> {
    let hooks = hooks::load_hooks(dir);
    let output = serde_json::json!({ "data": hooks });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_create(
    dir: &std::path::Path,
    name: &str,
    trigger: &str,
    connector: Option<&str>,
    cron: Option<&str>,
    prompt: Option<&str>,
    prompt_file: Option<&str>,
    max_turns: usize,
    agent: &str,
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
        agent: agent.to_string(),
        extra_args: Vec::new(),
        trigger,
        prompt: PromptConfig { text: prompt_text },
    };

    hooks::save_hook(dir, &hook)?;
    let slug = hooks::slugify(name);
    eprintln!("Hook '{}' created: {}/{}.toml", name, dir.display(), slug);
    Ok(())
}

fn cmd_show(dir: &std::path::Path, name: &str) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)?;
    println!("{}", serde_json::to_string_pretty(&hook)?);
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

fn cmd_test(dir: &std::path::Path, name: &str, message_id: Option<&str>) -> anyhow::Result<()> {
    let hook = hooks::find_hook(dir, name)?;

    let msg = match (&hook.trigger, message_id) {
        (Trigger::NewMessage { .. }, Some(mid)) => {
            let config_path = void_core::config::default_config_path();
            let cfg = void_core::config::VoidConfig::load_or_default(&config_path);
            let db = void_core::db::Database::open(&cfg.db_path())?;
            let msg = super::resolve::resolve_message(&db, mid)?;
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

fn cmd_log(limit: usize, hook_filter: Option<&str>, detail_id: Option<i64>) -> anyhow::Result<()> {
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
