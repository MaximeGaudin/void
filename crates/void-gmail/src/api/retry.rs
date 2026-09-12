//! Retry policy for transient Gmail API failures.
//!
//! Gmail enforces a per-user quota (`Total Query Cost`, units per minute). It is
//! shared by every process authenticated as that user, so a burst from one client
//! can push another over the limit. Google's answer to that is documented: back off
//! and retry, honouring `Retry-After` when it is present.
//!
//! Without this, a 429 surfaces as a fatal `GmailError::Http` and the caller loses
//! a read that would have succeeded a second later.

use std::time::Duration;

use tracing::warn;

/// Bounded exponential backoff with jitter.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Total attempts, including the first one. `1` disables retrying.
    pub max_attempts: u32,
    /// Delay before the second attempt; doubles after each failure.
    pub base_delay: Duration,
    /// Upper bound on any single delay, before jitter.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(16),
        }
    }
}

impl RetryPolicy {
    /// Near-zero delays, for tests that assert retry behaviour without sleeping.
    #[cfg(test)]
    pub fn fast() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(4),
        }
    }

    /// Delay before attempt number `attempt` (1-based: `1` is the first retry).
    ///
    /// Full jitter, as recommended for shared quotas: without it, several clients
    /// that hit the same 429 would wake up together and collide again.
    fn delay_for(&self, attempt: u32) -> Duration {
        let exp = self
            .base_delay
            .saturating_mul(2u32.saturating_pow(attempt.saturating_sub(1)));
        let capped = exp.min(self.max_delay);
        jitter(capped)
    }
}

/// Full jitter in `[capped / 2, capped]`.
///
/// Uses the clock rather than a `rand` dependency: the quality needed here is
/// "two processes do not wake up in lockstep", not cryptographic randomness.
fn jitter(capped: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    let half = capped / 2;
    let spread = capped.saturating_sub(half);
    if spread.is_zero() {
        return capped;
    }
    half + Duration::from_nanos(nanos % (spread.as_nanos() as u64).max(1))
}

/// Whether the status alone is enough to retry.
///
/// 429 and 5xx are transient by definition. 401 and 404 are real answers and must
/// keep failing fast. 403 is decided by the body instead: see
/// [`needs_body_to_decide`].
fn is_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Whether the status is ambiguous, so the body has to be read to decide.
///
/// Gmail does not answer 429 for the per-user quota. It answers **403 with
/// `reason: rateLimitExceeded`**, the same status it uses for a missing scope or a
/// denied permission. One is transient, the others never clear. The discriminator
/// is Google's `reason` field, parsed by
/// [`crate::error::is_retryable_quota_body`].
fn needs_body_to_decide(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::FORBIDDEN
}

/// Read the body once, and hand back a response the caller can still consume.
///
/// Deciding on a 403 costs the response: `bytes()` takes ownership. So the parts
/// are put back together, URL and headers included, and the rebuilt response
/// behaves like the original for `error_for_status`, `text` and `json`. Without
/// this, a non-retryable 403 would reach the caller with an empty body and its
/// error message would lose the Google reason that explains it.
async fn buffer_body(
    resp: reqwest::Response,
) -> Result<(reqwest::Response, String), reqwest::Error> {
    use reqwest::ResponseBuilderExt;

    let url = resp.url().clone();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.bytes().await?;
    let text = String::from_utf8_lossy(&bytes).into_owned();

    let mut builder = http::Response::builder().status(status).url(url);
    if let Some(slot) = builder.headers_mut() {
        *slot = headers;
    }
    // Infallible: the status and headers come from a response that already
    // parsed, so there is nothing left for the builder to reject.
    let rebuilt = builder
        .body(bytes)
        .expect("rebuilding a response from its own parts");
    Ok((reqwest::Response::from(rebuilt), text))
}

/// Whether a transport error is worth retrying (timeouts and connection failures).
fn is_retryable_transport(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

/// `Retry-After`, when the server sent one. Seconds, or an HTTP date.
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let raw = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    if let Ok(secs) = raw.trim().parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let when = chrono::DateTime::parse_from_rfc2822(raw.trim()).ok()?;
    let delta = when.timestamp() - chrono::Utc::now().timestamp();
    (delta > 0).then(|| Duration::from_secs(delta as u64))
}

