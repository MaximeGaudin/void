use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct GmailArgs {
    #[command(subcommand)]
    pub command: GmailCommand,
}

#[derive(Debug, Subcommand)]
pub enum GmailCommand {
    /// Search emails using Gmail query syntax (e.g. "newer_than:7d", "from:alice")
    Search(SearchArgs),
    /// View a full email thread
    Thread(ThreadArgs),
    /// Generate Gmail web URL for a thread
    Url(UrlArgs),
    /// List Gmail labels
    Labels(LabelsArgs),
    /// Modify labels on a thread or message
    Label(LabelModifyArgs),
    /// Batch modify labels on multiple messages
    BatchModify(BatchModifyArgs),
    /// List drafts
    Drafts(DraftsArgs),
    /// Draft management (create, update, delete)
    Draft(DraftCommand),
    /// Download an attachment
    Attachment(AttachmentArgs),
    /// Forward a message to another recipient
    Forward(ForwardArgs),
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Gmail search query (e.g. "newer_than:7d", "from:alice@example.com")
    pub query: String,
    /// Max results to return
    #[arg(long, default_value = "20")]
    pub max: u32,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct ThreadArgs {
    /// Thread ID
    pub thread_id: String,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct UrlArgs {
    /// Thread ID
    pub thread_id: String,
}

#[derive(Debug, Args)]
pub struct LabelsArgs {
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct LabelModifyArgs {
    /// Thread ID to modify
    pub thread_id: String,
    /// Labels to add (comma-separated, e.g. "STARRED,IMPORTANT")
    #[arg(long)]
    pub add: Option<String>,
    /// Labels to remove (comma-separated, e.g. "INBOX,UNREAD")
    #[arg(long)]
    pub remove: Option<String>,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct BatchModifyArgs {
    /// Message IDs to modify
    pub message_ids: Vec<String>,
    /// Labels to add (comma-separated)
    #[arg(long)]
    pub add: Option<String>,
    /// Labels to remove (comma-separated)
    #[arg(long)]
    pub remove: Option<String>,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftsArgs {
    /// Max results
    #[arg(long, default_value = "20")]
    pub max: u32,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftCommand {
    #[command(subcommand)]
    pub action: DraftAction,
}

#[derive(Debug, Subcommand)]
pub enum DraftAction {
    /// Create a new draft
    Create(DraftCreateArgs),
    /// Update an existing draft
    Update(DraftUpdateArgs),
    /// Delete a draft
    Delete(DraftDeleteArgs),
}

#[derive(Debug, Args)]
pub struct DraftCreateArgs {
    /// Recipient email(s), comma-separated. Optional when --reply-to is set (defaults to reply-all).
    #[arg(long)]
    pub to: Option<String>,
    /// Email subject
    #[arg(long)]
    pub subject: String,
    /// Email body
    #[arg(long)]
    pub body: String,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
    /// Message ID to reply to — associates the draft with the thread and sets In-Reply-To headers.
    #[arg(long)]
    pub reply_to: Option<String>,
    /// Append the account's Gmail signature (fetched via users.settings.sendAs).
    /// Pass a body without an existing signature — re-appending doubles it.
    #[arg(long)]
    pub signature: bool,
    /// Send-as alias whose signature to use (requires --signature). Defaults to the account default/primary.
    #[arg(long, requires = "signature")]
    pub signature_from: Option<String>,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftUpdateArgs {
    /// Draft ID to update
    pub draft_id: String,
    /// Recipient email(s), comma-separated
    #[arg(long)]
    pub to: String,
    /// Email subject
    #[arg(long)]
    pub subject: String,
    /// Email body
    #[arg(long)]
    pub body: String,
    /// File to attach
    #[arg(long)]
    pub file: Option<String>,
    /// Append the account's Gmail signature (fetched via users.settings.sendAs).
    /// Not idempotent: pass `--body` without an existing signature or it will be duplicated.
    #[arg(long)]
    pub signature: bool,
    /// Send-as alias whose signature to use (requires --signature). Defaults to the account default/primary.
    #[arg(long, requires = "signature")]
    pub signature_from: Option<String>,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftDeleteArgs {
    /// Draft ID to delete
    pub draft_id: String,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct ForwardArgs {
    /// Message ID (void internal ID)
    pub message_id: String,
    /// Recipient email address
    #[arg(long)]
    pub to: String,
    /// Optional comment to include above the forwarded message
    #[arg(long)]
    pub comment: Option<String>,
    /// Append the account's Gmail signature (fetched via users.settings.sendAs).
    /// Placed between the comment and the forwarded quote (Gmail UI order).
    #[arg(long)]
    pub signature: bool,
    /// Send-as alias whose signature to use (requires --signature). Defaults to the account default/primary.
    #[arg(long, requires = "signature")]
    pub signature_from: Option<String>,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}

#[derive(Debug, Args)]
pub struct AttachmentArgs {
    /// Message ID containing the attachment
    pub message_id: String,
    /// Attachment ID
    pub attachment_id: String,
    /// Output file path
    #[arg(long)]
    pub out: String,
    /// Gmail connection to use
    #[arg(long)]
    pub connection: Option<String>,
}
