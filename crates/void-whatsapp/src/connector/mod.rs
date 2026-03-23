//! WhatsApp connector: struct, Connector impl, and orchestration.

mod connector_trait;
mod extract;
mod media;
mod send;
mod sync;

#[cfg(test)]
mod tests;

// Re-export public API for external crates
pub use send::{normalize_phone, parse_jid};

use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::{error, info};
use wa_rs::bot::Bot;
use wa_rs::client::Client;
use wa_rs::types::events::Event;
use wa_rs_sqlite_storage::SqliteStore;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;

pub struct WhatsAppConnector {
    config_id: String,
    session_db_path: String,
    client: Arc<Mutex<Option<Arc<Client>>>>,
    own_jid: Arc<std::sync::Mutex<Option<String>>>,
}

impl WhatsAppConnector {
    pub fn new(connection_id: &str, session_db_path: &str) -> Self {
        Self {
            config_id: connection_id.to_string(),
            session_db_path: session_db_path.to_string(),
            client: Arc::new(Mutex::new(None)),
            own_jid: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Connects to WhatsApp if not already connected using the saved session.
    /// Used by send/reply to establish a temporary connection.
    async fn ensure_connected(&self) -> anyhow::Result<()> {
        {
            let guard = self.client.lock().await;
            if guard.is_some() {
                return Ok(());
            }
        }

        info!(connection_id = %self.config_id, "starting WhatsApp connection for send");
        let backend = Arc::new(SqliteStore::new(&self.session_db_path).await?);
        let client_holder = Arc::clone(&self.client);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

        let mut bot = Bot::builder()
            .with_backend(backend)
            .with_transport_factory(TokioWebSocketTransportFactory::new())
            .with_http_client(UreqHttpClient::new())
            .on_event(move |event, client| {
                let client_holder = Arc::clone(&client_holder);
                let tx = tx.clone();
                async move {
                    {
                        let mut holder = client_holder.lock().await;
                        if holder.is_none() {
                            *holder = Some(client);
                        }
                    }
                    let _ = tx.send(event);
                }
            })
            .build()
            .await?;

        let bot_future = bot.run().await?;

        tokio::select! {
            _ = bot_future => {
                anyhow::bail!("WhatsApp disconnected before connecting");
            }
            result = async {
                loop {
                    match rx.recv().await {
                        Some(Event::Connected(_)) => {
                            info!("WhatsApp connected for send");
                            return Ok::<(), anyhow::Error>(());
                        }
                        Some(Event::PairError(e)) => {
                            error!(connection_id = %self.config_id, error = ?e, "WhatsApp PairError");
                            return Err(anyhow::anyhow!("Auth error: {:?}. Run `void setup` first.", e));
                        }
                        Some(Event::LoggedOut(_)) => {
                            error!(connection_id = %self.config_id, "WhatsApp LoggedOut");
                            return Err(anyhow::anyhow!("Session expired. Run `void setup` to re-authenticate."));
                        }
                        None => {
                            error!(connection_id = %self.config_id, "WhatsApp connection closed unexpectedly");
                            return Err(anyhow::anyhow!("Connection closed unexpectedly"));
                        }
                        _ => {}
                    }
                }
            } => {
                result?;
            }
        }

        Ok(())
    }
}
