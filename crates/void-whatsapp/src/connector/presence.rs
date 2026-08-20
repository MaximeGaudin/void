//! Keep the linked WhatsApp session from advertising "online".
//!
//! `wa-rs` mirrors WhatsApp Web and sends `presence type=available` after
//! login (and again when the push name arrives). That makes the account look
//! permanently online to contacts and can suppress phone push notifications.
//! We override that with `unavailable`, with a short delay so we win races
//! against the library's own `set_available` calls, plus a periodic refresh
//! while the sync daemon stays connected.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};
use wa_rs::client::Client;

/// Delay before overriding `wa-rs`'s automatic `set_available`.
const OVERRIDE_DELAY: Duration = Duration::from_secs(2);

/// How often to re-assert unavailable while the sync loop is alive.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub(crate) async fn set_unavailable(client: &Client) {
    match client.presence().set_unavailable().await {
        Ok(()) => debug!("WhatsApp presence set to unavailable"),
        Err(e) => warn!(error = %e, "failed to set WhatsApp presence unavailable"),
    }
}

/// Schedule an unavailable presence update after a short delay.
///
/// Used on connect / push-name updates / sends so we run after `wa-rs`
/// finishes its own `set_available` (which may race with
/// `Event::SelfPushNameUpdated`).
pub(crate) fn schedule_unavailable(client: Arc<Client>) {
    tokio::spawn(async move {
        tokio::time::sleep(OVERRIDE_DELAY).await;
        set_unavailable(&client).await;
    });
}

/// Periodically re-assert unavailable until sync is cancelled.
///
/// Spawned once per `start_sync` (not per reconnect) so reconnect storms
/// cannot pile up refresher tasks.
pub(crate) fn spawn_unavailable_refresher(
    client_holder: Arc<Mutex<Option<Arc<Client>>>>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Skip the immediate first tick — connect handlers already schedule one.
        ticker.tick().await;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("stopping WhatsApp presence refresher (sync cancelled)");
                    break;
                }
                _ = ticker.tick() => {
                    let client = client_holder.lock().await.clone();
                    let Some(client) = client.filter(|c| c.is_connected()) else {
                        continue;
                    };
                    set_unavailable(&client).await;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_delay_is_short_enough_to_beat_visible_online_window() {
        assert!(OVERRIDE_DELAY <= Duration::from_secs(5));
        assert!(OVERRIDE_DELAY >= Duration::from_secs(1));
    }

    #[test]
    fn refresh_interval_is_minutes_not_seconds() {
        assert!(REFRESH_INTERVAL >= Duration::from_secs(60));
        assert!(REFRESH_INTERVAL <= Duration::from_secs(15 * 60));
    }

    #[tokio::test]
    async fn refresher_exits_promptly_when_sync_is_cancelled() {
        let holder = Arc::new(Mutex::new(None));
        let cancel = CancellationToken::new();
        let handle = spawn_unavailable_refresher(Arc::clone(&holder), cancel.clone());

        cancel.cancel();

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("refresher should exit shortly after cancel")
            .expect("refresher task should not panic");
    }
}
