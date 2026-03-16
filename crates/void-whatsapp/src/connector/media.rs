//! Media handling: type detection, upload, and download.

use anyhow::Context;
use wa_rs::client::Client;
use wa_rs::download::MediaType as WaMediaType;
use wa_rs_proto::whatsapp::message::{
    AudioMessage, DocumentMessage, ImageMessage, VideoMessage,
};
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

use super::WhatsAppConnector;

/// Maps MIME type and filename to wa_rs MediaType and default MIME string.
pub(crate) fn determine_media_type(mime: Option<&str>, filename: &str) -> (WaMediaType, &'static str) {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if let Some(m) = mime {
        let m_lower = m.to_lowercase();
        if m_lower.starts_with("image/") {
            return (WaMediaType::Image, "image/jpeg");
        }
        if m_lower.starts_with("video/") {
            return (WaMediaType::Video, "video/mp4");
        }
        if m_lower.starts_with("audio/") {
            return (WaMediaType::Audio, "audio/ogg; codecs=opus");
        }
    }

    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" => (WaMediaType::Image, "image/jpeg"),
        "mp4" | "mov" | "avi" => (WaMediaType::Video, "video/mp4"),
        "ogg" | "mp3" | "m4a" | "wav" | "opus" => (WaMediaType::Audio, "audio/ogg; codecs=opus"),
        _ => (WaMediaType::Document, "application/octet-stream"),
    }
}

/// Uploads a file to WhatsApp and builds the appropriate WaMessage.
pub(crate) async fn upload_and_build_media_message(
    client: &Client,
    path: &std::path::Path,
    caption: Option<&str>,
    mime_type: Option<&str>,
    context_info: Option<ContextInfo>,
) -> anyhow::Result<WaMessage> {
    let data = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read file {}", path.display()))?;
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let (media_type, default_mime) = determine_media_type(mime_type, filename);
    let mime = mime_type.unwrap_or(default_mime);

    let upload = client
        .upload(data, media_type)
        .await
        .context("WhatsApp media upload failed")?;

    #[allow(clippy::wildcard_in_or_patterns)]
    let msg = match media_type {
        WaMediaType::Image => WaMessage {
            image_message: Some(Box::new(ImageMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key),
                file_sha256: Some(upload.file_sha256),
                file_enc_sha256: Some(upload.file_enc_sha256),
                file_length: Some(upload.file_length),
                mimetype: Some(mime.to_string()),
                caption: caption.map(|c| c.to_string()),
                context_info: context_info.map(Box::new),
                ..Default::default()
            })),
            ..Default::default()
        },
        WaMediaType::Video => WaMessage {
            video_message: Some(Box::new(VideoMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key),
                file_sha256: Some(upload.file_sha256),
                file_enc_sha256: Some(upload.file_enc_sha256),
                file_length: Some(upload.file_length),
                mimetype: Some(mime.to_string()),
                caption: caption.map(|c| c.to_string()),
                context_info: context_info.map(Box::new),
                ..Default::default()
            })),
            ..Default::default()
        },
        WaMediaType::Audio => WaMessage {
            audio_message: Some(Box::new(AudioMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key),
                file_sha256: Some(upload.file_sha256),
                file_enc_sha256: Some(upload.file_enc_sha256),
                file_length: Some(upload.file_length),
                mimetype: Some(mime.to_string()),
                context_info: context_info.map(Box::new),
                ..Default::default()
            })),
            ..Default::default()
        },
        _ => WaMessage {
            document_message: Some(Box::new(DocumentMessage {
                url: Some(upload.url),
                direct_path: Some(upload.direct_path),
                media_key: Some(upload.media_key),
                file_sha256: Some(upload.file_sha256),
                file_enc_sha256: Some(upload.file_enc_sha256),
                file_length: Some(upload.file_length),
                mimetype: Some(mime.to_string()),
                file_name: Some(filename.to_string()),
                context_info: context_info.map(Box::new),
                ..Default::default()
            })),
            ..Default::default()
        },
    };

    Ok(msg)
}

impl WhatsAppConnector {
    /// Download encrypted media from WhatsApp using direct_path and keys.
    pub async fn download_media(
        &self,
        direct_path: &str,
        media_key_b64: &str,
        file_sha256_b64: &str,
        file_enc_sha256_b64: &str,
        file_length: u64,
        media_type_str: &str,
    ) -> Result<Vec<u8>, crate::error::WhatsAppError> {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        self.ensure_connected()
            .await
            .map_err(|e| crate::error::WhatsAppError::Connection(e.to_string()))?;
        let guard = self.client.lock().await;
        let client = guard.as_ref().ok_or_else(|| {
            crate::error::WhatsAppError::Connection("WhatsApp not connected".into())
        })?;

        let media_key = STANDARD
            .decode(media_key_b64)
            .map_err(|e| crate::error::WhatsAppError::Decode(e.to_string()))?;
        let file_sha256 = STANDARD
            .decode(file_sha256_b64)
            .map_err(|e| crate::error::WhatsAppError::Decode(e.to_string()))?;
        let file_enc_sha256 = STANDARD
            .decode(file_enc_sha256_b64)
            .map_err(|e| crate::error::WhatsAppError::Decode(e.to_string()))?;

        let media_type = match media_type_str {
            "image" => WaMediaType::Image,
            "video" => WaMediaType::Video,
            "audio" => WaMediaType::Audio,
            "document" => WaMediaType::Document,
            "sticker" => WaMediaType::Sticker,
            other => {
                return Err(crate::error::WhatsAppError::Media(format!(
                    "unsupported media type: {other}"
                )))
            }
        };

        let data = client
            .download_from_params(
                direct_path,
                &media_key,
                &file_sha256,
                &file_enc_sha256,
                file_length,
                media_type,
            )
            .await
            .map_err(|e| crate::error::WhatsAppError::Media(format!("download failed: {e}")))?;

        Ok(data)
    }
}
