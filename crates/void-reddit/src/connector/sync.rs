use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{sanitize_subreddit, RedditClient, RedditPost};

const REDDIT_BASE: &str = "https://www.reddit.com";
const POSTS_PER_SUBREDDIT: u32 = 100;

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack/HN).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    client_id: &str,
    client_secret: &str,
    subreddits: &[String],
    keywords: &[String],
    min_score: u32,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = RedditClient::new(client_id, client_secret);

    for subreddit in subreddits {
        ensure_subreddit_conversation(db, connection_id, subreddit)?;
    }

    info!(connection_id, "running initial Reddit sync");
    if let Err(e) = poll_subreddits(
        &client,
        db,
        connection_id,
        subreddits,
        keywords,
        min_score,
        &cancel,
        true,
    )
    .await
    {
        error!(connection_id, error = %e, "initial Reddit sync failed");
    }

    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    interval.tick().await;
    let mut last_poll = SystemTime::now();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                info!(connection_id, "Reddit sync cancelled");
                break;
            }
            _ = interval.tick() => {
                let elapsed = last_poll.elapsed().unwrap_or_default();
                if elapsed > IDLE_THRESHOLD {
                    warn!(
                        connection_id,
                        idle_secs = elapsed.as_secs(),
                        "Reddit sync was idle, catching up"
                    );
                    void_core::status!(
                        "[reddit:{connection_id}] sync idle for {}s, catching up",
                        elapsed.as_secs(),
                    );
                } else {
                    info!(connection_id, "polling Reddit");
                }
                if let Err(e) = poll_subreddits(
                    &client,
                    db,
                    connection_id,
                    subreddits,
                    keywords,
                    min_score,
                    &cancel,
                    elapsed > IDLE_THRESHOLD,
                )
                .await
                {
                    error!(connection_id, error = %e, "Reddit poll error");
                }
                last_poll = SystemTime::now();
            }
        }
    }
    Ok(())
}

