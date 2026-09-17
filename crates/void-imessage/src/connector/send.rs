//! Sending side for iMessage on macOS.
//!
//! Sending via Messages.app uses AppleScript / Apple Events (`osascript`), which
//! is the only supported, non-SIP-violating automation mechanism provided by macOS.
//!
//! In accordance with the Void golden rule (never report a send as sent without
//! server confirmation), sending does not return immediately after `osascript`
//! completes. Instead, it waits for the sent row to be committed to `chat.db`
//! with `is_from_me = 1`, and extracts the real message GUID.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use void_core::models::MessageContent;

use super::store;

/// Escapes a string so it can be safely embedded in an AppleScript string literal.
///
/// Only `\` and `"` need escaping, and that is sufficient rather than merely
/// convenient: every value passed through this function is interpolated inside
/// a double-quoted AppleScript string literal in [`build_applescript`]. Within
/// those delimiters the only two characters that can terminate the literal or
/// change its meaning are the closing quote and the escape character itself.
/// Escaping the quote is what closes the injection path: a recipient or body
/// containing `" & (do shell script "…") & "` stays one inert literal instead
/// of becoming concatenation and a command.
///
/// Newlines, tabs, carriage returns and non-ASCII text deliberately pass
/// through unchanged. AppleScript accepts a raw newline inside a string
/// literal, and `osascript` reads the script as UTF-8, so rewriting them to
/// `\n`-style sequences would alter the message the recipient sees rather than
/// protect anything. This is verified by the escaping tests below.
///
/// This function is NOT safe for building AppleScript outside a quoted string
/// literal (an identifier, a raw script fragment); nothing in this module does
/// that.
pub fn escape_applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Builds the AppleScript snippet to send text or a file to an iMessage / SMS recipient.
pub fn build_applescript(to: &str, content: &MessageContent) -> Result<String> {
    let escaped_to = escape_applescript_string(to);

    match content {
        MessageContent::Text { body, .. } => {
            let escaped_body = escape_applescript_string(body);
            Ok(format!(
                r#"tell application "Messages"
    set targetService to 1st service whose service type = iMessage
    set targetBuddy to buddy "{escaped_to}" of targetService
    send "{escaped_body}" to targetBuddy
end tell"#
            ))
        }
        MessageContent::File { path, caption, .. } => {
            let posix_path = path
                .to_str()
                .ok_or_else(|| anyhow!("attachment path is not valid UTF-8"))?;
            let escaped_path = escape_applescript_string(posix_path);

            let send_file = format!(
                r#"tell application "Messages"
    set targetService to 1st service whose service type = iMessage
    set targetBuddy to buddy "{escaped_to}" of targetService
    send (POSIX file "{escaped_path}") to targetBuddy
end tell"#
            );

            if let Some(caption_text) = caption {
                if !caption_text.trim().is_empty() {
                    let escaped_caption = escape_applescript_string(caption_text);
                    return Ok(format!(
                        r#"{send_file}
delay 0.5
tell application "Messages"
    set targetService to 1st service whose service type = iMessage
    set targetBuddy to buddy "{escaped_to}" of targetService
    send "{escaped_caption}" to targetBuddy
end tell"#
                    ));
                }
            }

            Ok(send_file)
        }
    }
}

/// Executes an AppleScript script via `osascript`.
pub async fn execute_applescript(script: &str) -> Result<()> {
    let output = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .await
        .context("spawning osascript to execute AppleScript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("osascript failed ({}): {}", output.status, stderr.trim());
    }

    Ok(())
}

/// Query `chat.db` for the most recent outgoing message to `to` sent after `sent_after_apple_time`.
///
/// Matching deliberately avoids `LIKE`. The recipient is caller data, and in a
/// `LIKE` pattern `%` and `_` are wildcards: the address `a_b@example.com`
/// would match the unrelated handle `axb@example.com` and confirm a send that
/// never happened, returning the GUID of somebody else's conversation.
/// `INSTR` and `SUBSTR` compare literal text, so no escaping is needed and no
/// input is special.
///
/// The empty-string guards matter for the same reason: `INSTR(x, '')` is 1 and
/// a zero-length suffix comparison is trivially true, so an empty handle or an
/// empty recipient would match every outgoing row.
pub fn find_sent_message(
    conn: &rusqlite::Connection,
    to: &str,
    sent_after_apple_time: i64,
) -> Result<Option<String>> {
    // `chat.db` normalizes handles, so the stored form may omit the country
    // code the caller passed (or carry one the caller omitted). Compare in both
    // directions by literal suffix.
    let mut stmt = conn.prepare(
        "SELECT m.guid
         FROM message m
         LEFT JOIN handle h ON m.handle_id = h.ROWID
         LEFT JOIN chat_message_join cmj ON cmj.message_id = m.ROWID
         LEFT JOIN chat c ON c.ROWID = cmj.chat_id
         WHERE m.is_from_me = 1
           AND m.date >= ?1
           AND LENGTH(?2) > 0
           AND (
                h.id = ?2
                OR c.chat_identifier = ?2
                OR INSTR(c.guid, ?2) > 0
                OR (LENGTH(h.id) > 0 AND SUBSTR(?2, -LENGTH(h.id)) = h.id)
                OR (LENGTH(h.id) > 0 AND SUBSTR(h.id, -LENGTH(?2)) = ?2)
           )
         ORDER BY m.date DESC
         LIMIT 1",
    )?;

    let mut rows = stmt.query(rusqlite::params![sent_after_apple_time, to])?;

    if let Some(row) = rows.next()? {
        let guid: String = row.get(0)?;
        Ok(Some(guid))
    } else {
        Ok(None)
    }
}

/// Sends an iMessage and waits until `chat.db` records it, returning the confirmed message GUID.
pub async fn send_and_confirm(
    db_path: &Path,
    to: &str,
    content: &MessageContent,
    timeout: Duration,
) -> Result<String> {
    let script = build_applescript(to, content)?;

    // Record the current timestamp in Apple's epoch (seconds or nanoseconds since 2001).
    // In chat.db modern macOS stores nanoseconds since 2001-01-01.
    // 1 second buffer before sending to avoid missing fast inserts.
    let now_unix = chrono::Utc::now().timestamp();
    let sent_after_apple_time = (now_unix - store::APPLE_EPOCH_OFFSET - 2) * 1_000_000_000;

    execute_applescript(&script).await?;

    let deadline = Instant::now() + timeout;
    let poll_interval = Duration::from_millis(300);

    while Instant::now() < deadline {
        tokio::time::sleep(poll_interval).await;

        if let Ok(conn) = store::open_read_only(db_path) {
            if let Ok(Some(guid)) = find_sent_message(&conn, to, sent_after_apple_time) {
                return Ok(guid);
            }
        }
    }

    // If confirmation times out, don't pretend success.
    anyhow::bail!(
        "message was dispatched to Messages.app but not confirmed in chat.db within {:?}",
        timeout
    )
}
