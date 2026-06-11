mod handlers;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AgentInboxArgs {
    #[command(subcommand)]
    pub command: AgentInboxCommand,
}

#[derive(Debug, Subcommand)]
pub enum AgentInboxCommand {
    /// Submit a new item to the agent inbox
    Submit {
        /// Item type: fyi, approval, input, action
        #[arg(long = "type")]
        item_type: String,

        /// Unique callback ID (auto-generated UUID if omitted)
        #[arg(long)]
        callback_id: Option<String>,

        /// Source agent name
        #[arg(long)]
        source: String,

        /// Title / subject line
        #[arg(long)]
        title: String,

        /// Markdown body
        #[arg(long)]
        body: String,

        /// Priority: normal or high
        #[arg(long, default_value = "normal")]
        priority: String,

        /// Action JSON (inline)
        #[arg(long, conflicts_with = "action_file")]
        action: Option<String>,

        /// Read action JSON from a file (use - for stdin)
        #[arg(long, conflicts_with = "action")]
        action_file: Option<String>,

        /// Label for the input field (input type only)
        #[arg(long)]
        input_label: Option<String>,
    },
    /// List inbox items
    List {
        /// Filter by status: unread, read, done
        #[arg(long)]
        status: Option<String>,

        /// Filter by type: fyi, approval, input, action
        #[arg(long = "type")]
        item_type: Option<String>,

        /// Max number of items to return
        #[arg(long, default_value = "50")]
        size: i64,
    },
    /// Get a single item by callback ID
    Get {
        /// Callback ID
        callback_id: String,
    },
    /// Record a response on an item
    Respond {
        /// Callback ID
        callback_id: String,

        /// Response text
        #[arg(long)]
        response: String,

        /// Optional comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Mark an item as read
    MarkRead {
        /// Callback ID
        callback_id: String,
    },
    /// Archive one or more items (sets status to done)
    Archive {
        /// Callback IDs to archive
        callback_ids: Vec<String>,
    },
}

pub fn run(args: &AgentInboxArgs) -> anyhow::Result<()> {
    use handlers::{
        run_archive, run_get, run_list, run_mark_read, run_respond, run_submit,
    };

    match &args.command {
        AgentInboxCommand::Submit {
            item_type,
            callback_id,
            source,
            title,
            body,
            priority,
            action,
            action_file,
            input_label,
        } => run_submit(
            item_type,
            callback_id.as_deref(),
            source,
            title,
            body,
            priority,
            action.as_deref(),
            action_file.as_deref(),
            input_label.as_deref(),
        ),
        AgentInboxCommand::List {
            status,
            item_type,
            size,
        } => run_list(status.as_deref(), item_type.as_deref(), *size),
        AgentInboxCommand::Get { callback_id } => run_get(callback_id),
        AgentInboxCommand::Respond {
            callback_id,
            response,
            comment,
        } => run_respond(callback_id, response, comment.as_deref()),
        AgentInboxCommand::MarkRead { callback_id } => run_mark_read(callback_id),
        AgentInboxCommand::Archive { callback_ids } => run_archive(callback_ids),
    }
}

#[cfg(test)]
mod tests;
