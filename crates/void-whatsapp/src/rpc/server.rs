//! WhatsApp RPC server — serves send/reply/download via the sync daemon connection.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::connector::WhatsAppConnector;

use super::path::{endpoint_path, remove_stale_endpoint};
use super::protocol::{RpcRequest, RpcResponse};

pub struct Server {
    handlers: Arc<RwLock<HashMap<String, Arc<WhatsAppConnector>>>>,
    store_path: PathBuf,
}

impl Server {
    pub fn new(store_path: &Path) -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
            store_path: store_path.to_path_buf(),
        }
    }

    pub async fn register(&self, connection_id: &str, connector: Arc<WhatsAppConnector>) {
        self.handlers
            .write()
            .await
            .insert(connection_id.to_string(), connector);
    }

    pub async fn has_handlers(&self) -> bool {
        !self.handlers.read().await.is_empty()
    }

    pub async fn run(self, cancel: CancellationToken) -> anyhow::Result<()> {
        if !self.has_handlers().await {
            return Ok(());
        }

        let endpoint = endpoint_path(&self.store_path);
        remove_stale_endpoint(&endpoint);
        info!(endpoint = %display_endpoint(&endpoint), "starting WhatsApp RPC server");

        #[cfg(unix)]
        {
            use tokio::net::UnixListener;
            let listener = UnixListener::bind(&endpoint)?;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("WhatsApp RPC server shutting down");
                        break;
                    }
                    accept = listener.accept() => {
                        match accept {
                            Ok((stream, _)) => {
                                let handlers = Arc::clone(&self.handlers);
                                tokio::spawn(async move {
                                    if let Err(e) = handle_connection(stream, handlers).await {
                                        debug!("WhatsApp RPC connection error: {e}");
                                    }
                                });
                            }
                            Err(e) => {
                                warn!("WhatsApp RPC accept error: {e}");
                            }
                        }
                    }
                }
            }
            drop(listener);
            remove_stale_endpoint(&endpoint);
        }

        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            // Only the first instance of a named pipe may set `first_pipe_instance`;
            // every subsequent instance must be created without it. Create the next
            // instance before handing the connected one to the handler so the next
            // client can connect immediately.
            let mut server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(&endpoint)?;
            loop {
                // Resolve the connect (or cancellation) before touching `server`,
                // so its borrow ends before we move it into the handler.
                let connect = tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("WhatsApp RPC server shutting down");
                        break;
                    }
                    connect = server.connect() => connect,
                };
                let connected = server;
                server = ServerOptions::new().create(&endpoint)?;
                if let Err(e) = connect {
                    warn!("WhatsApp RPC pipe connect error: {e}");
                    continue;
                }
                let handlers = Arc::clone(&self.handlers);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(connected, handlers).await {
                        debug!("WhatsApp RPC connection error: {e}");
                    }
                });
            }
        }

        Ok(())
    }
}

fn display_endpoint(endpoint: &Endpoint) -> String {
    #[cfg(unix)]
    {
        endpoint.display().to_string()
    }
    #[cfg(windows)]
    {
        endpoint.clone()
    }
}

#[cfg(unix)]
type Endpoint = PathBuf;
#[cfg(windows)]
type Endpoint = String;

#[cfg(unix)]
type RpcStream = tokio::net::UnixStream;
#[cfg(windows)]
type RpcStream = tokio::net::windows::named_pipe::NamedPipeServer;

