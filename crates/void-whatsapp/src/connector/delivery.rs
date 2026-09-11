//! Server-acceptance verification for outbound WhatsApp sends.
//!
//! `wa-rs` 0.2 hands back a message id as soon as the stanza bytes are given to
//! the noise socket: `send::send_message_with_options` generates the id locally,
//! calls `send_message_impl`, which ends on `Client::send_node`, which only
//! marshals, encrypts and writes. Nothing waits for the server `<ack/>`. A send
//! on a dying socket therefore returns `Ok(id)` for a message the server never
//! saw. Measured on 2026-09-11: the CLI printed "Message sent (id: ...)" in
//! 0.66 s, the message was in no store afterwards, and the wa-rs message loop
//! exited 52 s later.
//!
//! The library does wait for acks internally, but that machinery is not
//! reachable from a consumer: `Client::response_waiters` is `pub(crate)`, and
//! incoming `<ack/>` stanzas are swallowed by `handlers::basic::AckHandler`
//! without being dispatched on the public event bus (there is no
//! `Event::ServerAck`). So a consumer cannot observe the ack for its own send.
//!
//! What a consumer can do is prove the stanza reached the server, using stream
//! ordering. Every outbound frame goes through a single `NoiseSocket` sender
//! task (`socket/noise_socket.rs`: `encrypt_and_send` pushes a job onto an mpsc
//! queue and awaits that job's result), so frames hit the TCP stream in call
//! order, and the caller does not return until its own frame was handed to the
//! transport. Issuing a `w:p` ping IQ *after* the message and waiting for its
//! pong is therefore a barrier: a pong proves the server consumed the stream
//! past our message frame. No pong means we cannot claim the message was sent.
//!
//! That is what this module implements: a liveness precheck before the write,
//! and a bounded barrier after it.

use std::time::Duration;

use thiserror::Error;
use tracing::{debug, warn};
use wa_rs::client::Client;
use wa_rs::request::IqError;
use wa_rs_core::iq::keepalive::KeepaliveSpec;

/// Bounded wait for the post-send barrier to round-trip.
pub(crate) const ACK_TIMEOUT: Duration = Duration::from_secs(12);

/// Hard ceiling on the barrier call. `send_iq` applies `ACK_TIMEOUT` to the
/// response wait only: the `send_node` that precedes it can itself block on a
/// stuck transport, so the whole call gets an outer deadline too.
const BARRIER_HARD_TIMEOUT: Duration = Duration::from_secs(15);

/// Hard ceiling on the stanza write. `send_message_with_options` has no
/// internal timeout, and the noise sender task can hang on `transport.send`.
pub(crate) const SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// `Client::is_connected` reads the noise socket slot with `try_lock`, so it
/// reports `false` while another send briefly holds that mutex. Retry a few
/// times before declaring the connection down, to avoid failing a healthy send.
const LIVENESS_ATTEMPTS: u32 = 5;
const LIVENESS_RETRY_DELAY: Duration = Duration::from_millis(20);

/// Why the barrier did not confirm the send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnconfirmedReason {
    /// No response from the server before the deadline.
    Timeout,
    /// The server answered, but not with a clean result.
    BadServerReply(String),
}

impl std::fmt::Display for UnconfirmedReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "no server response before the deadline"),
            Self::BadServerReply(detail) => write!(f, "server replied with {detail}"),
        }
    }
}

/// A send that could not be reported as successful.
///
/// The wording matters as much as the variant: none of these may read as a
/// success, and the two "unknown" cases must not read as a confirmed loss.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum SendFailure {
    #[error(
        "WhatsApp send aborted on connection '{connection_id}': {reason}. \
         Nothing was written to the socket, the message was not sent."
    )]
    NotLive {
        connection_id: String,
        reason: &'static str,
    },

    #[error(
        "WhatsApp send NOT confirmed on connection '{connection_id}': the stanza for message \
         {message_id} was written but the server did not confirm it within {}s ({reason}). \
         Delivery is unknown, do not assume the message arrived.",
        .waited.as_secs()
    )]
    Unconfirmed {
        connection_id: String,
        message_id: String,
        waited: Duration,
        reason: UnconfirmedReason,
    },

    #[error(
        "WhatsApp send NOT confirmed on connection '{connection_id}': the transport failed while \
         confirming message {message_id} ({detail}). The message very likely never reached \
         WhatsApp."
    )]
    Transport {
        connection_id: String,
        message_id: String,
        detail: String,
    },

    #[error(
        "WhatsApp send stalled on connection '{connection_id}': writing the stanza did not \
         complete within {}s. Delivery is unknown, do not assume the message arrived.",
        .waited.as_secs()
    )]
    SendStalled {
        connection_id: String,
        waited: Duration,
    },
}

