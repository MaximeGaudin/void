use clap::{Args, Subcommand};
use tracing::debug;
use void_core::config::{self, expand_tilde, AccountType, VoidConfig};

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
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Gmail search query (e.g. "newer_than:7d", "from:alice@example.com")
    pub query: String,
    /// Max results to return
    #[arg(long, default_value = "20")]
    pub max: u32,
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct ThreadArgs {
    /// Thread ID
    pub thread_id: String,
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct UrlArgs {
    /// Thread ID
    pub thread_id: String,
}

#[derive(Debug, Args)]
pub struct LabelsArgs {
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
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
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
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
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftsArgs {
    /// Max results
    #[arg(long, default_value = "20")]
    pub max: u32,
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
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
    /// Recipient email(s), comma-separated
    #[arg(long)]
    pub to: String,
    /// Email subject
    #[arg(long)]
    pub subject: String,
    /// Email body
    #[arg(long)]
    pub body: String,
    /// Message ID to reply to (creates a reply draft)
    #[arg(long)]
    pub reply_to: Option<String>,
    /// Thread ID to associate with
    #[arg(long)]
    pub thread_id: Option<String>,
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
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
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
}

#[derive(Debug, Args)]
pub struct DraftDeleteArgs {
    /// Draft ID to delete
    pub draft_id: String,
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
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
    /// Gmail account to use
    #[arg(long)]
    pub account: Option<String>,
}

pub async fn run(args: &GmailArgs, json: bool) -> anyhow::Result<()> {
    match &args.command {
        GmailCommand::Search(a) => run_search(a, json).await,
        GmailCommand::Thread(a) => run_thread(a, json).await,
        GmailCommand::Url(a) => run_url(a),
        GmailCommand::Labels(a) => run_labels(a, json).await,
        GmailCommand::Label(a) => run_label_modify(a).await,
        GmailCommand::BatchModify(a) => run_batch_modify(a).await,
        GmailCommand::Drafts(a) => run_drafts(a, json).await,
        GmailCommand::Draft(a) => run_draft(a, json).await,
        GmailCommand::Attachment(a) => run_attachment(a).await,
    }
}

async fn run_search(args: &SearchArgs, json: bool) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;
    let messages = connector.search_api(&args.query, args.max).await?;

    if json {
        let items: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                let attachments: Vec<serde_json::Value> = m
                    .file_attachments()
                    .iter()
                    .map(|a| serde_json::json!(a))
                    .collect();
                serde_json::json!({
                    "id": m.id,
                    "threadId": m.thread_id,
                    "from": m.get_header("From"),
                    "to": m.get_header("To"),
                    "subject": m.get_header("Subject"),
                    "date": m.get_header("Date"),
                    "snippet": m.snippet,
                    "labels": m.label_ids,
                    "attachments": attachments,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "data": items, "error": null }))?
        );
    } else {
        if messages.is_empty() {
            eprintln!("No messages found.");
            return Ok(());
        }
        for m in &messages {
            let from = m.get_header("From").unwrap_or_default();
            let subject = m
                .get_header("Subject")
                .unwrap_or_else(|| "(no subject)".into());
            let date = m.get_header("Date").unwrap_or_default();
            let id = m.id.as_deref().unwrap_or("?");
            let thread = m.thread_id.as_deref().unwrap_or("?");
            println!("[{id}] {date}  {from}");
            println!("  Subject: {subject}");
            println!("  Thread: {thread}");
            let attachments = m.file_attachments();
            if !attachments.is_empty() {
                for a in &attachments {
                    println!("  📎 {} (id: {})", a.filename, a.attachment_id);
                }
            }
            println!();
        }
    }
    Ok(())
}

