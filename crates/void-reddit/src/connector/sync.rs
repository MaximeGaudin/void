use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};
use void_core::progress::BackfillProgress;

use crate::api::{sanitize_subreddit, RedditClient, RedditComment, RedditPost};

const REDDIT_BASE: &str = "https://www.reddit.com";
const POSTS_PER_SUBREDDIT: u32 = 100;
const COMMENT_FETCH_DELAY: Duration = Duration::from_millis(1100);

/// Wall-clock threshold to detect hibernation gaps (same rationale as Gmail/Slack/HN).
const IDLE_THRESHOLD: Duration = Duration::from_secs(3 * 60);

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_sync(
    db: &Arc<Database>,
    connection_id: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: Option<&str>,
    subreddits: &[String],
    keywords: &[String],
    min_score: u32,
    poll_interval_secs: u64,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = RedditClient::with_refresh_token(
        client_id,
        client_secret,
        refresh_token.map(str::to_string),
    );
    let comment_sync_enabled = refresh_token.is_some();

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
        comment_sync_enabled,
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
                    comment_sync_enabled,
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
    comment_sync_enabled: bool,
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
            let already_exists = db.message_exists(connection_id, &external_id)?;

            if !already_exists {
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

            if comment_sync_enabled
                && (already_exists || matches_filters(&post, keywords, min_score))
            {
                if let Err(e) = sync_post_comments(client, db, connection_id, &post, &sub).await {
                    warn!(post_id = %post.id, error = %e, "failed to sync Reddit post comments");
                }
                tokio::time::sleep(COMMENT_FETCH_DELAY).await;
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

async fn sync_post_comments(
    client: &RedditClient,
    db: &Arc<Database>,
    connection_id: &str,
    post: &RedditPost,
    subreddit: &str,
) -> anyhow::Result<()> {
    let (thread_conv, post_body_msg) =
        build_post_thread_conversation(post, connection_id, subreddit);
    db.upsert_conversation(&thread_conv)?;
    db.upsert_message(&post_body_msg)?;

    let (_, comments) = client.get_post_comments(&post.id, "new", 200, 3).await?;

    for comment in comments {
        let external_id = format!("reddit_{connection_id}_comment_{}", comment.id);
        if db.message_exists(connection_id, &external_id)? {
            continue;
        }
        let msg = build_comment_message(
            &comment,
            connection_id,
            &thread_conv.id,
            subreddit,
            &post.id,
        );
        db.upsert_message(&msg)?;
    }

    Ok(())
}

pub(crate) fn build_post_thread_conversation(
    post: &RedditPost,
    connection_id: &str,
    subreddit: &str,
) -> (Conversation, Message) {
    let post_id = &post.id;
    let conv_id = format!("{connection_id}-post-{post_id}");
    let conv_external_id = format!("reddit_{connection_id}_post_{post_id}");
    let title = post.title.as_deref().unwrap_or("(untitled)");
    let permalink = post.permalink.as_deref().unwrap_or("");
    let reddit_url = if permalink.starts_with("http") {
        permalink.to_string()
    } else {
        format!("{REDDIT_BASE}{permalink}")
    };

    let conv = Conversation {
        id: conv_id.clone(),
        connection_id: connection_id.to_string(),
        connector: "reddit".to_string(),
        external_id: conv_external_id.clone(),
        name: Some(title.to_string()),
        kind: ConversationKind::Thread,
        last_message_at: post.created_utc.map(|ts| ts as i64),
        unread_count: 0,
        is_muted: false,
        metadata: Some(serde_json::json!({
            "reddit_id": post_id,
            "subreddit": subreddit,
            "permalink": reddit_url,
        })),
    };

    let post_body_msg = build_post_body_message(post, connection_id, &conv_id, subreddit);
    (conv, post_body_msg)
}

pub(crate) fn build_post_body_message(
    post: &RedditPost,
    connection_id: &str,
    conv_id: &str,
    subreddit: &str,
) -> Message {
    let post_id = &post.id;
    let title = post.title.as_deref().unwrap_or("(untitled)");
    let author = post.author.as_deref().unwrap_or("[deleted]");
    let selftext = post.selftext.as_deref().unwrap_or("").trim();
    let permalink = post.permalink.as_deref().unwrap_or("");
    let reddit_url = if permalink.starts_with("http") {
        permalink.to_string()
    } else {
        format!("{REDDIT_BASE}{permalink}")
    };

    let body = if selftext.is_empty() {
        format!("{title}\n{reddit_url}")
    } else {
        format!("{title}\n\n{selftext}\n\n{reddit_url}")
    };

    let timestamp = post
        .created_utc
        .map(|ts| ts as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    Message {
        id: conv_id.to_string(),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "reddit".to_string(),
        external_id: format!("reddit_{connection_id}_postbody_{post_id}"),
        sender: author.to_string(),
        sender_name: Some(author.to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp,
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(serde_json::json!({
            "reddit_id": post_id,
            "subreddit": subreddit,
            "permalink": reddit_url,
            "source": "reddit_post_body",
        })),
        context_id: None,
        context: None,
    }
}

pub(crate) fn build_comment_message(
    comment: &RedditComment,
    connection_id: &str,
    conv_id: &str,
    subreddit: &str,
    post_id: &str,
) -> Message {
    let author = comment.author.as_deref().unwrap_or("[deleted]");
    let body = comment.body.as_deref().unwrap_or("[removed]").to_string();
    let score = comment.score.unwrap_or(0).max(0) as u32;
    let timestamp = comment
        .created_utc
        .map(|ts| ts as i64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    Message {
        id: format!("{connection_id}-comment-{}", comment.id),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "reddit".to_string(),
        external_id: format!("reddit_{connection_id}_comment_{}", comment.id),
        sender: author.to_string(),
        sender_name: Some(author.to_string()),
        sender_avatar_url: None,
        body: Some(body),
        timestamp,
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(serde_json::json!({
            "reddit_id": comment.id,
            "post_id": post_id,
            "subreddit": subreddit,
            "parent_id": comment.parent_id,
            "score": score,
            "depth": comment.depth,
            "source": "reddit_comment",
        })),
        context_id: None,
        context: None,
    }
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
        is_saved: false,
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
            selftext: Some("post body".to_string()),
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
    fn build_post_thread_conversation_creates_thread_with_metadata() {
        let post = make_post("abc123", "Thread title", 100);
        let (conv, msg) = build_post_thread_conversation(&post, "reddit", "rust");
        assert_eq!(conv.id, "reddit-post-abc123");
        assert_eq!(conv.external_id, "reddit_reddit_post_abc123");
        assert_eq!(conv.kind, ConversationKind::Thread);
        assert_eq!(conv.metadata.as_ref().unwrap()["subreddit"], "rust");
        assert!(conv.metadata.as_ref().unwrap()["permalink"]
            .as_str()
            .unwrap()
            .contains("reddit.com"));
        assert_eq!(msg.external_id, "reddit_reddit_postbody_abc123");
        assert!(msg.body.as_ref().unwrap().contains("Thread title"));
        assert!(msg.body.as_ref().unwrap().contains("post body"));
    }

    #[test]
    fn build_comment_message_sets_parent_metadata() {
        let comment = RedditComment {
            id: "c1".to_string(),
            author: Some("user1".to_string()),
            body: Some("nice".to_string()),
            score: Some(5),
            parent_id: Some("t3_abc123".to_string()),
            link_id: Some("t3_abc123".to_string()),
            created_utc: Some(1_700_000_001.0),
            depth: Some(0),
            replies: crate::api::RedditReplies::Empty,
        };
        let msg = build_comment_message(&comment, "reddit", "reddit-post-abc123", "rust", "abc123");
        assert_eq!(msg.external_id, "reddit_reddit_comment_c1");
        assert_eq!(msg.sender, "user1");
        assert_eq!(msg.body.as_deref(), Some("nice"));
        let meta = msg.metadata.unwrap();
        assert_eq!(meta["parent_id"], "t3_abc123");
        assert_eq!(meta["post_id"], "abc123");
    }

    #[test]
    fn sanitize_subreddit_for_conversation_ids() {
        assert_eq!(sanitize_subreddit("r/Rust"), "rust");
        assert_eq!(sanitize_subreddit("start-ups!"), "startups");
    }
}