/// Precheck decision, split from the `Client` so it is testable on its own.
///
/// Both flags are required: `is_connected` alone is true during a reconnect
/// handshake, before the session is usable again.
pub(crate) fn liveness_failure(
    connection_id: &str,
    connected: bool,
    logged_in: bool,
) -> Option<SendFailure> {
    let reason = match (connected, logged_in) {
        (true, true) => return None,
        (false, _) => "the connection has no live socket (disconnected or reconnecting)",
        (true, false) => "the socket is up but the session is not logged in yet",
    };
    Some(SendFailure::NotLive {
        connection_id: connection_id.to_string(),
        reason,
    })
}

/// Fails fast when the connection cannot carry a stanza right now.
pub(crate) async fn precheck(client: &Client, connection_id: &str) -> Result<(), SendFailure> {
    let mut failure = None;
    for attempt in 0..LIVENESS_ATTEMPTS {
        match liveness_failure(connection_id, client.is_connected(), client.is_logged_in()) {
            None => return Ok(()),
            Some(f) => {
                failure = Some(f);
                if attempt + 1 < LIVENESS_ATTEMPTS {
                    tokio::time::sleep(LIVENESS_RETRY_DELAY).await;
                }
            }
        }
    }
    let failure = failure.expect("loop ran at least once without returning Ok");
    warn!(connection_id = %connection_id, error = %failure, "WhatsApp send rejected by liveness precheck");
    Err(failure)
}

/// Turns a barrier IQ result into a send verdict.
///
/// Split from the network call so every branch is testable without a socket.
pub(crate) fn classify_barrier(
    connection_id: &str,
    message_id: &str,
    result: Result<(), IqError>,
) -> Result<(), SendFailure> {
    let unconfirmed = |reason| SendFailure::Unconfirmed {
        connection_id: connection_id.to_string(),
        message_id: message_id.to_string(),
        waited: ACK_TIMEOUT,
        reason,
    };
    let transport = |detail: String| SendFailure::Transport {
        connection_id: connection_id.to_string(),
        message_id: message_id.to_string(),
        detail,
    };

    match result {
        // The pong came back, so the server consumed the stream past our
        // message frame. The stanza was accepted.
        Ok(()) => Ok(()),

        Err(IqError::Timeout) => Err(unconfirmed(UnconfirmedReason::Timeout)),

        // A reply came back but was not a clean result. The stream did
        // round-trip, yet we refuse to read that as proof of acceptance.
        Err(e @ IqError::ServerError { .. }) | Err(e @ IqError::ParseError(_)) => Err(unconfirmed(
            UnconfirmedReason::BadServerReply(e.to_string()),
        )),

        Err(e) => Err(transport(e.to_string())),
    }
}

/// Waits, bounded, for proof that the server consumed the message stanza.
///
/// Call this immediately after the send, on the same `Client`, so no other
/// stanza can be interleaved by this code path between the two.
pub(crate) async fn confirm_accepted(
    client: &Client,
    connection_id: &str,
    message_id: &str,
) -> Result<(), SendFailure> {
    let barrier = client.execute(KeepaliveSpec::with_timeout(ACK_TIMEOUT));
    let result = match tokio::time::timeout(BARRIER_HARD_TIMEOUT, barrier).await {
        Ok(result) => result,
        Err(_) => Err(IqError::Timeout),
    };

    match classify_barrier(connection_id, message_id, result) {
        Ok(()) => {
            debug!(connection_id = %connection_id, message_id = %message_id, "WhatsApp send confirmed by server");
            Ok(())
        }
        Err(failure) => {
            warn!(connection_id = %connection_id, message_id = %message_id, error = %failure, "WhatsApp send not confirmed");
            Err(failure)
        }
    }
}

/// Runs the stanza write under a hard deadline.
pub(crate) async fn with_send_timeout<F, T>(connection_id: &str, fut: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    with_send_deadline(connection_id, SEND_TIMEOUT, fut).await
}

