//! Unit tests for WhatsApp RPC client/server integration.

use std::path::Path;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use void_core::models::MessageContent;

use crate::connector::WhatsAppConnector;
use crate::rpc::client::{reply_message, send_message};
use crate::rpc::protocol::{RpcContent, RpcMethod, RpcRequest, RpcResponseBody};
use crate::rpc::Server;

async fn start_test_server(
    dir: &Path,
    connection_id: &str,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let server = Server::new(dir);
    let connector = Arc::new(WhatsAppConnector::new(
        connection_id,
        dir.join(format!("whatsapp-{connection_id}.db"))
            .to_str()
            .unwrap(),
    ));
    server.register(connection_id, connector).await;
    let cancel = CancellationToken::new();
    let cancel_bg = cancel.clone();
    let handle = tokio::spawn(async move {
        server.run(cancel_bg).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    (cancel, handle)
}

#[tokio::test]
async fn client_send_returns_sync_not_ready_without_live_client() {
    let dir = std::env::temp_dir().join(format!("void-wa-rpc-client-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let (cancel, server) = start_test_server(&dir, "whatsapp").await;

    let err = send_message(
        &dir,
        "whatsapp",
        "33612345678",
        MessageContent::Text("hello".into()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("not ready") || err.contains("not connected"),
        "{err}"
    );

    cancel.cancel();
    server.await.unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn client_reply_routes_to_registered_connection() {
    let dir = std::env::temp_dir().join(format!("void-wa-rpc-reply-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let (cancel, server) = start_test_server(&dir, "whatsapp").await;

    let err = reply_message(
        &dir,
        "whatsapp",
        "120363@g.us:MSG123",
        MessageContent::Text("reply".into()),
        true,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("not ready") || err.contains("not connected"),
        "{err}"
    );

    cancel.cancel();
    server.await.unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn client_unknown_connection_returns_error() {
    let dir = std::env::temp_dir().join(format!("void-wa-rpc-unknown-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let (cancel, server) = start_test_server(&dir, "whatsapp").await;

    let err = send_message(
        &dir,
        "other-account",
        "336",
        MessageContent::Text("x".into()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("other-account"), "{err}");

    cancel.cancel();
    server.await.unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn client_send_surfaces_connect_error_when_no_server() {
    // A store dir with no running server: the client must fail to connect
    // rather than hang or panic.
    let dir = std::env::temp_dir().join(format!("void-wa-rpc-noserver-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let err = send_message(
        &dir,
        "whatsapp",
        "33612345678",
        MessageContent::Text("hi".into()),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(err.contains("failed to connect"), "{err}");

    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(unix)]
#[tokio::test]
async fn end_to_end_protocol_on_unix_socket() {
    use crate::rpc::path::endpoint_path;

    let dir = std::env::temp_dir().join(format!("void-wa-rpc-e2e-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let (cancel, server) = start_test_server(&dir, "wa").await;

    let req = RpcRequest {
        id: 99,
        connection_id: "wa".into(),
        method: RpcMethod::Send {
            to: "336".into(),
            content: RpcContent::Text {
                text: "ping".into(),
            },
        },
    };

    let mut stream = tokio::net::UnixStream::connect(endpoint_path(&dir))
        .await
        .unwrap();
    stream
        .write_all(req.encode_line().unwrap().as_bytes())
        .await
        .unwrap();
    stream.shutdown().await.ok();
    let mut buf = String::new();
    stream.read_to_string(&mut buf).await.unwrap();
    let resp = crate::rpc::protocol::RpcResponse::decode_line(&buf).unwrap();
    assert_eq!(resp.id, 99);
    assert!(matches!(resp.body, RpcResponseBody::Error { .. }));

    cancel.cancel();
    server.await.unwrap();
    std::fs::remove_dir_all(&dir).ok();
}