async fn handle_connection(
    stream: RpcStream,
    handlers: Arc<RwLock<HashMap<String, Arc<WhatsAppConnector>>>>,
) -> anyhow::Result<()> {
    // `tokio::io::split` works for any AsyncRead+AsyncWrite, including the
    // Windows `NamedPipeServer` (which has no `into_split`). The halves are used
    // in this same task, so the generic split is sufficient.
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();
    let line = match lines.next_line().await? {
        Some(line) if !line.trim().is_empty() => line,
        _ => anyhow::bail!("empty RPC request"),
    };

    let request = RpcRequest::decode_line(&line)?;
    let response = dispatch(&handlers, request).await;
    writer.write_all(response.encode_line()?.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

async fn dispatch(
    handlers: &RwLock<HashMap<String, Arc<WhatsAppConnector>>>,
    request: RpcRequest,
) -> RpcResponse {
    let id = request.id;
    let connection_id = request.connection_id.clone();
    let handler = handlers.read().await.get(&connection_id).cloned();
    let Some(connector) = handler else {
        return RpcResponse::error(
            id,
            format!("no WhatsApp connection registered for '{connection_id}'"),
        );
    };

    match connector.dispatch_rpc(request.method).await {
        Ok(result) => RpcResponse::ok(id, result),
        Err(e) => {
            error!(connection_id = %connection_id, error = %e, "WhatsApp RPC handler failed");
            RpcResponse::error(id, e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;

    use super::*;
    use crate::connector::WhatsAppConnector;
    use crate::rpc::protocol::{RpcContent, RpcMethod, RpcRequest, RpcResponseBody};

    #[tokio::test]
    async fn server_dispatches_send_to_registered_connector() {
        let dir = std::env::temp_dir().join(format!("void-wa-rpc-srv-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let session = dir.join("whatsapp-test.db");

        let server = Server::new(&dir);
        let connector = Arc::new(WhatsAppConnector::new("test", session.to_str().unwrap()));
        server.register("test", Arc::clone(&connector)).await;

        let cancel = CancellationToken::new();
        let cancel_bg = cancel.clone();
        let server_task = tokio::spawn(async move { server.run(cancel_bg).await.unwrap() });

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let req = RpcRequest {
            id: 42,
            connection_id: "test".into(),
            method: RpcMethod::Send {
                to: "33612345678".into(),
                content: RpcContent::Text { text: "hi".into() },
            },
        };

        let resp = rpc_round_trip(&dir, req).await.unwrap();
        cancel.cancel();
        server_task.await.unwrap();

        assert_eq!(resp.id, 42);
        match resp.body {
            RpcResponseBody::Error { error } => {
                assert!(
                    error.contains("not connected") || error.contains("WhatsApp"),
                    "unexpected error: {error}"
                );
            }
            RpcResponseBody::Ok { .. } => panic!("expected not-connected error"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn server_returns_error_for_unknown_connection() {
        let dir = std::env::temp_dir().join(format!("void-wa-rpc-srv2-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();

        let server = Server::new(&dir);
        let connector = Arc::new(WhatsAppConnector::new(
            "known",
            dir.join("whatsapp-known.db").to_str().unwrap(),
        ));
        server.register("known", connector).await;

        let cancel = CancellationToken::new();
        let cancel_bg = cancel.clone();
        let server_task = tokio::spawn(async move { server.run(cancel_bg).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let req = RpcRequest {
            id: 1,
            connection_id: "missing".into(),
            method: RpcMethod::Send {
                to: "336".into(),
                content: RpcContent::Text { text: "x".into() },
            },
        };
        let resp = rpc_round_trip(&dir, req).await.unwrap();
        cancel.cancel();
        server_task.await.unwrap();

        match resp.body {
            RpcResponseBody::Error { error } => assert!(error.contains("missing")),
            _ => panic!("expected unknown connection error"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn has_handlers_reflects_registration() {
        let dir = std::env::temp_dir().join(format!("void-wa-rpc-hh-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let server = Server::new(&dir);
        assert!(!server.has_handlers().await);
        let connector = Arc::new(WhatsAppConnector::new(
            "c",
            dir.join("c.db").to_str().unwrap(),
        ));
        server.register("c", connector).await;
        assert!(server.has_handlers().await);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn run_returns_immediately_without_handlers() {
        let dir = std::env::temp_dir().join(format!("void-wa-rpc-empty-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let server = Server::new(&dir);
        // No handlers registered: run must return Ok without binding an endpoint
        // and without waiting for cancellation.
        let cancel = CancellationToken::new();
        server.run(cancel).await.unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn empty_request_line_yields_error() {
        use tokio::net::UnixStream;

        let dir = std::env::temp_dir().join(format!("void-wa-rpc-emptyline-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let server = Server::new(&dir);
        let connector = Arc::new(WhatsAppConnector::new(
            "c",
            dir.join("c.db").to_str().unwrap(),
        ));
        server.register("c", connector).await;

        let cancel = CancellationToken::new();
        let cancel_bg = cancel.clone();
        let task = tokio::spawn(async move { server.run(cancel_bg).await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Send a blank line; the server should close the connection with no
        // response (handle_connection bails on the empty request).
        let mut stream = UnixStream::connect(endpoint_path(&dir)).await.unwrap();
        stream.write_all(b"\n").await.unwrap();
        stream.shutdown().await.ok();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).await.unwrap();
        assert!(buf.is_empty(), "expected no response, got: {buf}");

        cancel.cancel();
        task.await.unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    async fn rpc_round_trip(dir: &Path, request: RpcRequest) -> anyhow::Result<RpcResponse> {
        use tokio::net::UnixStream;
        let path = endpoint_path(dir);
        let mut stream = UnixStream::connect(&path).await?;
        stream.write_all(request.encode_line()?.as_bytes()).await?;
        stream.shutdown().await.ok();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).await?;
        RpcResponse::decode_line(&buf)
    }

    #[cfg(windows)]
    async fn rpc_round_trip(dir: &Path, request: RpcRequest) -> anyhow::Result<RpcResponse> {
        use tokio::net::windows::named_pipe::ClientOptions;
        let pipe = endpoint_path(dir);
        let mut client = ClientOptions::new().open(&pipe)?;
        client.write_all(request.encode_line()?.as_bytes()).await?;
        client.flush().await?;
        let mut buf = String::new();
        client.read_to_string(&mut buf).await?;
        RpcResponse::decode_line(&buf)
    }
}
