//! Sync comments on the connected account's LinkedIn posts (Unipile Posts API).

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use void_core::db::Database;
use void_core::models::{Conversation, ConversationKind, Message};

use crate::api::{AccountOwnerProfile, UnipileClient, UnipileComment, UnipilePost};

use super::extract;
use super::profiles::ProfileCache;
use super::sync::backfill_cutoff_iso;

const POST_PAGE_LIMIT: u32 = 50;
const COMMENT_PAGE_LIMIT: u32 = 50;

struct PostsSyncCtx<'a> {
    client: &'a UnipileClient,
    account_id: &'a str,
    db: &'a Arc<Database>,
    connection_id: &'a str,
    owner: &'a AccountOwnerProfile,
    cutoff_ts: i64,
    profile_cache: &'a mut ProfileCache,
    cancel: &'a CancellationToken,
}

pub(super) async fn sync_posts_backfill(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_days: u64,
    profile_cache: &mut ProfileCache,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let cutoff_iso = backfill_cutoff_iso(backfill_days);
    let cutoff_ts = chrono::DateTime::parse_from_rfc3339(&cutoff_iso)
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let owner = client
        .get_account_owner_profile(account_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if owner.provider_id.is_empty() {
        warn!(
            connection_id,
            "LinkedIn posts sync skipped: empty provider_id from users/me"
        );
        return Ok(());
    }

    sync_posts_for_user(&mut PostsSyncCtx {
        client,
        account_id,
        db,
        connection_id,
        owner: &owner,
        cutoff_ts,
        profile_cache,
        cancel,
    })
    .await
}

pub(super) async fn sync_posts_incremental(
    client: &UnipileClient,
    account_id: &str,
    db: &Arc<Database>,
    connection_id: &str,
    backfill_days: u64,
    profile_cache: &mut ProfileCache,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let cutoff_iso = backfill_cutoff_iso(backfill_days);
    let cutoff_ts = chrono::DateTime::parse_from_rfc3339(&cutoff_iso)
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let owner = match client.get_account_owner_profile(account_id).await {
        Ok(o) => o,
        Err(e) => {
            warn!(connection_id, error = %e, "LinkedIn posts incremental: users/me failed");
            return Ok(());
        }
    };
    if owner.provider_id.is_empty() {
        return Ok(());
    }

    sync_posts_for_user(&mut PostsSyncCtx {
        client,
        account_id,
        db,
        connection_id,
        owner: &owner,
        cutoff_ts,
        profile_cache,
        cancel,
    })
    .await
}

async fn sync_posts_for_user(ctx: &mut PostsSyncCtx<'_>) -> anyhow::Result<()> {
    let mut post_cursor: Option<String> = None;
    let mut posts_seen = 0u64;

    loop {
        if ctx.cancel.is_cancelled() {
            break;
        }

        let page = ctx
            .client
            .list_user_posts(
                ctx.account_id,
                &ctx.owner.provider_id,
                post_cursor.as_deref(),
                POST_PAGE_LIMIT,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut stop_pagination = false;
        for post in &page.items {
            if ctx.cancel.is_cancelled() {
                break;
            }
            if post.id.is_empty() || post.social_id.is_empty() {
                continue;
            }

            let post_ts = post_timestamp(post);
            if post_ts < ctx.cutoff_ts {
                stop_pagination = true;
                continue;
            }

            if post.comment_counter.unwrap_or(0) <= 0 {
                continue;
            }

            sync_post_comments(ctx, post).await?;
            posts_seen += 1;
        }

        if stop_pagination {
            break;
        }
        post_cursor = page.cursor;
        if post_cursor.is_none() {
            break;
        }
    }

    debug!(
        connection_id = ctx.connection_id,
        posts_with_comments = posts_seen,
        "LinkedIn post comments sync pass done"
    );
    Ok(())
}

async fn sync_post_comments(ctx: &mut PostsSyncCtx<'_>, post: &UnipilePost) -> anyhow::Result<()> {
    let (conv_id, conv_external_id) = post_conversation_ids(ctx.connection_id, post);
    let conv = post_to_conversation(ctx.connection_id, post, &conv_id, &conv_external_id);
    ctx.db.upsert_conversation(&conv)?;

    let mut cursor: Option<String> = None;
    loop {
        if ctx.cancel.is_cancelled() {
            break;
        }

        let page = ctx
            .client
            .list_post_comments(
                ctx.account_id,
                &post.social_id,
                cursor.as_deref(),
                None,
                COMMENT_PAGE_LIMIT,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for comment in &page.items {
            if ctx.cancel.is_cancelled() {
                break;
            }
            if comment.id.is_empty() {
                continue;
            }
            let ts = comment_timestamp(comment);
            if ts < ctx.cutoff_ts {
                continue;
            }
            ingest_comment(ctx, post, &conv_id, &conv_external_id, comment).await?;

            if comment.reply_counter.unwrap_or(0) > 0 {
                sync_comment_replies(ctx, post, &conv_id, &conv_external_id, comment).await?;
            }
        }

        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }

    Ok(())
}

async fn sync_comment_replies(
    ctx: &mut PostsSyncCtx<'_>,
    post: &UnipilePost,
    conv_id: &str,
    conv_external_id: &str,
    parent: &UnipileComment,
) -> anyhow::Result<()> {
    let mut cursor: Option<String> = None;
    loop {
        if ctx.cancel.is_cancelled() {
            break;
        }

        let page = ctx
            .client
            .list_post_comments(
                ctx.account_id,
                &post.social_id,
                cursor.as_deref(),
                Some(&parent.id),
                COMMENT_PAGE_LIMIT,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for comment in &page.items {
            if comment.id.is_empty() {
                continue;
            }
            let ts = comment_timestamp(comment);
            if ts < ctx.cutoff_ts {
                continue;
            }
            ingest_comment(ctx, post, conv_id, conv_external_id, comment).await?;
        }

        cursor = page.cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(())
}

async fn ingest_comment(
    ctx: &mut PostsSyncCtx<'_>,
    post: &UnipilePost,
    conv_id: &str,
    conv_external_id: &str,
    comment: &UnipileComment,
) -> anyhow::Result<()> {
    let msg_external_id = format!("linkedin_{}_comment_{}", ctx.connection_id, comment.id);
    if ctx.db.message_exists(ctx.connection_id, &msg_external_id)? {
        return Ok(());
    }

    let provider_id = comment.author_provider_id();
    let profile = ctx
        .profile_cache
        .resolve_provider(ctx.client, ctx.account_id, provider_id, None)
        .await;

    let display_name = if profile.display_name == provider_id {
        comment.author_display_name()
    } else {
        profile.display_name
    };

    let void_msg = comment_to_void(CommentVoidInput {
        connection_id: ctx.connection_id,
        conv_id,
        conv_external_id,
        post,
        comment,
        display_name: &display_name,
        profile_url: &profile.profile_url,
        avatar_url: &profile.avatar_url,
    });

    ctx.db.upsert_message(&void_msg)?;

    let preview: String = void_msg
        .body
        .as_deref()
        .unwrap_or("[comment]")
        .chars()
        .take(80)
        .collect();
    let time = chrono::DateTime::from_timestamp(void_msg.timestamp, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S %Z")
                .to_string()
        })
        .unwrap_or_else(|| void_msg.timestamp.to_string());
    let direction = if void_msg.sender == ctx.owner.provider_id {
        "sent"
    } else {
        "new"
    };

    void_core::status!(
        "[linkedin:{}] {time} ({direction}) {} — {}: {preview}",
        ctx.connection_id,
        post.display_label(),
        display_name
    );

    Ok(())
}

struct CommentVoidInput<'a> {
    connection_id: &'a str,
    conv_id: &'a str,
    conv_external_id: &'a str,
    post: &'a UnipilePost,
    comment: &'a UnipileComment,
    display_name: &'a str,
    profile_url: &'a Option<String>,
    avatar_url: &'a Option<String>,
}

fn post_conversation_ids(connection_id: &str, post: &UnipilePost) -> (String, String) {
    let conv_id = format!("{connection_id}-post-{}", post.id);
    let conv_external_id = format!("linkedin_{connection_id}_post_{}", post.id);
    (conv_id, conv_external_id)
}

fn post_to_conversation(
    connection_id: &str,
    post: &UnipilePost,
    conv_id: &str,
    conv_external_id: &str,
) -> Conversation {
    Conversation {
        id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "linkedin".to_string(),
        external_id: conv_external_id.to_string(),
        name: Some(post.display_label()),
        kind: ConversationKind::Thread,
        last_message_at: Some(post_timestamp(post)),
        unread_count: 0,
        is_muted: false,
        metadata: Some(serde_json::json!({
            "source": "linkedin_post",
            "post_id": post.id,
            "social_id": post.social_id,
            "share_url": post.share_url,
            "comment_counter": post.comment_counter,
            "reaction_counter": post.reaction_counter,
        })),
    }
}

fn comment_to_void(input: CommentVoidInput<'_>) -> Message {
    let CommentVoidInput {
        connection_id,
        conv_id,
        conv_external_id,
        post,
        comment,
        display_name,
        profile_url,
        avatar_url,
    } = input;
    let author_id = comment.author_provider_id().to_string();
    let body = comment
        .text
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string());

    let author_profile_url = profile_url.clone().or_else(|| {
        comment
            .author_details
            .as_ref()
            .and_then(|d| d.profile_url.clone())
    });

    Message {
        id: format!("{connection_id}-comment-{}", comment.id),
        conversation_id: conv_id.to_string(),
        connection_id: connection_id.to_string(),
        connector: "linkedin".to_string(),
        external_id: format!("linkedin_{connection_id}_comment_{}", comment.id),
        sender: author_id,
        sender_name: Some(display_name.to_string()),
        sender_avatar_url: avatar_url.clone(),
        body,
        timestamp: comment_timestamp(comment),
        synced_at: Some(chrono::Utc::now().timestamp()),
        is_archived: false,
        is_saved: false,
        reply_to_id: None,
        media_type: None,
        metadata: Some(serde_json::json!({
            "source": "linkedin_post_comment",
            "comment_id": comment.id,
            "post_id": post.id,
            "post_social_id": post.social_id,
            "author_name": display_name,
            "author_profile_url": author_profile_url,
            "reply_counter": comment.reply_counter,
        })),
        context_id: Some(conv_external_id.to_string()),
        context: None,
    }
}

fn post_timestamp(post: &UnipilePost) -> i64 {
    extract::parse_timestamp(post.parsed_datetime.as_deref())
}

fn comment_timestamp(comment: &UnipileComment) -> i64 {
    extract::parse_timestamp(comment.date.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_conversation_ids_use_numeric_post_id() {
        let post = UnipilePost {
            id: "7332661864792854528".into(),
            social_id: "urn:li:activity:7332661864792854528".into(),
            ..Default::default()
        };
        let (conv_id, ext) = post_conversation_ids("li", &post);
        assert_eq!(conv_id, "li-post-7332661864792854528");
        assert_eq!(ext, "linkedin_li_post_7332661864792854528");
    }

    #[test]
    fn comment_to_void_sets_post_comment_metadata() {
        let post = UnipilePost {
            id: "p1".into(),
            social_id: "urn:li:activity:p1".into(),
            text: Some("My post".into()),
            ..Default::default()
        };
        let comment = UnipileComment {
            id: "c1".into(),
            text: Some("Nice!".into()),
            date: Some("2026-05-27T10:00:00.000Z".into()),
            author: Some("Jane".into()),
            author_details: Some(crate::api::UnipileCommentAuthor {
                id: Some("ACo123".into()),
                profile_url: Some("https://www.linkedin.com/in/jane".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let msg = comment_to_void(CommentVoidInput {
            connection_id: "li",
            conv_id: "li-post-p1",
            conv_external_id: "linkedin_li_post_p1",
            post: &post,
            comment: &comment,
            display_name: "Jane",
            profile_url: &None,
            avatar_url: &None,
        });
        assert_eq!(msg.external_id, "linkedin_li_comment_c1");
        assert_eq!(
            msg.metadata.as_ref().unwrap()["source"],
            "linkedin_post_comment"
        );
        assert_eq!(
            msg.metadata.as_ref().unwrap()["post_social_id"],
            "urn:li:activity:p1"
        );
    }
}