async fn run_thread(args: &ThreadArgs, json: bool) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;
    let thread = connector.get_thread(&args.thread_id).await?;

    if json {
        let msgs: Vec<serde_json::Value> = thread
            .messages
            .as_ref()
            .map(|msgs| {
                msgs.iter()
                    .map(|m| {
                        let attachments: Vec<serde_json::Value> = m
                            .file_attachments()
                            .iter()
                            .map(|a| serde_json::json!(a))
                            .collect();
                        serde_json::json!({
                            "id": m.id,
                            "from": m.get_header("From"),
                            "to": m.get_header("To"),
                            "subject": m.get_header("Subject"),
                            "date": m.get_header("Date"),
                            "snippet": m.snippet,
                            "labels": m.label_ids,
                            "body": m.text_body(),
                            "attachments": attachments,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "data": {
                    "threadId": thread.id,
                    "messages": msgs,
                },
                "error": null,
            }))?
        );
    } else {
        let msgs = thread.messages.as_deref().unwrap_or(&[]);
        if msgs.is_empty() {
            eprintln!("Thread is empty.");
            return Ok(());
        }
        eprintln!("Thread {} ({} messages)\n", args.thread_id, msgs.len());
        for m in msgs {
            let from = m.get_header("From").unwrap_or_default();
            let date = m.get_header("Date").unwrap_or_default();
            let subject = m
                .get_header("Subject")
                .unwrap_or_else(|| "(no subject)".into());
            let id = m.id.as_deref().unwrap_or("?");
            println!("--- [{id}] {date} ---");
            println!("From: {from}");
            println!("Subject: {subject}");
            let attachments = m.file_attachments();
            if !attachments.is_empty() {
                for a in &attachments {
                    println!("  📎 {} (id: {})", a.filename, a.attachment_id);
                }
            }
            if let Some(body) = m.text_body() {
                println!();
                println!("{body}");
            } else if let Some(snippet) = &m.snippet {
                println!();
                println!("{snippet}...");
            }
            println!();
        }
    }
    Ok(())
}

fn run_url(args: &UrlArgs) -> anyhow::Result<()> {
    let url = void_gmail::connector::GmailConnector::gmail_url(&args.thread_id);
    println!("{url}");
    Ok(())
}

async fn run_labels(args: &LabelsArgs, json: bool) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;
    let labels = connector.list_labels().await?;

    if json {
        let items: Vec<serde_json::Value> = labels
            .iter()
            .map(|l| {
                serde_json::json!({
                    "id": l.id,
                    "name": l.name,
                    "type": l.label_type,
                    "messagesTotal": l.messages_total,
                    "messagesUnread": l.messages_unread,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "data": items, "error": null }))?
        );
    } else {
        if labels.is_empty() {
            eprintln!("No labels found.");
            return Ok(());
        }
        for l in &labels {
            let unread = l
                .messages_unread
                .map(|u| format!(" ({u} unread)"))
                .unwrap_or_default();
            let total = l
                .messages_total
                .map(|t| format!(" [{t} total]"))
                .unwrap_or_default();
            println!("  {}  —  {}{}{}", l.id, l.name, total, unread);
        }
    }
    Ok(())
}

async fn run_label_modify(args: &LabelModifyArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;

    let add_labels: Vec<&str> = args
        .add
        .as_deref()
        .map(|s| s.split(',').map(|l| l.trim()).collect())
        .unwrap_or_default();
    let remove_labels: Vec<&str> = args
        .remove
        .as_deref()
        .map(|s| s.split(',').map(|l| l.trim()).collect())
        .unwrap_or_default();

    if add_labels.is_empty() && remove_labels.is_empty() {
        anyhow::bail!("Specify at least --add or --remove labels.");
    }

    connector
        .modify_thread_labels(&args.thread_id, &add_labels, &remove_labels)
        .await?;

    eprintln!(
        "Thread {} labels modified (added: {:?}, removed: {:?}).",
        args.thread_id, add_labels, remove_labels
    );
    Ok(())
}

async fn run_batch_modify(args: &BatchModifyArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;

    let add_labels: Vec<&str> = args
        .add
        .as_deref()
        .map(|s| s.split(',').map(|l| l.trim()).collect())
        .unwrap_or_default();
    let remove_labels: Vec<&str> = args
        .remove
        .as_deref()
        .map(|s| s.split(',').map(|l| l.trim()).collect())
        .unwrap_or_default();

    if add_labels.is_empty() && remove_labels.is_empty() {
        anyhow::bail!("Specify at least --add or --remove labels.");
    }

    let ids: Vec<&str> = args.message_ids.iter().map(|s| s.as_str()).collect();
    connector
        .batch_modify(&ids, &add_labels, &remove_labels)
        .await?;

    eprintln!(
        "Batch modified {} messages (added: {:?}, removed: {:?}).",
        ids.len(),
        add_labels,
        remove_labels
    );
    Ok(())
}

