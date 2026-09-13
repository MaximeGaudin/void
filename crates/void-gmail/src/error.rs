use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GmailError {
    #[error("API error: {0}")]
    Api(String),
    #[error("Auth error: {0}")]
    Auth(String),
    /// Token lacks `gmail.settings.basic` (or similar) for send-as / signature reads.
    #[error(
        "insufficient OAuth scope for Gmail settings (need gmail.settings.basic); re-authenticate"
    )]
    InsufficientScope,
    /// The stored `historyId` is too old: Gmail purges history after a limited
    /// window, so incremental sync must fall back to a full INBOX refresh.
    #[error("gmail history expired; full inbox refresh required")]
    HistoryExpired,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

/// The `reason` values carried by a Google API error body, lowercased.
///
/// Google uses two shapes for the same field: the classic
/// `error.errors[].reason` and the newer `error.details[].reason`. Both are read
/// here so every caller matches against one list instead of learning the shapes
/// again. A body that is not JSON yields nothing, and callers fall back to
/// matching the reason token in the raw text.
fn google_error_reasons(body: &str) -> Vec<String> {
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(error) = parsed.get("error") else {
        return Vec::new();
    };
    ["errors", "details"]
        .iter()
        .filter_map(|key| error.get(key)?.as_array())
        .flatten()
        .filter_map(|item| item.get("reason")?.as_str())
        .map(|reason| reason.to_ascii_lowercase())
        .collect()
}

/// Google reasons that mean "the request was fine, come back later".
///
/// `rateLimitExceeded` and `userRateLimitExceeded` are the per-user quota,
/// `quotaExceeded` the project quota, `backendError` a transient Gmail fault.
const RETRYABLE_REASONS: [&str; 4] = [
    "ratelimitexceeded",
    "userratelimitexceeded",
    "quotaexceeded",
    "backenderror",
];

/// Whether a Gmail API error body indicates missing OAuth scopes (vs other 403s).
///
/// Matches Google's `ACCESS_TOKEN_SCOPE_INSUFFICIENT` reason and the common
/// "insufficient authentication scopes" message. Avoids broad phrases like
/// "insufficient permissions", which appear on unrelated 403s.
pub fn is_insufficient_scope_body(body: &str) -> bool {
    let reasons = google_error_reasons(body);
    if reasons
        .iter()
        .any(|r| r == "access_token_scope_insufficient" || r == "insufficientpermissions")
    {
        return true;
    }
    let lower = body.to_ascii_lowercase();
    lower.contains("access_token_scope_insufficient")
        || lower.contains("insufficientpermissions")
        || lower.contains("insufficient authentication scopes")
}

/// Whether a Gmail API error body says the quota was hit, so the call is worth
/// retrying after a backoff.
///
/// Needed because Gmail does not answer 429 for the per-user quota: it answers
/// **403 with `reason: rateLimitExceeded`**. The status alone therefore cannot
/// decide, and every other 403 (missing scope, denied permission, domain policy)
/// is a real answer that must keep failing fast. Retrying those burns quota and
/// hides an auth problem.
pub fn is_retryable_quota_body(body: &str) -> bool {
    // A scope error wins: it is a 403 that will never clear on its own, and some
    // bodies mention both a permission reason and quota-looking prose.
    if is_insufficient_scope_body(body) {
        return false;
    }
    if google_error_reasons(body)
        .iter()
        .any(|r| RETRYABLE_REASONS.contains(&r.as_str()))
    {
        return true;
    }
    // Non-JSON or an unparsed shape: match the reason token itself, never loose
    // prose like "quota", which shows up in unrelated messages.
    let lower = body.to_ascii_lowercase();
    RETRYABLE_REASONS.iter().any(|r| lower.contains(r))
}

/// How long to wait before retrying a quota 403, taken from the error body.
///
/// Gmail's per-user quota is a per-minute window. The payload it actually sends
/// does not put that wait in a `Retry-After` header: it puts
/// `Retry after 2026-09-11T20:00:00.000Z` in `error.message`, or a protobuf
/// `retryDelay` (`"30s"`) on `google.rpc.RetryInfo`. Honouring only a 500 ms
/// exponential curve retries inside the same minute and burns more quota.
///
/// `now` is injected so tests do not depend on the wall clock.
pub fn retry_delay_from_quota_body(body: &str, now: DateTime<Utc>) -> Option<Duration> {
    retry_delay_from_json(body).or_else(|| retry_delay_from_retry_after_text(body, now))
}

fn retry_delay_from_json(body: &str) -> Option<Duration> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    let details = parsed.get("error")?.get("details")?.as_array()?;
    for item in details {
        if let Some(d) = item
            .get("retryDelay")
            .and_then(|v| v.as_str())
            .and_then(protobuf_duration)
        {
            return Some(d);
        }
        if let Some(d) = item
            .get("metadata")
            .and_then(|m| m.get("retryDelay"))
            .and_then(|v| v.as_str())
            .and_then(protobuf_duration)
        {
            return Some(d);
        }
    }
    None
}

/// JSON encoding of `google.protobuf.Duration`: `"30s"`, `"1.5s"`.
fn protobuf_duration(raw: &str) -> Option<Duration> {
    let secs: f64 = raw.trim().strip_suffix('s')?.parse().ok()?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(secs).ok()
}

