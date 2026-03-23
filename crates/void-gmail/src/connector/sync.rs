use std::collections::HashSet;

use tracing::{debug, info, warn};

use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

use crate::api::GmailMessage;

use super::compose::{html_to_markdown, looks_like_html, parse_email_address, parse_email_name};
use super::GmailConnector;

impl GmailConnector {
    pub(crate) async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;

        let profile = api.get_profile().await?;
        if let Some(email) = &profile.email_address {
            *self.my_email.lock().expect("mutex") = Some(email.clone());
        }

        let had_history = db.get_sync_state(&self.config_id, "history_id")?.is_some();

        if had_history {
            debug!(config_id = %self.config_id, "history_id exists, refreshing inbox state");
            self.refresh_inbox(db).await?;
            return Ok(());
        }

        if let Some(history_id) = &profile.history_id {
            db.set_sync_state(&self.config_id, "history_id", history_id)?;
        }

        info!(config_id = %self.config_id, "starting Gmail initial sync");

        let mut page_token: Option<String> = None;
        let max_pages: u64 = 5;

        let mut progress = void_core::progress::BackfillProgress::new(
            &format!("gmail:{}", self.config_id),
            "messages",
        );
        progress.set_pages(max_pages);

        loop {
            let resp = api
                .list_messages(
                    100,
                    page_token.as_deref(),
                    Some(&["INBOX"]),
                    Some("newer_than:7d"),
                )
                .await?;
            progress.inc_page();

            if let Some(msgs) = resp.messages {
                for msg_ref in &msgs {
                    match api.get_message(&msg_ref.id).await {
                        Ok(msg) => {
                            self.store_message(db, &msg)?;
                            progress.inc(1);
                        }
                        Err(e) => {
                            warn!(message_id = %msg_ref.id, "failed to fetch message: {e}");
                        }
                    }
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() || progress.pages_done >= max_pages {
                break;
            }
        }

        progress.finish();
        info!(config_id = %self.config_id, messages = progress.items, "Gmail initial sync complete");
        Ok(())
    }

    /// Refresh inbox state: fetch current INBOX message IDs from Gmail and
    /// reconcile `is_archived` in the local DB so it mirrors Gmail exactly.
    /// Also fetches any new INBOX messages not yet in the local DB.
    async fn refresh_inbox(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        let connection_id = self.display_connection_id();

        let mut inbox_ids: HashSet<String> = HashSet::new();
        let mut new_msg_ids: Vec<String> = Vec::new();
        let mut page_token: Option<String> = None;
        let max_pages = 5u32;
        let mut pages = 0u32;

        loop {
            let resp = api
                .list_messages(
                    100,
                    page_token.as_deref(),
                    Some(&["INBOX"]),
                    Some("newer_than:7d"),
                )
                .await?;
            pages += 1;

            if let Some(msgs) = &resp.messages {
                for msg_ref in msgs {
                    inbox_ids.insert(msg_ref.id.clone());
                    if !db.message_exists(&connection_id, &msg_ref.id)? {
                        new_msg_ids.push(msg_ref.id.clone());
                    }
                }
            }

            page_token = resp.next_page_token;
            if page_token.is_none() || pages >= max_pages {
                break;
            }
        }

        for msg_id in &new_msg_ids {
            match api.get_message(msg_id).await {
                Ok(msg) => {
                    self.store_message(db, &msg)?;
                }
                Err(e) => {
                    warn!(message_id = %msg_id, "failed to fetch new message: {e}");
                }
            }
        }

        let (unarchived, archived) = db.reconcile_inbox(&connection_id, "gmail", &inbox_ids)?;

        if unarchived > 0 || archived > 0 || !new_msg_ids.is_empty() {
            info!(
                config_id = %self.config_id,
                new = new_msg_ids.len(),
                unarchived,
                archived,
                "inbox refresh complete"
            );
        }

        Ok(())
    }

    pub(crate) async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let Some(history_id) = db.get_sync_state(&self.config_id, "history_id")? else {
            debug!("no history_id, skipping incremental sync");
            return Ok(());
        };

        let api = self.get_client().await?;
        let connection_id = self.display_connection_id();
        let resp = api.list_history(&history_id, Some("INBOX")).await?;

        if let Some(records) = resp.history {
            for record in &records {
                if let Some(added) = &record.messages_added {
                    for item in added {
                        match api.get_message(&item.message.id).await {
                            Ok(msg) => {
                                let labels = msg.label_ids.as_deref().unwrap_or(&[]);
                                let is_sent = labels.iter().any(|l| l == "SENT");
                                let is_inbox = labels.iter().any(|l| l == "INBOX");

                                if is_sent && !is_inbox {
                                    debug!(message_id = %item.message.id, "skipping sent-only message");
                                    continue;
                                }

                                let from = msg.get_header("From").unwrap_or_default();
                                let sender = parse_email_name(&from);
                                let subject = msg
                                    .get_header("Subject")
                                    .unwrap_or_else(|| "(no subject)".into());
                                let time = msg
                                    .internal_date
                                    .as_deref()
                                    .and_then(|d| d.parse::<i64>().ok())
                                    .and_then(|ms| chrono::DateTime::from_timestamp(ms / 1000, 0))
                                    .map(|utc| utc.with_timezone(&chrono::Local))
                                    .map(|local| local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
                                    .unwrap_or_default();
                                let direction = if is_sent { "sent" } else { "new" };
                                eprintln!(
                                    "[gmail:{}] {} ({direction}) {subject} — {sender}",
                                    self.display_connection_id(),
                                    time,
                                );
                                self.store_message(db, &msg)?;
                            }
                            Err(e) => {
                                warn!(message_id = %item.message.id, "failed to fetch: {e}");
                            }
                        }
                    }
                }

                // INBOX label removed → mark as archived locally
                if let Some(removed) = &record.labels_removed {
                    for item in removed {
                        if item.label_ids.iter().any(|l| l == "INBOX") {
                            let msg_id = format!("{}-{}", connection_id, item.message.id);
                            if db.mark_message_archived(&msg_id)? {
                                debug!(message_id = %msg_id, "marked archived (INBOX label removed)");
                            }
                        }
                    }
                }

                // INBOX label added to existing message → re-fetch to update is_archived
                if let Some(added) = &record.labels_added {
                    for item in added {
                        if item.label_ids.iter().any(|l| l == "INBOX")
                            && db.message_exists(&connection_id, &item.message.id)?
                        {
                            match api.get_message(&item.message.id).await {
                                Ok(msg) => {
                                    self.store_message(db, &msg)?;
                                    debug!(message_id = %item.message.id, "updated (INBOX label added)");
                                }
                                Err(e) => {
                                    warn!(message_id = %item.message.id, "failed to re-fetch: {e}");
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(new_id) = resp.history_id {
            db.set_sync_state(&self.config_id, "history_id", &new_id)?;
        }

        Ok(())
    }

    pub(crate) fn store_message(&self, db: &Database, msg: &GmailMessage) -> anyhow::Result<()> {
        let msg_id = msg.id.as_deref().unwrap_or("");
        let thread_id = msg.thread_id.as_deref().unwrap_or(msg_id);
        let from = msg.get_header("From").unwrap_or_default();
        let connection_id = self.display_connection_id();
        debug!(message_id = %msg_id, thread_id = %thread_id, from = %from, "storing message");

        let conv_id = format!("{}-{}", connection_id, thread_id);
        let subject = msg
            .get_header("Subject")
            .unwrap_or_else(|| "(no subject)".into());

        let conversation = Conversation {
            id: conv_id.clone(),
            connection_id: connection_id.clone(),
            connector: "gmail".into(),
            external_id: thread_id.to_string(),
            name: Some(subject),
            kind: ConversationKind::Thread,
            last_message_at: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000),
            unread_count: 0,
            is_muted: false,
            metadata: None,
        };
        db.upsert_conversation(&conversation)?;

        let sender_email = parse_email_address(&from);
        let sender_name = parse_email_name(&from);

        let text_body = msg.text_body();
        let html_body = msg.html_body();

        let body = match (text_body, &html_body) {
            (Some(text), _) if !looks_like_html(&text) => Some(text),
            (Some(text), _) => Some(html_to_markdown(&text)),
            (None, Some(html)) => Some(html_to_markdown(html)),
            (None, None) => msg.snippet.clone(),
        };

        let attachments = msg.file_attachments();
        let mut metadata = serde_json::Map::new();
        if html_body.is_some() {
            metadata.insert("has_html".into(), serde_json::json!(true));
            metadata.insert("snippet".into(), serde_json::json!(msg.snippet));
        }
        if !attachments.is_empty() {
            metadata.insert("attachments".into(), serde_json::json!(attachments));
        }
        let metadata = if metadata.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(metadata))
        };

        let message = Message {
            id: format!("{}-{}", connection_id, msg_id),
            conversation_id: conv_id,
            connection_id: connection_id.clone(),
            connector: "gmail".into(),
            external_id: msg_id.to_string(),
            sender: sender_email,
            sender_name: Some(sender_name),
            body,
            timestamp: msg
                .internal_date
                .as_deref()
                .and_then(|d| d.parse().ok())
                .map(|ms: i64| ms / 1000)
                .unwrap_or(0),
            synced_at: None,
            is_archived: !msg
                .label_ids
                .as_ref()
                .is_some_and(|labels| labels.iter().any(|l| l == "INBOX")),
            reply_to_id: msg
                .get_header("In-Reply-To")
                .map(|v| format!("{}-{v}", connection_id)),
            media_type: None,
            metadata,
            context_id: Some(format!("{}-thread-{}", connection_id, thread_id)),
            context: None,
        };
        db.upsert_message(&message)?;
        Ok(())
    }
}
