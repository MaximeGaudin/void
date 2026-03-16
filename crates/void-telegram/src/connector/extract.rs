use grammers_client::message::Message;

pub(crate) fn extract_text(msg: &Message) -> Option<String> {
    let text = msg.text().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub(crate) fn extract_media_type(msg: &Message) -> Option<String> {
    use grammers_client::media::Media;

    msg.media().map(|m| match m {
        Media::Photo(_) => "image".to_string(),
        Media::Sticker(_) => "sticker".to_string(),
        Media::Document(ref doc) => {
            let mime = doc.mime_type().unwrap_or("");
            if mime.starts_with("video/") {
                "video".to_string()
            } else if mime.starts_with("audio/") {
                "audio".to_string()
            } else {
                "document".to_string()
            }
        }
        Media::Contact(_) => "contact".to_string(),
        _ => "unknown".to_string(),
    })
}

pub(crate) fn extract_media_metadata(msg: &Message) -> Option<serde_json::Value> {
    use grammers_client::media::Media;

    msg.media().map(|m| match m {
        Media::Photo(photo) => serde_json::json!({
            "type": "photo",
            "photo_id": photo.id(),
        }),
        Media::Document(ref doc) => serde_json::json!({
            "type": "document",
            "document_id": doc.id(),
            "mime_type": doc.mime_type().unwrap_or(""),
            "file_name": doc.name(),
            "size": doc.size(),
        }),
        _ => serde_json::json!({ "type": "other" }),
    })
}