fn retry_delay_from_retry_after_text(body: &str, now: DateTime<Utc>) -> Option<Duration> {
    let lower = body.to_ascii_lowercase();
    let idx = lower.find("retry after")?;
    let rest = body[idx + "retry after".len()..].trim_start();
    // Timestamp sits inside JSON (`...000Z","errors"`), so stop at the first
    // character that cannot appear in RFC 3339.
    let token: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+' | 'T' | 'Z'))
        .collect();
    if token.is_empty() {
        return None;
    }
    let when = DateTime::parse_from_rfc3339(&token)
        .ok()?
        .with_timezone(&Utc);
    let secs = when.signed_duration_since(now).num_seconds();
    (secs > 0).then(|| Duration::from_secs(secs as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use std::time::Duration;

    #[test]
    fn insufficient_scope_body_detects_google_reason() {
        assert!(is_insufficient_scope_body(
            r#"{"error":{"details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#
        ));
        assert!(is_insufficient_scope_body(
            "Request had insufficient authentication scopes."
        ));
        assert!(is_insufficient_scope_body(
            r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#
        ));
        assert!(!is_insufficient_scope_body(
            "Admin has disabled this API for the domain."
        ));
        assert!(!is_insufficient_scope_body(
            "You do not have permission to access this resource."
        ));
        assert!(!is_insufficient_scope_body("insufficient permissions"));
        assert!(!is_insufficient_scope_body(""));
    }

    #[test]
    fn google_error_reasons_reads_both_shapes() {
        assert_eq!(
            google_error_reasons(r#"{"error":{"errors":[{"reason":"rateLimitExceeded"}]}}"#),
            vec!["ratelimitexceeded"]
        );
        assert_eq!(
            google_error_reasons(r#"{"error":{"details":[{"reason":"backendError"}]}}"#),
            vec!["backenderror"]
        );
        assert!(google_error_reasons("not json at all").is_empty());
        assert!(google_error_reasons(r#"{"something":"else"}"#).is_empty());
    }

    #[test]
    fn retryable_quota_body_detects_the_403_gmail_actually_sends() {
        // The real payload, trimmed: this is what the per-user quota looks like.
        assert!(is_retryable_quota_body(
            r#"{"error":{"code":403,"message":"User-rate limit exceeded.  Retry after 2026-09-11T20:00:00.000Z","errors":[{"message":"User-rate limit exceeded.","domain":"usageLimits","reason":"rateLimitExceeded"}],"status":"PERMISSION_DENIED"}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"userRateLimitExceeded"}]}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"quotaExceeded"}]}}"#
        ));
        assert!(is_retryable_quota_body(
            r#"{"error":{"details":[{"reason":"backendError"}]}}"#
        ));
    }

    #[test]
    fn retryable_quota_body_rejects_real_answers() {
        // A scope error must keep failing fast: retrying hides the auth problem.
        assert!(!is_retryable_quota_body(
            r#"{"error":{"message":"Request had insufficient authentication scopes.","status":"PERMISSION_DENIED","details":[{"reason":"ACCESS_TOKEN_SCOPE_INSUFFICIENT"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"insufficientPermissions"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            r#"{"error":{"errors":[{"reason":"forbidden"}]}}"#
        ));
        assert!(!is_retryable_quota_body(
            "Admin has disabled this API for the domain."
        ));
        // Prose about quotas is not a reason. Only the reason token counts.
        assert!(!is_retryable_quota_body(
            "You have exceeded your daily quota of patience."
        ));
        assert!(!is_retryable_quota_body(""));
    }

    #[test]
    fn retry_delay_from_quota_body_reads_the_message_gmail_sends() {
        let now = DateTime::parse_from_rfc3339("2026-09-11T19:59:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let body = r#"{"error":{"code":403,"message":"User-rate limit exceeded.  Retry after 2026-09-11T20:00:00.000Z","errors":[{"reason":"rateLimitExceeded"}]}}"#;
        assert_eq!(
            retry_delay_from_quota_body(body, now),
            Some(Duration::from_secs(30))
        );
    }

    #[test]
    fn retry_delay_from_quota_body_reads_retryinfo() {
        let now = Utc::now();
        let body = r#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"45s"}]}}"#;
        assert_eq!(
            retry_delay_from_quota_body(body, now),
            Some(Duration::from_secs(45))
        );
        let nested = r#"{"error":{"details":[{"metadata":{"retryDelay":"1.5s"}}]}}"#;
        assert_eq!(
            retry_delay_from_quota_body(nested, now),
            Some(Duration::from_millis(1500))
        );
    }

    #[test]
    fn retry_delay_from_quota_body_ignores_past_and_prose() {
        let now = DateTime::parse_from_rfc3339("2026-09-13T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let past = r#"{"error":{"message":"Retry after 2026-09-11T20:00:00.000Z"}}"#;
        assert_eq!(retry_delay_from_quota_body(past, now), None);
        assert_eq!(
            retry_delay_from_quota_body("You have exceeded your daily quota of patience.", now),
            None
        );
        assert_eq!(retry_delay_from_quota_body("", now), None);
    }
}