/// Deadline is a parameter so tests can exercise the stall branch without
/// burning [`SEND_TIMEOUT`] of wall clock.
async fn with_send_deadline<F, T>(
    connection_id: &str,
    deadline: Duration,
    fut: F,
) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    match tokio::time::timeout(deadline, fut).await {
        Ok(result) => result,
        Err(_) => Err(SendFailure::SendStalled {
            connection_id: connection_id.to_string(),
            waited: deadline,
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wa_rs::socket::error::SocketError;
    use wa_rs_binary::node::{Attrs, Node};

    const CONN: &str = "WA-french";
    const MSG: &str = "3EB01E021E254E73BF2930";

    fn assert_never_claims_success(failure: &SendFailure) {
        let text = failure.to_string();
        assert!(
            !text.contains("Message sent"),
            "failure text must not read as a success: {text}"
        );
        assert!(
            text.contains("NOT confirmed") || text.contains("aborted") || text.contains("stalled"),
            "failure text must be unambiguous: {text}"
        );
    }

    #[test]
    fn liveness_passes_only_when_connected_and_logged_in() {
        assert!(liveness_failure(CONN, true, true).is_none());
    }

    #[test]
    fn liveness_rejects_dead_socket() {
        let failure = liveness_failure(CONN, false, true).expect("must reject");
        assert!(matches!(failure, SendFailure::NotLive { .. }));
        assert!(failure.to_string().contains("no live socket"));
        assert!(failure.to_string().contains("was not sent"));
        assert_never_claims_success(&failure);
    }

    #[test]
    fn liveness_rejects_socket_up_but_not_logged_in() {
        // The state during a reconnect handshake: this is what the production
        // failure on 2026-09-11 wrote into.
        let failure = liveness_failure(CONN, true, false).expect("must reject");
        assert!(failure.to_string().contains("not logged in"));
        assert_never_claims_success(&failure);
    }

    #[test]
    fn liveness_rejects_fully_down_connection() {
        let failure = liveness_failure(CONN, false, false).expect("must reject");
        assert!(matches!(failure, SendFailure::NotLive { .. }));
    }

    #[test]
    fn barrier_pong_confirms_the_send() {
        assert!(classify_barrier(CONN, MSG, Ok(())).is_ok());
    }

    #[test]
    fn barrier_timeout_is_unconfirmed_not_transport() {
        let failure = classify_barrier(CONN, MSG, Err(IqError::Timeout)).expect_err("must fail");
        assert!(matches!(
            failure,
            SendFailure::Unconfirmed {
                reason: UnconfirmedReason::Timeout,
                ..
            }
        ));
        let text = failure.to_string();
        assert!(text.contains(MSG));
        assert!(text.contains("12s"));
        assert!(text.contains("Delivery is unknown"));
        assert_never_claims_success(&failure);
    }

    #[test]
    fn barrier_transport_errors_are_distinguishable_from_timeout() {
        let cases = [
            IqError::NotConnected,
            IqError::Socket(SocketError::Crypto("write failed".into())),
            IqError::Disconnected(Node::new("stream:error", Attrs::new(), None)),
            IqError::InternalChannelClosed,
        ];
        for case in cases {
            let label = case.to_string();
            let failure = classify_barrier(CONN, MSG, Err(case)).expect_err("must fail");
            assert!(
                matches!(failure, SendFailure::Transport { .. }),
                "{label} should classify as transport, got {failure:?}"
            );
            assert!(failure.to_string().contains("never reached WhatsApp"));
            assert_never_claims_success(&failure);
        }
    }

    #[test]
    fn barrier_bad_server_reply_is_unconfirmed() {
        let failure = classify_barrier(
            CONN,
            MSG,
            Err(IqError::ServerError {
                code: 500,
                text: "internal".into(),
            }),
        )
        .expect_err("must fail");
        assert!(matches!(
            failure,
            SendFailure::Unconfirmed {
                reason: UnconfirmedReason::BadServerReply(_),
                ..
            }
        ));
        assert_never_claims_success(&failure);
    }

    #[test]
    fn send_stalled_reports_unknown_delivery() {
        let failure = SendFailure::SendStalled {
            connection_id: CONN.into(),
            waited: SEND_TIMEOUT,
        };
        assert!(failure.to_string().contains("30s"));
        assert_never_claims_success(&failure);
    }

    #[tokio::test]
    async fn with_send_timeout_passes_through_success() {
        let value = with_send_timeout(CONN, async { Ok::<_, anyhow::Error>("id-1".to_string()) })
            .await
            .expect("should pass through");
        assert_eq!(value, "id-1");
    }

    #[tokio::test]
    async fn with_send_deadline_fails_when_the_write_hangs() {
        let err = with_send_deadline(CONN, Duration::from_millis(20), async {
            // A noise sender task stuck on `transport.send` never returns.
            std::future::pending::<anyhow::Result<String>>().await
        })
        .await
        .expect_err("a hanging write must not succeed");
        assert!(err.to_string().contains("stalled"), "{err}");
        assert!(!err.to_string().contains("Message sent"), "{err}");
    }
}
