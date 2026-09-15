//! Media handling: type detection, upload, and download.

use anyhow::Context;
use wa_rs::client::Client;
use wa_rs::download::MediaType as WaMediaType;
use wa_rs_proto::whatsapp::message::{AudioMessage, DocumentMessage, ImageMessage, VideoMessage};
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

use super::WhatsAppConnector;

/// Maps MIME type and filename to wa_rs MediaType and default MIME string.
pub(crate) fn determine_media_type(
    mime: Option<&str>,
    filename: &str,
) -> (WaMediaType, &'static str) {
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
        // HEIC/HEIF (the iPhone default) is transcoded to JPEG before upload,
        // so we announce image/jpeg for it. See `prepare_image`.
        "jpg" | "jpeg" | "heic" | "heif" => (WaMediaType::Image, "image/jpeg"),
        "png" => (WaMediaType::Image, "image/png"),
        "gif" => (WaMediaType::Image, "image/gif"),
        "webp" => (WaMediaType::Image, "image/webp"),
        "mp4" | "mov" | "avi" => (WaMediaType::Video, "video/mp4"),
        "ogg" | "mp3" | "m4a" | "wav" | "opus" => (WaMediaType::Audio, "audio/ogg; codecs=opus"),
        _ => (WaMediaType::Document, "application/octet-stream"),
    }
}

/// An image ready to be uploaded: decoded, possibly transcoded, measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedImage {
    /// Bytes to upload. Same as the input unless the source was transcoded.
    pub bytes: Vec<u8>,
    /// MIME string that honestly describes `bytes`.
    pub mime: String,
    pub width: u32,
    pub height: u32,
    /// Small JPEG preview shown before the full image is downloaded.
    pub thumbnail: Option<Vec<u8>>,
}

/// Decodes an image, transcoding HEIC/HEIF to JPEG, and measures it.
///
/// Pure: no network, no client, no filesystem. `declared_mime` is what
/// `determine_media_type` announced for these bytes (or an extension-aware
/// override such as `image/heic`).
///
/// HEIC/HEIF cannot be rendered by WhatsApp clients, so those bytes are
/// re-encoded as JPEG. Every other format is uploaded untouched: re-encoding
/// a photo the user picked would silently lose quality.
pub(crate) fn prepare_image(data: Vec<u8>, declared_mime: &str) -> anyhow::Result<PreparedImage> {
    register_heif_hooks();

    let source_is_heif = looks_like_heif(&data)
        || declared_mime.eq_ignore_ascii_case("image/heic")
        || declared_mime.eq_ignore_ascii_case("image/heif");

    // ImageReader (used by load_from_memory) consults the libheif hooks.
    // The free-function guess_format does not: never use it to detect HEIC.
    let decoded = image::load_from_memory(&data).context("failed to decode image data")?;
    let (width, height) = (decoded.width(), decoded.height());

    // WhatsApp clients cannot decode HEIC, so transcode. Announcing image/jpeg
    // over HEIC bytes would be worse than the Document fallback it replaces:
    // a broken photo instead of an openable attachment.
    let (bytes, mime) = if source_is_heif {
        (
            encode_jpeg(&decoded, JPEG_QUALITY)?,
            "image/jpeg".to_string(),
        )
    } else {
        (data, declared_mime.to_string())
    };

    let thumbnail = build_thumbnail(&decoded);

    Ok(PreparedImage {
        bytes,
        mime,
        width,
        height,
        thumbnail,
    })
}

/// Quality used when we have to re-encode. 85 is the usual quality/size knee.
const JPEG_QUALITY: u8 = 85;

/// Longest edge of the inline preview, in pixels.
const THUMBNAIL_MAX_EDGE: u32 = 200;

/// Registers libheif's decoders into the `image` crate, exactly once.
///
/// The `image` crate does not decode HEIC on its own (image-rs/image#1375).
fn register_heif_hooks() {
    static HOOKS: std::sync::Once = std::sync::Once::new();
    HOOKS.call_once(libheif_rs::integration::image::register_all_decoding_hooks);
}

