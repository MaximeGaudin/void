use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

use crate::api::{repo_full_name_from_url, GhNotification, GitHubClient};

const NOTIFICATIONS_CURSOR_KEY: &str = "github_notifications_since";
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    token: &str,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = GitHubClient::new(token);

    info!(connection_id, "running initial GitHub sync");
    if let Err(e) = poll_github(&client, db, connection_id, &cancel).await {
        error!(connection_id, error = %e, "initial GitHub sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "GitHub sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "GitHub sync was idle, catching up"
                    );
                    void_core::status!(
                        "[github:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling GitHub");
                }
                if let Err(e) = poll_github(&client, db, connection_id, &cancel).await {
                    error!(connection_id, error = %e, "GitHub poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

async fn poll_github(
    client: &GitHubClient,
    db: &Arc<Database>,
    connection_id: &str,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    if cancel.is_cancelled() {
        return Ok(());
    }

    sync_review_requests(client, db, connection_id).await?;
    if cancel.is_cancelled() {
        return Ok(());
    }
    sync_notifications(client, db, connection_id).await?;
    Ok(())
}

async fn sync_review_requests(
    client: &GitHubClient,
    db: &Arc<Database>,
    connection_id: &str,
) -> anyhow::Result<()> {
    let prs = client.review_requested_prs().await?;
    for pr in prs {
        let repo_full_name = repo_full_name_from_url(&pr.repository_url)
            .unwrap_or_else(|| "unknown/unknown".to_string());
        let (conv_id, conv_external_id) =
            upsert_repo_conversation(db, connection_id, &repo_full_name)?;

        let external_id = format!(
            "github_{connection_id}_review_{repo_full_name}_{}",
            pr.number
        );
        if db.message_exists(connection_id, &external_id)? {
            continue;
        }

        let body = format!(
            "Review requested: {}\n{}\nAuthor: @{}",
            pr.title, pr.html_url, pr.user.login
        );
        let timestamp = parse_github_timestamp(&pr.updated_at);

        let msg = Message {
            id: format!("{connection_id}-review-{}", pr.id),
            conversation_id: conv_id,
            connection_id: connection_id.to_string(),
            connector: "github".to_string(),
            external_id: external_id.clone(),
            sender: pr.user.login.clone(),
            sender_name: Some(pr.user.login.clone()),
            sender_avatar_url: None,
            body: Some(body.clone()),
            timestamp,
            synced_at: Some(Utc::now().timestamp()),
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: Some(serde_json::json!({
                "kind": "review_request",
                "pr_number": pr.number,
                "repo": repo_full_name,
                "html_url": pr.html_url,
            })),
            context_id: Some(conv_external_id),
            context: None,
        };
        db.upsert_message(&msg)?;
        log_new_message(
            connection_id,
            &repo_full_name,
            "review request",
            &pr.user.login,
            &pr.title,
        );
    }

    Ok(())
}

async fn sync_notifications(
    client: &GitHubClient,
    db: &Arc<Database>,
    connection_id: &str,
) -> anyhow::Result<()> {
    let since = db
        .get_sync_state(connection_id, NOTIFICATIONS_CURSOR_KEY)?
        .filter(|value| !value.is_empty());

    let notifications = client.notifications(since.as_deref()).await?;
    let mut latest_updated: Option<String> = since;

    for notification in notifications {
        // Advance the cursor for every notification (even filtered-out ones) so a
        // batch that contains only irrelevant notifications still moves `since`
        // forward instead of re-fetching the same window every poll.
        update_latest_cursor(&mut latest_updated, &notification.updated_at);

        if !should_include_notification(&notification) {
            continue;
        }

        let external_id = format!("github_{connection_id}_notification_{}", notification.id);
        if db.message_exists(connection_id, &external_id)? {
            continue;
        }

        let repo_full_name = notification
            .repository
            .as_ref()
            .map(|repo| repo.full_name.clone())
            .unwrap_or_else(|| "github/mentions".to_string());

        let (conv_id, conv_external_id) =
            upsert_repo_conversation(db, connection_id, &repo_full_name)?;

        let reason_label = notification.notification_reason_label();
        let subject_url = notification.subject.url.clone().unwrap_or_default();
        let body = if subject_url.is_empty() {
            format!(
                "{}: {}\nReason: {}",
                reason_label, notification.subject.title, notification.reason
            )
        } else {
            format!(
                "{}: {}\nReason: {}\n{}",
                reason_label, notification.subject.title, notification.reason, subject_url
            )
        };

        let timestamp = parse_github_timestamp(&notification.updated_at);
        let sender = notification
            .repository
            .as_ref()
            .map(|repo| repo.full_name.clone())
            .unwrap_or_else(|| "github".to_string());

        let msg = Message {
            id: format!("{connection_id}-notification-{}", notification.id),
            conversation_id: conv_id,
            connection_id: connection_id.to_string(),
            connector: "github".to_string(),
            external_id: external_id.clone(),
            sender: sender.clone(),
            sender_name: Some(sender.clone()),
            sender_avatar_url: None,
            body: Some(body.clone()),
            timestamp,
            synced_at: Some(Utc::now().timestamp()),
            is_archived: false,
            reply_to_id: None,
            media_type: None,
            metadata: Some(serde_json::json!({
                "kind": notification.reason,
                "subject_type": notification.subject.subject_type,
                "repo": repo_full_name,
                "subject_url": notification.subject.url,
            })),
            context_id: Some(conv_external_id),
            context: None,
        };
        db.upsert_message(&msg)?;
        log_new_message(
            connection_id,
            &repo_full_name,
            reason_label,
            &sender,
            &notification.subject.title,
        );
    }

    if let Some(latest) = latest_updated {
        db.set_sync_state(connection_id, NOTIFICATIONS_CURSOR_KEY, &latest)?;
    }

    Ok(())
}

fn should_include_notification(notification: &GhNotification) -> bool {
    match notification.reason.as_str() {
        "mention" => true,
        "author" => notification.subject.subject_type == "PullRequest",
        _ => false,
    }
}

impl GhNotification {
    fn notification_reason_label(&self) -> &'static str {
        match self.reason.as_str() {
            "mention" => "Mention",
            "author" => "PR comment",
            _ => "Notification",
        }
    }
}

fn upsert_repo_conversation(
    db: &Arc<Database>,
    connection_id: &str,
    repo_full_name: &str,
) -> anyhow::Result<(String, String)> {
    let conv_external_id = format!("github_{connection_id}_{repo_full_name}");
    let conv_id = format!("{connection_id}-{repo_full_name}");
    let conv = Conversation {
        id: conv_id.clone(),
        connection_id: connection_id.to_string(),
        connector: "github".to_string(),
        external_id: conv_external_id.clone(),
        name: Some(repo_full_name.to_string()),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;
    Ok((conv_id, conv_external_id))
}

fn parse_github_timestamp(value: &str) -> i64 {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|_| Utc::now().timestamp())
}

fn update_latest_cursor(latest: &mut Option<String>, candidate: &str) {
    match latest {
        Some(current) if candidate <= current.as_str() => {}
        slot => *slot = Some(candidate.to_string()),
    }
}

fn log_new_message(connection_id: &str, repo: &str, kind: &str, sender: &str, preview: &str) {
    let preview = preview.chars().take(80).collect::<String>();
    eprintln!("[github:{connection_id}] (new) {repo} — {kind} — {sender}: {preview}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_notification(reason: &str, subject_type: &str) -> GhNotification {
        GhNotification {
            id: "1".to_string(),
            reason: reason.to_string(),
            updated_at: "2024-01-02T12:00:00Z".to_string(),
            subject: crate::api::GhSubject {
                title: "Test PR".to_string(),
                subject_type: subject_type.to_string(),
                url: Some("https://api.github.com/repos/o/r/pulls/1".to_string()),
            },
            repository: Some(crate::api::GhRepository {
                full_name: "owner/repo".to_string(),
            }),
        }
    }

    #[test]
    fn includes_mention_notifications() {
        let n = make_notification("mention", "Issue");
        assert!(should_include_notification(&n));
    }

    #[test]
    fn includes_author_notifications_for_pull_requests_only() {
        let pr = make_notification("author", "PullRequest");
        assert!(should_include_notification(&pr));

        let issue = make_notification("author", "Issue");
        assert!(!should_include_notification(&issue));
    }

    #[test]
    fn ignores_other_notification_reasons() {
        let n = make_notification("review_requested", "PullRequest");
        assert!(!should_include_notification(&n));
    }

    #[test]
    fn update_latest_cursor_keeps_newest_timestamp() {
        let mut latest = Some("2024-01-01T00:00:00Z".to_string());
        update_latest_cursor(&mut latest, "2024-01-02T00:00:00Z");
        assert_eq!(latest.as_deref(), Some("2024-01-02T00:00:00Z"));

        update_latest_cursor(&mut latest, "2023-12-31T00:00:00Z");
        assert_eq!(latest.as_deref(), Some("2024-01-02T00:00:00Z"));
    }

    #[test]
    fn repo_full_name_parsed_for_search_issue() {
        use crate::api::GhSearchIssue;
        let issue = GhSearchIssue {
            id: 1,
            number: 7,
            title: "Fix".to_string(),
            html_url: "https://github.com/o/r/pull/7".to_string(),
            repository_url: "https://api.github.com/repos/o/r".to_string(),
            user: crate::api::GhSearchUser {
                login: "alice".to_string(),
            },
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };
        assert_eq!(
            repo_full_name_from_url(&issue.repository_url).as_deref(),
            Some("o/r")
        );
    }
}
