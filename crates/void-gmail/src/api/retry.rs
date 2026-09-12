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

/// Whether a response status is worth retrying.
///
/// 429 is the quota case. 5xx covers Gmail's transient backend errors. Everything
/// else (401, 403 scope errors, 404) is a real answer and must keep failing fast.
fn is_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
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
        // A scope error or a missing thread must keep failing fast: retrying it
        // burns quota for an answer that will not change.
        assert!(!is_retryable(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(reqwest::StatusCode::FORBIDDEN));
        assert!(!is_retryable(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_retryable(reqwest::StatusCode::OK));
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