/// Send a request, retrying transient failures per `policy`.
///
/// Returns the last response when attempts run out, so the caller's existing
/// `.error_for_status()` still decides the final error. That keeps the error type
/// of every call site unchanged.
///
/// A request whose body cannot be cloned (streaming) is sent exactly once.
pub async fn send_with_retry(
    req: reqwest::RequestBuilder,
    policy: &RetryPolicy,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut attempt = 1u32;
    loop {
        let clone = req.try_clone();
        let is_last = attempt >= policy.max_attempts || clone.is_none();

        let this = match clone {
            Some(c) if !is_last => c,
            // Last attempt, or an unclonable body: consume the original.
            _ => return req.send().await,
        };

        match this.send().await {
            Ok(resp) if is_retryable(resp.status()) => {
                let status = resp.status();
                let wait = retry_after(&resp).unwrap_or_else(|| policy.delay_for(attempt));
                warn!(
                    %status,
                    attempt,
                    max_attempts = policy.max_attempts,
                    wait_ms = wait.as_millis() as u64,
                    "gmail: transient API failure, backing off"
                );
                tokio::time::sleep(wait).await;
            }
            Ok(resp) if needs_body_to_decide(resp.status()) => {
                let status = resp.status();
                let wait = retry_after(&resp).unwrap_or_else(|| policy.delay_for(attempt));
                let (resp, body) = buffer_body(resp).await?;
                if !crate::error::is_retryable_quota_body(&body) {
                    // A real 403: missing scope, denied permission, domain policy.
                    // Retrying burns quota and hides the problem.
                    return Ok(resp);
                }
                warn!(
                    %status,
                    attempt,
                    max_attempts = policy.max_attempts,
                    wait_ms = wait.as_millis() as u64,
                    "gmail: quota exceeded (403 rateLimitExceeded), backing off"
                );
                tokio::time::sleep(wait).await;
            }
            Ok(resp) => return Ok(resp),
            Err(e) if is_retryable_transport(&e) => {
                let wait = policy.delay_for(attempt);
                warn!(
                    attempt,
                    max_attempts = policy.max_attempts,
                    wait_ms = wait.as_millis() as u64,
                    "gmail: transport error, backing off: {e}"
                );
                tokio::time::sleep(wait).await;
            }
            Err(e) => return Err(e),
        }
        attempt += 1;
    }
}

/// Lets a call site opt into retrying by replacing `.send()` with
/// `.send_retrying(&self.retry)`, keeping the rest of the chain untouched.
pub trait SendRetrying {
    /// Send with the given retry policy. See [`send_with_retry`].
    fn send_retrying(
        self,
        policy: &RetryPolicy,
    ) -> impl std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>;
}

impl SendRetrying for reqwest::RequestBuilder {
    async fn send_retrying(
        self,
        policy: &RetryPolicy,
    ) -> Result<reqwest::Response, reqwest::Error> {
        send_with_retry(self, policy).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_covers_quota_and_backend_errors() {
        assert!(is_retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable(reqwest::StatusCode::SERVICE_UNAVAILABLE));
    }

    #[test]
    fn retryable_excludes_real_answers() {
        // A missing thread must keep failing fast: retrying it burns quota for an
        // answer that will not change.
        assert!(!is_retryable(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_retryable(reqwest::StatusCode::OK));
    }

    #[test]
    fn forbidden_is_decided_by_the_body_not_the_status() {
        // 403 is the status Gmail actually sends for the per-user quota, and also
        // the one it sends for a missing scope. The status alone decides nothing.
        assert!(!is_retryable(reqwest::StatusCode::FORBIDDEN));
        assert!(needs_body_to_decide(reqwest::StatusCode::FORBIDDEN));
        assert!(!needs_body_to_decide(reqwest::StatusCode::NOT_FOUND));
        assert!(!needs_body_to_decide(
            reqwest::StatusCode::TOO_MANY_REQUESTS
        ));
        assert!(!needs_body_to_decide(reqwest::StatusCode::OK));
    }

    #[tokio::test]
    async fn buffer_body_preserves_status_headers_and_body() {
        use reqwest::ResponseBuilderExt;

        let original = reqwest::Response::from(
            http::Response::builder()
                .status(403)
                .header("X-Marker", "kept")
                .url(url::Url::parse("https://example.test/threads/t1").unwrap())
                .body("quota body")
                .unwrap(),
        );

        let (rebuilt, text) = buffer_body(original).await.unwrap();
        assert_eq!(text, "quota body");
        assert_eq!(rebuilt.status(), reqwest::StatusCode::FORBIDDEN);
        assert_eq!(
            rebuilt
                .headers()
                .get("X-Marker")
                .map(|v| v.to_str().unwrap()),
            Some("kept")
        );
        assert_eq!(rebuilt.url().as_str(), "https://example.test/threads/t1");
        // The caller still gets the body: reading it to decide must not consume it.
        assert_eq!(rebuilt.text().await.unwrap(), "quota body");
    }

    #[test]
    fn delay_grows_and_stays_capped() {
        let p = RetryPolicy {
            max_attempts: 6,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(800),
        };
        // Jitter puts each delay in [capped/2, capped].
        assert!(p.delay_for(1) >= Duration::from_millis(50));
        assert!(p.delay_for(1) <= Duration::from_millis(100));
        assert!(p.delay_for(3) <= Duration::from_millis(400));
        // Far-out attempts stay bounded by max_delay.
        assert!(p.delay_for(20) <= Duration::from_millis(800));
    }
}
