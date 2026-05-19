use crate::api::UnipileMessage;

pub(crate) fn extract_text(msg: &UnipileMessage) -> Option<String> {
    msg.text
        .as_ref()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
}

pub(crate) fn extract_media_type(msg: &UnipileMessage) -> Option<String> {
    let att = msg.attachments.as_ref()?.first()?;
    Some(match att.r#type.as_deref() {
        Some("img") => "image".to_string(),
        Some("video") => "video".to_string(),
        Some("audio") => "audio".to_string(),
        Some("file") | Some("document") => "document".to_string(),
        _ => att
            .mimetype
            .as_deref()
            .map(|m| {
                if m.starts_with("image/") {
                    "image".to_string()
                } else if m.starts_with("video/") {
                    "video".to_string()
                } else if m.starts_with("audio/") {
                    "audio".to_string()
                } else {
                    "document".to_string()
                }
            })
            .unwrap_or_else(|| "document".to_string()),
    })
}

pub(crate) fn extract_media_metadata(msg: &UnipileMessage) -> Option<serde_json::Value> {
    let att = msg.attachments.as_ref()?.first()?;
    Some(serde_json::json!({
        "message_id": msg.id,
        "provider_id": msg.provider_id,
        "attachment_id": att.id,
        "mimetype": att.mimetype,
        "type": att.r#type,
        "url": att.url,
    }))
}

pub(crate) fn parse_timestamp(ts: Option<&str>) -> i64 {
    ts.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{UnipileAttachment, UnipileMessage};

    #[test]
    fn extract_text_trims_and_skips_empty() {
        let with_text = UnipileMessage {
            text: Some("  hello  \n".into()),
            ..Default::default()
        };
        assert_eq!(extract_text(&with_text).as_deref(), Some("hello"));

        let empty = UnipileMessage {
            text: Some("   ".into()),
            ..Default::default()
        };
        assert!(extract_text(&empty).is_none());
    }

    #[test]
    fn extract_media_type_from_type_and_mimetype() {
        let img = UnipileMessage {
            attachments: Some(vec![UnipileAttachment {
                id: "a1".into(),
                r#type: Some("img".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_media_type(&img).as_deref(), Some("image"));

        let mime = UnipileMessage {
            attachments: Some(vec![UnipileAttachment {
                id: "a2".into(),
                mimetype: Some("video/mp4".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(extract_media_type(&mime).as_deref(), Some("video"));
    }

    #[test]
    fn extract_media_metadata_includes_ids() {
        let msg = UnipileMessage {
            id: "m1".into(),
            provider_id: Some("prov-1".into()),
            attachments: Some(vec![UnipileAttachment {
                id: "att-1".into(),
                mimetype: Some("application/pdf".into()),
                r#type: Some("file".into()),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let meta = extract_media_metadata(&msg).unwrap();
        assert_eq!(meta["message_id"], "m1");
        assert_eq!(meta["attachment_id"], "att-1");
    }

    #[test]
    fn parse_timestamp_from_rfc3339() {
        let ts = parse_timestamp(Some("2026-05-19T11:41:45.871Z"));
        assert_eq!(ts, 1_779_190_905);
    }
}