/// ISO BMFF `ftyp` brands that mean "this is HEIF-family, not a plain JPEG".
///
/// iPhone camera output uses major brand `heic`. Structural brands `mif1` /
/// `mif2` also appear. We deliberately omit `avif`: that is a different codec
/// and must not be silently JPEG-transcoded.
fn looks_like_heif(data: &[u8]) -> bool {
    const BRANDS: &[&[u8; 4]] = &[
        b"heic", b"heix", b"heif", b"hevc", b"hevx", b"mif1", b"mif2",
    ];
    if data.len() < 12 || &data[4..8] != b"ftyp" {
        return false;
    }
    if BRANDS.iter().any(|b| &data[8..12] == *b) {
        return true;
    }
    // Compatible brands start at offset 16, four bytes each.
    let end = data.len().min(64);
    let mut off = 16;
    while off + 4 <= end {
        if BRANDS.iter().any(|b| &data[off..off + 4] == *b) {
            return true;
        }
        off += 4;
    }
    false
}

fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    // JPEG has no alpha channel; drop it rather than letting the encoder fail.
    image::DynamicImage::ImageRgb8(img.to_rgb8())
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut out, quality,
        ))
        .context("failed to encode JPEG")?;
    Ok(out.into_inner())
}

/// Small JPEG preview shown by receiving clients before the full download.
///
/// Returns None rather than failing the send: a missing preview is a cosmetic
/// loss, a failed send is not.
fn build_thumbnail(img: &image::DynamicImage) -> Option<Vec<u8>> {
    let thumb = img.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE);
    match encode_jpeg(&thumb, 70) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!("could not build image thumbnail: {e}");
            None
        }
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

    // Images go through prepare_image so HEIC is transcoded and the
    // ImageMessage carries real dimensions + a jpeg_thumbnail.
    let (data, mime, width, height, thumbnail) = if media_type == WaMediaType::Image {
        let prepare_mime = heic_aware_declared_mime(filename, mime_type, default_mime);
        let prepared = prepare_image(data, prepare_mime)
            .with_context(|| format!("failed to prepare image {}", path.display()))?;
        (
            prepared.bytes,
            prepared.mime,
            Some(prepared.width),
            Some(prepared.height),
            prepared.thumbnail,
        )
    } else {
        (
            data,
            mime_type.unwrap_or(default_mime).to_string(),
            None,
            None,
            None,
        )
    };

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
                mimetype: Some(mime),
                caption: caption.map(|c| c.to_string()),
                width,
                height,
                jpeg_thumbnail: thumbnail,
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
                mimetype: Some(mime),
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
                mimetype: Some(mime),
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
                mimetype: Some(mime),
                file_name: Some(filename.to_string()),
                context_info: context_info.map(Box::new),
                ..Default::default()
            })),
            ..Default::default()
        },
    };

    Ok(msg)
}

/// MIME passed to `prepare_image` for HEIC/HEIF detection.
///
/// `determine_media_type` announces `image/jpeg` for `.heic` because we
/// transcode before upload. That announcement alone cannot tell
/// `prepare_image` the *source* is HEIC, so the extension fills the gap
/// when the caller did not pass an explicit HEIC MIME.
fn heic_aware_declared_mime<'a>(
    filename: &str,
    mime_type: Option<&'a str>,
    default_mime: &'a str,
) -> &'a str {
    if let Some(m) = mime_type {
        return m;
    }
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "heic" => "image/heic",
        "heif" => "image/heif",
        _ => default_mime,
    }
}

pub(crate) async fn download_media_with_client(
    client: &Client,
    direct_path: &str,
    media_key_b64: &str,
    file_sha256_b64: &str,
    file_enc_sha256_b64: &str,
    file_length: u64,
    media_type_str: &str,
) -> Result<Vec<u8>, crate::error::WhatsAppError> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

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

    client
        .download_from_params(
            direct_path,
            &media_key,
            &file_sha256,
            &file_enc_sha256,
            file_length,
            media_type,
        )
        .await
        .map_err(|e| crate::error::WhatsAppError::Media(format!("download failed: {e}")))
}

impl WhatsAppConnector {
    /// Download encrypted media from WhatsApp using direct_path and keys.
    /// Opens a standalone connection when the sync daemon is not running.
    pub async fn download_media(
        &self,
        direct_path: &str,
        media_key_b64: &str,
        file_sha256_b64: &str,
        file_enc_sha256_b64: &str,
        file_length: u64,
        media_type_str: &str,
    ) -> Result<Vec<u8>, crate::error::WhatsAppError> {
        self.ensure_connected()
            .await
            .map_err(|e| crate::error::WhatsAppError::Connection(e.to_string()))?;
        let guard = self.client.lock().await;
        let client = guard.as_ref().ok_or_else(|| {
            crate::error::WhatsAppError::Connection("WhatsApp not connected".into())
        })?;
        download_media_with_client(
            client,
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
