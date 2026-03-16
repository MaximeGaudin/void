//! Extraction of text, media type, and metadata from WhatsApp messages.

use wa_rs::proto_helpers::MessageExt;
use wa_rs_proto::whatsapp::Message as WaMessage;

pub(crate) fn extract_text(msg: &WaMessage) -> Option<String> {
    let base = msg.get_base_message();

    if let Some(text) = base.text_content() {
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }

    if let Some(caption) = base.get_caption() {
        if !caption.is_empty() {
            return Some(caption.to_string());
        }
    }

    if let Some(ref loc) = base.location_message {
        return Some(
            loc.name
                .clone()
                .unwrap_or_else(|| "📍 Location".to_string()),
        );
    }
    if let Some(ref contact) = base.contact_message {
        return Some(format!(
            "👤 {}",
            contact.display_name.as_deref().unwrap_or("Contact")
        ));
    }
    if let Some(ref contacts) = base.contacts_array_message {
        let count = contacts.contacts.len();
        return Some(format!("👥 {count} contacts"));
    }
    if base.sticker_message.is_some() || base.lottie_sticker_message.is_some() {
        return Some("🖼️ Sticker".to_string());
    }
    if base.audio_message.is_some() {
        return Some("🎵 Audio".to_string());
    }
    if base.ptv_message.is_some() {
        return Some("🎥 Video note".to_string());
    }

    if let Some(ref poll) = base.poll_creation_message {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v2 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v3 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if let Some(ref poll) = base.poll_creation_message_v5 {
        return Some(format!("📊 {}", poll.name.as_deref().unwrap_or("Poll")));
    }
    if base.poll_update_message.is_some() {
        return Some("📊 Poll vote".to_string());
    }

    if let Some(ref invite) = base.group_invite_message {
        return Some(format!(
            "👥 Group invite: {}",
            invite.group_name.as_deref().unwrap_or("group")
        ));
    }

    if let Some(ref live_loc) = base.live_location_message {
        return Some(
            live_loc
                .caption
                .clone()
                .unwrap_or_else(|| "📍 Live location".to_string()),
        );
    }

    if base.call.is_some() || base.bcall_message.is_some() {
        return Some("📞 Call".to_string());
    }
    if base.call_log_messsage.is_some() {
        return Some("📞 Call".to_string());
    }

    if let Some(ref event) = base.event_message {
        return Some(format!("📅 {}", event.name.as_deref().unwrap_or("Event")));
    }

    if base.image_message.is_some() {
        return Some("📷 Image".to_string());
    }
    if base.video_message.is_some() {
        return Some("🎬 Video".to_string());
    }
    if let Some(ref doc) = base.document_message {
        let name = doc
            .file_name
            .as_deref()
            .or(doc.title.as_deref())
            .unwrap_or("Document");
        return Some(format!("📄 {name}"));
    }

    None
}

pub(crate) fn extract_media_type(msg: &WaMessage) -> Option<String> {
    let base = msg.get_base_message();
    if base.image_message.is_some() {
        return Some("image".into());
    }
    if base.video_message.is_some() || base.ptv_message.is_some() {
        return Some("video".into());
    }
    if base.audio_message.is_some() {
        return Some("audio".into());
    }
    if base.document_message.is_some() {
        return Some("document".into());
    }
    if base.sticker_message.is_some() || base.lottie_sticker_message.is_some() {
        return Some("sticker".into());
    }
    if base.location_message.is_some() || base.live_location_message.is_some() {
        return Some("location".into());
    }
    if base.contact_message.is_some() || base.contacts_array_message.is_some() {
        return Some("contact".into());
    }
    if base.poll_creation_message.is_some()
        || base.poll_creation_message_v2.is_some()
        || base.poll_creation_message_v3.is_some()
        || base.poll_creation_message_v5.is_some()
        || base.poll_update_message.is_some()
    {
        return Some("poll".into());
    }
    if base.group_invite_message.is_some() {
        return Some("invite".into());
    }
    if base.call.is_some() || base.call_log_messsage.is_some() || base.bcall_message.is_some() {
        return Some("call".into());
    }
    if base.event_message.is_some() {
        return Some("event".into());
    }
    None
}

pub(crate) fn insert_download_fields(
    meta: &mut serde_json::Map<String, serde_json::Value>,
    media_key: &Option<Vec<u8>>,
    file_sha256: &Option<Vec<u8>>,
    file_enc_sha256: &Option<Vec<u8>>,
) {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    if let Some(ref key) = media_key {
        meta.insert("media_key".into(), serde_json::json!(STANDARD.encode(key)));
    }
    if let Some(ref sha) = file_sha256 {
        meta.insert(
            "file_sha256".into(),
            serde_json::json!(STANDARD.encode(sha)),
        );
    }
    if let Some(ref sha) = file_enc_sha256 {
        meta.insert(
            "file_enc_sha256".into(),
            serde_json::json!(STANDARD.encode(sha)),
        );
    }
}

pub(crate) fn extract_media_metadata(msg: &WaMessage) -> Option<serde_json::Value> {
    let base = msg.get_base_message();

    if let Some(ref doc) = base.document_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref name) = doc.file_name {
            meta.insert("file_name".into(), serde_json::json!(name));
        }
        if let Some(ref mime) = doc.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = doc.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(ref title) = doc.title {
            meta.insert("title".into(), serde_json::json!(title));
        }
        if let Some(pages) = doc.page_count {
            meta.insert("page_count".into(), serde_json::json!(pages));
        }
        if let Some(ref path) = doc.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("document"));
        insert_download_fields(
            &mut meta,
            &doc.media_key,
            &doc.file_sha256,
            &doc.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref img) = base.image_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = img.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = img.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(w) = img.width {
            meta.insert("width".into(), serde_json::json!(w));
        }
        if let Some(h) = img.height {
            meta.insert("height".into(), serde_json::json!(h));
        }
        if let Some(ref path) = img.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("image"));
        insert_download_fields(
            &mut meta,
            &img.media_key,
            &img.file_sha256,
            &img.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref vid) = base.video_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = vid.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = vid.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(secs) = vid.seconds {
            meta.insert("duration_secs".into(), serde_json::json!(secs));
        }
        if let Some(w) = vid.width {
            meta.insert("width".into(), serde_json::json!(w));
        }
        if let Some(h) = vid.height {
            meta.insert("height".into(), serde_json::json!(h));
        }
        if let Some(ref path) = vid.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("video"));
        insert_download_fields(
            &mut meta,
            &vid.media_key,
            &vid.file_sha256,
            &vid.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    if let Some(ref aud) = base.audio_message {
        let mut meta = serde_json::Map::new();
        if let Some(ref mime) = aud.mimetype {
            meta.insert("mimetype".into(), serde_json::json!(mime));
        }
        if let Some(size) = aud.file_length {
            meta.insert("file_size".into(), serde_json::json!(size));
        }
        if let Some(secs) = aud.seconds {
            meta.insert("duration_secs".into(), serde_json::json!(secs));
        }
        if let Some(ptt) = aud.ptt {
            meta.insert("voice_note".into(), serde_json::json!(ptt));
        }
        if let Some(ref path) = aud.direct_path {
            meta.insert("direct_path".into(), serde_json::json!(path));
        }
        meta.insert("media_type".into(), serde_json::json!("audio"));
        insert_download_fields(
            &mut meta,
            &aud.media_key,
            &aud.file_sha256,
            &aud.file_enc_sha256,
        );
        if !meta.is_empty() {
            return Some(serde_json::Value::Object(meta));
        }
    }

    None
}

pub(crate) fn extract_quoted_id(msg: &WaMessage) -> Option<String> {
    if let Some(ref ext) = msg.extended_text_message {
        if let Some(ref ctx) = ext.context_info {
            return ctx.stanza_id.clone();
        }
    }
    None
}
