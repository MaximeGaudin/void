//! Send/reply/download using the sync daemon's live WhatsApp client.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tracing::{debug, info};
use void_core::models::MessageContent;
use wa_rs::client::Client;
use wa_rs::send::SendOptions;
use wa_rs_proto::whatsapp::ContextInfo;

use super::media::{download_media_with_client, upload_and_build_media_message};
use super::self_chat::send_self_chat_message;
use super::send::{build_wa_message, parse_jid};
use super::WhatsAppConnector;
use crate::rpc::rpc_to_message_content;
use crate::rpc::{RpcMethod, RpcResult};
use void_core::models::parse_reply_id;

impl WhatsAppConnector {
    pub fn connection_id(&self) -> &str {
        &self.config_id
    }

    pub(crate) async fn require_sync_client(&self) -> anyhow::Result<Arc<Client>> {
        self.client.lock().await.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "WhatsApp sync connection not ready for '{}'. Wait for sync to connect.",
                self.config_id
            )
        })
    }

    pub async fn dispatch_rpc(&self, method: RpcMethod) -> anyhow::Result<RpcResult> {
        match method {
            RpcMethod::Send { to, content } => {
                let message_id = self
                    .send_via_sync(&to, rpc_to_message_content(content))
                    .await?;
                Ok(RpcResult::MessageId { message_id })
            }
            RpcMethod::Reply {
                message_id,
                content,
                in_thread,
            } => {
                let sent_id = self
                    .reply_via_sync(&message_id, rpc_to_message_content(content), in_thread)
                    .await?;
                Ok(RpcResult::MessageId {
                    message_id: sent_id,
                })
            }
            RpcMethod::DownloadMedia { params } => {
                let data = self
                    .download_media_via_sync(
                        &params.direct_path,
                        &params.media_key,
                        &params.file_sha256,
                        &params.file_enc_sha256,
                        params.file_length,
                        &params.media_type,
                    )
                    .await?;
                Ok(RpcResult::MediaBytes {
                    data_base64: STANDARD.encode(data),
                })
            }
        }
    }

    pub async fn send_via_sync(&self, to: &str, content: MessageContent) -> anyhow::Result<String> {
        let client = self.require_sync_client().await?;
        let identity = self.own_identity.lock().expect("mutex").clone();

        if identity.should_route_as_self_chat(to) {
            return send_self_chat_message(&client, &identity, content, None).await;
        }

        let jid = parse_jid(to)?;
        info!(connection_id = %self.config_id, recipient_jid = %jid, "sending WhatsApp message via sync");

        let msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                upload_and_build_media_message(
                    &client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                    None,
                )
                .await?
            }
            _ => build_wa_message(&content, None)?,
        };

        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        debug!(connection_id = %self.config_id, message_id = %msg_id, "WhatsApp message sent via sync");
        Ok(msg_id)
    }

    pub async fn reply_via_sync(
        &self,
        message_id: &str,
        content: MessageContent,
        in_thread: bool,
    ) -> anyhow::Result<String> {
        let (chat_jid_str, quoted_msg_id) = parse_reply_id(message_id)?;
        info!(
            connection_id = %self.config_id,
            reply_target = %chat_jid_str,
            quoted_msg_id = %quoted_msg_id,
            in_thread,
            "sending WhatsApp reply via sync"
        );

        let client = self.require_sync_client().await?;
        let identity = self.own_identity.lock().expect("mutex").clone();

        if identity.should_route_as_self_chat(&chat_jid_str) {
            let context_info = if in_thread {
                Some(ContextInfo {
                    stanza_id: Some(quoted_msg_id.clone()),
                    ..Default::default()
                })
            } else {
                None
            };
            info!(
                connection_id = %self.config_id,
                quoted_msg_id = %quoted_msg_id,
                in_thread,
                "sending WhatsApp notes-to-self reply via sync"
            );
            return send_self_chat_message(&client, &identity, content, context_info).await;
        }

        let jid = parse_jid(&chat_jid_str)?;

        let context_info = if in_thread {
            Some(ContextInfo {
                stanza_id: Some(quoted_msg_id),
                ..Default::default()
            })
        } else {
            None
        };

        let msg = match &content {
            MessageContent::File {
                path,
                caption,
                mime_type,
            } => {
                upload_and_build_media_message(
                    &client,
                    path,
                    caption.as_deref(),
                    mime_type.as_deref(),
                    context_info,
                )
                .await?
            }
            _ => build_wa_message(&content, context_info)?,
        };

        let msg_id = client
            .send_message_with_options(jid, msg, SendOptions::default())
            .await?;
        debug!(connection_id = %self.config_id, message_id = %msg_id, "WhatsApp reply sent via sync");
        Ok(msg_id)
    }

    pub async fn download_media_via_sync(
        &self,
        direct_path: &str,
        media_key_b64: &str,
        file_sha256_b64: &str,
        file_enc_sha256_b64: &str,
        file_length: u64,
        media_type_str: &str,
    ) -> Result<Vec<u8>, crate::error::WhatsAppError> {
        let client = self.require_sync_client().await.map_err(|e| {
            crate::error::WhatsAppError::Connection(format!(
                "WhatsApp sync connection not ready for '{}': {e}",
                self.config_id
            ))
        })?;
        download_media_with_client(
            &client,
            direct_path,
            media_key_b64,
            file_sha256_b64,
            file_enc_sha256_b64,
            file_length,
            media_type_str,
        )
        .await
    }
}
