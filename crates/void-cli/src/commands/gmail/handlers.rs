use void_core::config::{self, VoidConfig};
use void_core::connector::Connector;
use void_core::db::Database;

use super::{
    build_gmail_connector, strip_void_id_prefix, AttachmentArgs, BatchModifyArgs, DraftAction,
    DraftCommand, DraftsArgs, ForwardArgs, GmailArgs, GmailCommand, LabelModifyArgs, LabelsArgs,
    SearchArgs, ThreadArgs, UrlArgs,
};

pub(super) async fn dispatch(args: &GmailArgs) -> anyhow::Result<()> {
    match &args.command {
        GmailCommand::Search(a) => run_search(a).await,
        GmailCommand::Thread(a) => run_thread(a).await,
        GmailCommand::Url(a) => run_url(a),
        GmailCommand::Labels(a) => run_labels(a).await,
        GmailCommand::Label(a) => run_label_modify(a).await,
        GmailCommand::BatchModify(a) => run_batch_modify(a).await,
        GmailCommand::Drafts(a) => run_drafts(a).await,
        GmailCommand::Draft(a) => run_draft(a).await,
        GmailCommand::Attachment(a) => run_attachment(a).await,
        GmailCommand::Forward(a) => run_forward(a).await,
    }
}

async fn run_search(args: &SearchArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;
    let messages = connector.search_api(&args.query, args.max).await?;

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
    Ok(())
}

async fn run_thread(args: &ThreadArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;
    let thread = connector.get_thread(&args.thread_id).await?;

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
    Ok(())
}

fn run_url(args: &UrlArgs) -> anyhow::Result<()> {
    let url = void_gmail::connector::GmailConnector::gmail_url(&args.thread_id);
    println!("{url}");
    Ok(())
}

async fn run_labels(args: &LabelsArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;
    let labels = connector.list_labels().await?;

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
    Ok(())
}

async fn run_label_modify(args: &LabelModifyArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;

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
    let connector = build_gmail_connector(args.connection.as_deref())?;

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

async fn run_drafts(args: &DraftsArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;
    let drafts = connector.list_drafts(args.max).await?;

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
    Ok(())
}

async fn run_draft(args: &DraftCommand) -> anyhow::Result<()> {
    match &args.action {
        DraftAction::Create(a) => {
            let connector = build_gmail_connector(a.connection.as_deref())?;
            let file_path = a.file.as_deref().map(std::path::Path::new);
            let reply_to = a.reply_to.as_deref().map(strip_void_id_prefix);
            let draft = connector
                .create_draft(a.to.as_deref(), &a.subject, &a.body, reply_to, file_path)
                .await?;

            let draft_id = draft.id.as_deref().unwrap_or("?");
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "data": { "draftId": draft_id },
                    "error": null,
                }))?
            );
            Ok(())
        }
        DraftAction::Update(a) => {
            let connector = build_gmail_connector(a.connection.as_deref())?;
            let file_path = a.file.as_deref().map(std::path::Path::new);
            let draft = connector
                .update_draft(&a.draft_id, &a.to, &a.subject, &a.body, file_path)
                .await?;

            let draft_id = draft.id.as_deref().unwrap_or("?");
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "data": { "draftId": draft_id },
                    "error": null,
                }))?
            );
            Ok(())
        }
        DraftAction::Delete(a) => {
            let connector = build_gmail_connector(a.connection.as_deref())?;
            connector.delete_draft(&a.draft_id).await?;

            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "data": { "deleted": true },
                    "error": null,
                }))?
            );
            Ok(())
        }
    }
}

async fn run_forward(args: &ForwardArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let db = Database::open(&cfg.db_path())?;

    let msg = crate::commands::resolve::resolve_message(&db, &args.message_id)?;
    crate::commands::resolve::check_forward_connector(&args.message_id, &msg.connector, "gmail")?;

    let conv = db
        .get_conversation(&msg.conversation_id)?
        .ok_or_else(|| anyhow::anyhow!("Conversation not found: {}", msg.conversation_id))?;

    let conn_id = crate::commands::resolve::resolve_forward_connection(
        args.connection.as_deref(),
        &msg.connection_id,
    );
    let connector = build_gmail_connector(Some(conn_id))?;

    let fwd_id = connector
        .forward(
            &msg.external_id,
            &conv.external_id,
            &args.to,
            args.comment.as_deref(),
        )
        .await?;

    eprintln!("Message forwarded (id: {fwd_id})");
    Ok(())
}

async fn run_attachment(args: &AttachmentArgs) -> anyhow::Result<()> {
    let connector = build_gmail_connector(args.connection.as_deref())?;
    let data = connector
        .get_attachment_data(&args.message_id, &args.attachment_id)
        .await?;

    std::fs::write(&args.out, &data)?;
    eprintln!("Attachment saved to {} ({} bytes).", args.out, data.len());
    Ok(())
}