async fn run_drafts(args: &DraftsArgs, json: bool) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;
    let drafts = connector.list_drafts(args.max).await?;

    if json {
        let items: Vec<serde_json::Value> = drafts
            .iter()
            .map(|d| {
                let msg = d.message.as_ref();
                serde_json::json!({
                    "draftId": d.id,
                    "messageId": msg.and_then(|m| m.id.as_deref()),
                    "threadId": msg.and_then(|m| m.thread_id.as_deref()),
                    "to": msg.and_then(|m| m.get_header("To")),
                    "subject": msg.and_then(|m| m.get_header("Subject")),
                    "snippet": msg.and_then(|m| m.snippet.clone()),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "data": items, "error": null }))?
        );
    } else {
        if drafts.is_empty() {
            eprintln!("No drafts found.");
            return Ok(());
        }
        for d in &drafts {
            let draft_id = d.id.as_deref().unwrap_or("?");
            let msg = d.message.as_ref();
            let to = msg.and_then(|m| m.get_header("To")).unwrap_or_default();
            let subject = msg
                .and_then(|m| m.get_header("Subject"))
                .unwrap_or_else(|| "(no subject)".into());
            println!("[{draft_id}] To: {to}");
            println!("  Subject: {subject}");
            println!();
        }
    }
    Ok(())
}

async fn run_draft(args: &DraftCommand, json: bool) -> anyhow::Result<()> {
    match &args.action {
        DraftAction::Create(a) => {
            let connector = build_gmail_connector(a.account.as_deref())?;
            let draft = connector
                .create_draft(
                    &a.to,
                    &a.subject,
                    &a.body,
                    a.reply_to.as_deref(),
                    a.thread_id.as_deref(),
                )
                .await?;

            let draft_id = draft.id.as_deref().unwrap_or("?");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "data": { "draftId": draft_id },
                        "error": null,
                    }))?
                );
            } else {
                eprintln!("Draft created (id: {draft_id}).");
            }
            Ok(())
        }
        DraftAction::Update(a) => {
            let connector = build_gmail_connector(a.account.as_deref())?;
            let draft = connector
                .update_draft(&a.draft_id, &a.to, &a.subject, &a.body)
                .await?;

            let draft_id = draft.id.as_deref().unwrap_or("?");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "data": { "draftId": draft_id },
                        "error": null,
                    }))?
                );
            } else {
                eprintln!("Draft {} updated.", a.draft_id);
            }
            Ok(())
        }
        DraftAction::Delete(a) => {
            let connector = build_gmail_connector(a.account.as_deref())?;
            connector.delete_draft(&a.draft_id).await?;

            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "data": { "deleted": true },
                        "error": null,
                    }))?
                );
            } else {
                eprintln!("Draft {} deleted.", a.draft_id);
            }
            Ok(())
        }
    }
}

async fn run_attachment(args: &AttachmentArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.account.as_deref())?;
    let data = connector
        .get_attachment_data(&args.message_id, &args.attachment_id)
        .await?;

    std::fs::write(&args.out, &data)?;
    eprintln!("Attachment saved to {} ({} bytes).", args.out, data.len());
    Ok(())
}

fn build_gmail_connector(
    account_filter: Option<&str>,
) -> anyhow::Result<void_gmail::connector::GmailConnector> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let account = cfg
        .accounts
        .iter()
        .find(|a| {
            let is_gmail = a.account_type == AccountType::Gmail;
            let name_matches = account_filter.map_or(true, |n| a.id == n);
            is_gmail && name_matches
        })
        .ok_or_else(|| {
            anyhow::anyhow!("No Gmail account found in config. Run `void setup` to add one.")
        })?;

    let credentials_file = match &account.settings {
        void_core::config::AccountSettings::Gmail { credentials_file } => credentials_file.clone(),
        _ => anyhow::bail!(
            "Mismatched account settings for Gmail account '{}'",
            account.id
        ),
    };

    let cred_path = credentials_file.as_ref().map(|f| expand_tilde(f));
    let store_path = cfg.store_path();
    debug!(account_id = %account.id, "building Gmail connector for CLI");
    Ok(void_gmail::connector::GmailConnector::new(
        &account.id,
        cred_path.as_deref().and_then(|p| p.to_str()),
        &store_path,
    ))
}
