use std::path::Path;

use grammers_client::client::Client;
use grammers_client::media::Downloadable;
use grammers_client::message::InputMessage;

use crate::error::TelegramError;

pub(crate) async fn upload_and_build_media_message(
    client: &Client,
    path: &Path,
    caption: Option<&str>,
    mime_type: Option<&str>,
) -> anyhow::Result<InputMessage> {
    let uploaded = client.upload_file(path).await?;
    let caption_text = caption.unwrap_or("");

    let is_image = mime_type.is_some_and(|m| m.starts_with("image/"))
        || path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| matches!(ext, "jpg" | "jpeg" | "png" | "gif" | "webp"));

    let msg = if is_image {
        InputMessage::new().text(caption_text).photo(uploaded)
    } else {
        InputMessage::new().text(caption_text).document(uploaded)
    };

    Ok(msg)
}

pub(crate) async fn download_media_to_bytes<D: Downloadable>(
    client: &Client,
    downloadable: &D,
) -> Result<Vec<u8>, TelegramError> {
    let mut bytes = Vec::new();
    let mut download = client.iter_download(downloadable);
    while let Some(chunk) = download
        .next()
        .await
        .map_err(|e| TelegramError::Media(e.to_string()))?
    {
        bytes.extend(chunk);
    }
    Ok(bytes)
}
