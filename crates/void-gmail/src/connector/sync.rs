use tracing::{debug, info, warn};

use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

use crate::api::GmailMessage;

use super::compose::{html_to_markdown, looks_like_html, parse_email_address, parse_email_name};
use super::GmailConnector;

impl GmailConnector {
    pub(crate) async fn initial_sync(&self, db: &Database) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        info!(config_id = %self.config_id, "starting Gmail initial sync");

        let profile = api.get_profile().await?;
        if let Some(email) = &profile.email_address {
            *self.my_email.lock().expect("mutex") = Some(email.clone());
        }

        if let Some(history_id) = &profile.history_id {
            db.set_sync_state(&self.config_id, "history_id", history_id)?;
        }

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

    pub(crate) async fn incremental_sync(&self, db: &Database) -> anyhow::Result<()> {
        let Some(history_id) = db.get_sync_state(&self.config_id, "history_id")? else {
            debug!("no history_id, skipping incremental sync");
            return Ok(());
        };

        let api = self.get_client().await?;
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
                                let subject = msg
                                    .get_header("Subject")
                                    .unwrap_or_else(|| "(no subject)".into());
                                let direction = if is_sent { "sent" } else { "new" };
                                eprintln!(
                                    "[gmail:{}] {}: {} — {}",
                                    self.display_account_id(),
                                    direction,
                                    from,
                                    subject
                                );
                                self.store_message(db, &msg)?;
                            }
                            Err(e) => {
                                warn!(message_id = %item.message.id, "failed to fetch: {e}");
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
        let account_id = self.display_account_id();
        debug!(message_id = %msg_id, thread_id = %thread_id, from = %from, "storing message");

        let conv_id = format!("{}-{}", account_id, thread_id);
        let subject = msg
            .get_header("Subject")
            .unwrap_or_else(|| "(no subject)".into());

        let conversation = Conversation {
            id: conv_id.clone(),
            account_id: account_id.clone(),
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

        let metadata = if html_body.is_some() {
            Some(serde_json::json!({ "has_html": true, "snippet": msg.snippet }))
        } else {
            None
        };

        let message = Message {
            id: format!("{}-{}", account_id, msg_id),
            conversation_id: conv_id,
            account_id: account_id.clone(),
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
                .map(|v| format!("{}-{v}", account_id)),
            media_type: None,
            metadata,
            context_id: Some(format!("{}-thread-{}", account_id, thread_id)),
            context: None,
        };
        db.upsert_message(&message)?;
        Ok(())
    }
}