fn ensure_subreddit_conversation(
    db: &Arc<Database>,
    connection_id: &str,
    subreddit: &str,
) -> anyhow::Result<()> {
    let sub = sanitize_subreddit(subreddit);
    let conv_external_id = format!("reddit_{connection_id}_{sub}");
    let conv = Conversation {
        id: format!("{connection_id}-{sub}"),
        connection_id: connection_id.to_string(),
        connector: "reddit".to_string(),
        external_id: conv_external_id,
        name: Some(format!("r/{sub}")),
        kind: ConversationKind::Channel,
        last_message_at: None,
        unread_count: 0,
        is_muted: false,
        metadata: None,
    };
    db.upsert_conversation(&conv)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn poll_subreddits(
    client: &RedditClient,
    db: &Arc<Database>,
    connection_id: &str,
    subreddits: &[String],
    keywords: &[String],
    min_score: u32,
    cancel: &CancellationToken,
    show_progress: bool,
) -> anyhow::Result<()> {
    if subreddits.is_empty() {
        warn!(connection_id, "no subreddits configured, skipping poll");
        return Ok(());
    }

    let mut progress = show_progress.then(|| {
        BackfillProgress::new(&format!("reddit:{connection_id}"), "posts")
            .with_secondary("ingested")
    });

    for subreddit in subreddits {
        if cancel.is_cancelled() {
            break;
        }

        let sub = sanitize_subreddit(subreddit);
        let conv_id = format!("{connection_id}-{sub}");

        let posts = match client.subreddit_hot(&sub, POSTS_PER_SUBREDDIT).await {
            Ok(posts) => posts,
            Err(e) => {
                warn!(subreddit = %sub, error = %e, "failed to fetch subreddit posts");
                continue;
            }
        };

        for post in posts {
            if cancel.is_cancelled() {
                break;
            }

            if let Some(ref mut p) = progress {
                p.inc(1);
            }

            let external_id = format!("reddit_{connection_id}_{}", post.id);
            if db.message_exists(connection_id, &external_id)? {
                continue;
            }

            if !matches_filters(&post, keywords, min_score) {
                continue;
            }

            let msg = build_message(&post, connection_id, &conv_id, &sub);
            db.upsert_message(&msg)?;

            let when_str = chrono::DateTime::from_timestamp(msg.timestamp, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            let title = post.title.as_deref().unwrap_or("(untitled)");
            let author = post.author.as_deref().unwrap_or("unknown");
            eprintln!("[reddit:{connection_id}] {when_str} (new) r/{sub} — {author}: {title}");

            if let Some(ref mut p) = progress {
                p.inc_secondary(1);
            }
        }
    }

    if let Some(p) = progress {
        p.finish();
    }

    if !cancel.is_cancelled() {
        db.set_sync_state(
            connection_id,
            "reddit_last_sync",
            &chrono::Utc::now().timestamp().to_string(),
        )?;
    }

    Ok(())
}

pub(crate) fn matches_filters(post: &RedditPost, keywords: &[String], min_score: u32) -> bool {
    let score = post.score.unwrap_or(0).max(0) as u32;
    if score < min_score {
        return false;
    }

    if keywords.is_empty() {
        return true;
    }

    let title = post.title.as_deref().unwrap_or("").to_lowercase();
    keywords.iter().any(|kw| title.contains(kw.as_str()))
}

pub(crate) fn build_message(
    post: &RedditPost,
    connection_id: &str,
    conv_id: &str,
    subreddit: &str,
) -> Message {
    let post_id = &post.id;
    let title = post.title.as_deref().unwrap_or("(untitled)");
    let author = post.author.as_deref().unwrap_or("[deleted]");
    let score = post.score.unwrap_or(0).max(0) as u32;
    let url = post.url.as_deref().unwrap_or("").to_string();
    let permalink = post.permalink.as_deref().unwrap_or("");
    let reddit_url = if permalink.starts_with("http") {
        permalink.to_string()
    } else {
        format!("{REDDIT_BASE}{permalink}")
    };
    let comments = post.num_comments.unwrap_or(0);
    let upvote_ratio = post.upvote_ratio.unwrap_or(0.0);

    let body = if url.is_empty() || url == reddit_url {
        format!("{title}\n{reddit_url}\n{score} upvotes | {comments} comments")
    } else {
        format!("{title}\n{url}\n{reddit_url}\n{score} upvotes | {comments} comments")
    };

    let metadata = serde_json::json!({
        "reddit_id": post_id,
        "subreddit": subreddit,
        "score": score,
        "url": url,
        "reddit_url": reddit_url,
        "num_comments": comments,
        "upvote_ratio": upvote_ratio,
    });

    let timestamp = post
        .created_utc
        .map(|ts| ts as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    Message {
        id: format!("{connection_id}-{post_id}"),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "reddit".to_string(),
        external_id: format!("reddit_{connection_id}_{post_id}"),
        sender: author.to_string(),
        sender_name: Some(author.to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp,
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(metadata),
        context_id: None,
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_post(id: &str, title: &str, score: i32) -> RedditPost {
        RedditPost {
            id: id.to_string(),
            title: Some(title.to_string()),
            author: Some("author".to_string()),
            score: Some(score),
            url: Some("https://example.com".to_string()),
            permalink: Some("/r/rust/comments/abc/hello/".to_string()),
            num_comments: Some(10),
            upvote_ratio: Some(0.95),
            created_utc: Some(1_700_000_000.0),
            subreddit: Some("rust".to_string()),
            selftext: None,
        }
    }

    #[test]
    fn matches_keyword_case_insensitive() {
        let post = make_post("1", "Rust is Amazing", 200);
        let keywords = vec!["rust".to_string()];
        assert!(matches_filters(&post, &keywords, 0));
    }

    #[test]
    fn rejects_below_min_score() {
        let post = make_post("1", "Rust is Amazing", 50);
        let keywords = vec!["rust".to_string()];
        assert!(!matches_filters(&post, &keywords, 100));
    }

    #[test]
    fn rejects_non_matching_keyword() {
        let post = make_post("1", "Python is Great", 200);
        let keywords = vec!["rust".to_string()];
        assert!(!matches_filters(&post, &keywords, 0));
    }

    #[test]
    fn empty_keywords_matches_all_posts_above_threshold() {
        let post = make_post("1", "Anything Goes", 200);
        assert!(matches_filters(&post, &[], 0));
    }

    #[test]
    fn missing_score_defaults_to_zero() {
        let mut post = make_post("1", "No score", 0);
        post.score = None;
        assert!(matches_filters(&post, &[], 0));
        assert!(!matches_filters(&post, &[], 1));
    }

    #[test]
    fn build_message_includes_all_fields() {
        let post = make_post("abc123", "Cool Rust Tool", 350);
        let msg = build_message(&post, "reddit", "reddit-rust", "rust");
        assert_eq!(msg.id, "reddit-abc123");
        assert_eq!(msg.conversation_id, "reddit-rust");
        assert_eq!(msg.external_id, "reddit_reddit_abc123");
        assert_eq!(msg.sender, "author");
        assert!(msg.body.as_ref().unwrap().contains("Cool Rust Tool"));
        assert!(msg.body.as_ref().unwrap().contains("350 upvotes"));
        assert!(msg.body.as_ref().unwrap().contains("https://example.com"));
        let meta = msg.metadata.unwrap();
        assert_eq!(meta["reddit_id"], "abc123");
        assert_eq!(meta["subreddit"], "rust");
        assert_eq!(meta["score"], 350);
        assert_eq!(meta["num_comments"], 10);
    }

    #[test]
    fn sanitize_subreddit_for_conversation_ids() {
        assert_eq!(sanitize_subreddit("r/Rust"), "rust");
        assert_eq!(sanitize_subreddit("start-ups!"), "startups");
    }
}
