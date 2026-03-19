use std::sync::Arc;

use grammers_client::client::{Client, UpdatesConfiguration};
use grammers_client::message::Message as TgMessage;
use grammers_client::peer::Peer;
use grammers_client::update::Update;
use grammers_session::updates::UpdatesLike;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use super::extract;

pub(super) async fn run_sync(
    client: &Client,
    updates: UnboundedReceiver<UpdatesLike>,
    db: &Arc<Database>,
    account_id: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    // Start the live update stream BEFORE backfill so that real-time messages
    // arriving during backfill are not lost. Duplicates are handled by upsert.
    let mut stream = client
        .stream_updates(
            updates,
            UpdatesConfiguration {
                catch_up: true,
                ..Default::default()
            },
        )
        .await;

    info!(account_id, "telegram live update stream started");

    let backfill_task = async {
        if let Err(e) = backfill_dialogs(client, db, account_id).await {
            warn!(account_id, error = %e, "telegram backfill failed");
        }
        eprintln!("[telegram:{account_id}] listening for new messages");
    };

    let updates_task = async {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!(account_id, "telegram sync cancelled, persisting update state");
                    stream.sync_update_state().await;
                    break;
                }
                update = stream.next() => {
                    match update {
                        Ok(update) => {
                            if let Err(e) = handle_update(client, db, account_id, &update).await {
                                warn!(error = %e, "error handling telegram update");
                            }
                        }
                        Err(e) => {
                            warn!(error = %e, "telegram update stream error");
                        }
                    }
                    stream.sync_update_state().await;
                }
            }
        }
    };

    tokio::join!(backfill_task, updates_task);
    Ok(())
}

async fn backfill_dialogs(
    client: &Client,
    db: &Arc<Database>,
    account_id: &str,
) -> anyhow::Result<()> {
    info!(account_id, "starting telegram dialog backfill");
    let mut progress = BackfillProgress::new("telegram", "dialogs");

    let mut dialogs = client.iter_dialogs();

    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        let chat_id = peer.id().to_string();
        let conv_external_id = format!("telegram_{account_id}_{chat_id}");

        let kind = match &peer {
            Peer::User(_) => ConversationKind::Dm,
            Peer::Group(_) => ConversationKind::Group,
            Peer::Channel(_) => ConversationKind::Channel,
        };

        let conv = Conversation {
            id: format!("{account_id}-{chat_id}"),
            account_id: account_id.to_string(),
            connector: "telegram".to_string(),
            external_id: conv_external_id.clone(),
            name: peer.name().map(|n| n.to_string()),
            kind,
            last_message_at: None,
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&conv)?;

        backfill_messages(client, db, account_id, peer, &conv_external_id).await?;

        progress.inc(1);
    }

    progress.finish();
    info!(
        account_id,
        dialogs = progress.items,
        "telegram backfill complete"
    );
    Ok(())
}

async fn backfill_messages(
    client: &Client,
    db: &Arc<Database>,
    account_id: &str,
    peer: &Peer,
    conv_external_id: &str,
) -> anyhow::Result<()> {
    let peer_ref = peer
        .to_ref()
        .await
        .ok_or_else(|| anyhow::anyhow!("could not resolve peer ref for backfill"))?;
    let mut messages = client.iter_messages(peer_ref).limit(100);

    while let Some(msg) = messages.next().await? {
        let external_id = format!("telegram_{account_id}_{}", msg.id());

        if db.message_exists(account_id, &external_id)? {
            break;
        }

        let void_msg = tg_message_to_void(&msg, account_id, conv_external_id);
        db.upsert_message(&void_msg)?;
    }

    Ok(())
}

async fn handle_update(
    client: &Client,
    db: &Arc<Database>,
    account_id: &str,
    update: &Update,
) -> anyhow::Result<()> {
    match update {
        Update::NewMessage(msg) => {
            handle_new_message(client, db, account_id, msg).await?;
        }
        Update::MessageEdited(msg) => {
            handle_new_message(client, db, account_id, msg).await?;
        }
        Update::MessageDeleted(deletion) => {
            for msg_id in deletion.messages() {
                let external_id = format!("telegram_{account_id}_{msg_id}");
                debug!(external_id, "telegram message deleted (no-op in DB)");
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_new_message(
    _client: &Client,
    db: &Arc<Database>,
    account_id: &str,
    msg: &TgMessage,
) -> anyhow::Result<()> {
    let peer = match msg.peer() {
        Some(p) => p,
        None => return Ok(()),
    };

    let chat_id = peer.id().to_string();
    let conv_external_id = format!("telegram_{account_id}_{chat_id}");
    let conv_name = peer.name().unwrap_or("?").to_string();

    let kind = match &peer {
        Peer::User(_) => ConversationKind::Dm,
        Peer::Group(_) => ConversationKind::Group,
        Peer::Channel(_) => ConversationKind::Channel,
    };

    let conv = Conversation {
        id: format!("{account_id}-{chat_id}"),
        account_id: account_id.to_string(),
        connector: "telegram".to_string(),
        external_id: conv_external_id.clone(),
        name: Some(conv_name.clone()),
        kind,
        last_message_at: Some(msg.date().timestamp()),
        unread_count: if msg.outgoing() { 0 } else { 1 },
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;

    let void_msg = tg_message_to_void(msg, account_id, &conv_external_id);
    db.upsert_message(&void_msg)?;

    let sender_name = msg
        .sender()
        .and_then(|s| s.name().map(|n| n.to_string()))
        .unwrap_or_else(|| "unknown".to_string());
    let preview: String = extract::extract_text(msg)
        .unwrap_or_default()
        .chars()
        .take(80)
        .collect();
    let time = msg
        .date()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S %Z")
        .to_string();
    let direction = if msg.outgoing() { "sent" } else { "new" };
    eprintln!(
        "[telegram:{account_id}] {time} ({direction}) {conv_name} — {sender_name}: {preview}"
    );

    Ok(())
}

fn tg_message_to_void(msg: &TgMessage, account_id: &str, conv_external_id: &str) -> Message {
    let sender = msg
        .sender()
        .map(|s| s.id().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let sender_name = msg.sender().and_then(|s| s.name().map(|n| n.to_string()));

    let external_id = format!("telegram_{account_id}_{}", msg.id());

    let reply_to_id = msg
        .reply_to_message_id()
        .map(|id| format!("telegram_{account_id}_{id}"));

    let msg_id = msg.id();
    Message {
        id: format!("{account_id}-{msg_id}"),
        conversation_id: conv_external_id.to_string(),
        account_id: account_id.to_string(),
        connector: "telegram".to_string(),
        external_id,
        sender,
        sender_name,
        body: extract::extract_text(msg),
        timestamp: msg.date().timestamp(),
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        reply_to_id,
        media_type: extract::extract_media_type(msg),
        metadata: extract::extract_media_metadata(msg),
        context_id: Some(conv_external_id.to_string()),
        context: None,
    }
}
