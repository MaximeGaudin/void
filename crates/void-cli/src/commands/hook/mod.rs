mod handlers;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct HookArgs {
    #[command(subcommand)]
    pub command: HookCommand,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
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
        /// Active window: days of the week (comma-separated, e.g. "mon,tue,wed,thu,fri")
        #[arg(long)]
        active_days: Option<String>,
        /// Active window: start time in HH:MM 24h format (e.g. "08:00")
        #[arg(long, requires = "active_days")]
        active_start: Option<String>,
        /// Active window: end time in HH:MM 24h format (e.g. "21:00")
        #[arg(long, requires = "active_days")]
        active_end: Option<String>,
        /// Active window: UTC offset in hours (e.g. 2 for UTC+2, -5 for UTC-5). Defaults to local time.
        #[arg(long)]
        active_utc_offset: Option<i32>,
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
    use handlers::{cmd_create, cmd_delete, cmd_list, cmd_log, cmd_show, cmd_test, cmd_toggle};

    let dir = void_core::hooks::hooks_dir();

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
            active_days,
            active_start,
            active_end,
            active_utc_offset,
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
            active_days.as_deref(),
            active_start.as_deref(),
            active_end.as_deref(),
            *active_utc_offset,
        ),
        HookCommand::Show { name } => cmd_show(&dir, name),
        HookCommand::Delete { name } => cmd_delete(&dir, name),
        HookCommand::Enable { name } => cmd_toggle(&dir, name, true),
        HookCommand::Disable { name } => cmd_toggle(&dir, name, false),
        HookCommand::Test { name, message_id } => cmd_test(&dir, name, message_id.as_deref()),
        HookCommand::Log { limit, hook, id } => cmd_log(*limit, hook.as_deref(), *id),
    }
}
