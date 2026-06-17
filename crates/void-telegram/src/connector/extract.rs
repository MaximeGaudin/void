use grammers_client::message::Message;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MediaExtractView {
    Photo {
        photo_id: i64,
    },
    Sticker,
    Document {
        document_id: i64,
        mime_type: Option<String>,
        file_name: Option<String>,
        size: Option<usize>,
    },
    Contact,
    Other,
}

pub(crate) trait MessageExtractView {
    fn text(&self) -> &str;
    fn media(&self) -> Option<MediaExtractView>;
}

impl MessageExtractView for Message {
    fn text(&self) -> &str {
        Message::text(self)
    }

    fn media(&self) -> Option<MediaExtractView> {
        use grammers_client::media::Media;

        Message::media(self).map(|m| match m {
            Media::Photo(photo) => MediaExtractView::Photo {
                photo_id: photo.id(),
            },
            Media::Sticker(_) => MediaExtractView::Sticker,
            Media::Document(ref doc) => MediaExtractView::Document {
                document_id: doc.id(),
                mime_type: doc.mime_type().map(str::to_string),
                file_name: doc.name().map(str::to_string),
                size: doc.size(),
            },
            Media::Contact(_) => MediaExtractView::Contact,
            _ => MediaExtractView::Other,
        })
    }
}

fn extract_text_from_view(msg: &dyn MessageExtractView) -> Option<String> {
    let text = msg.text().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_media_type_from_view(msg: &dyn MessageExtractView) -> Option<String> {
    msg.media().map(|m| match m {
        MediaExtractView::Photo { .. } => "image".to_string(),
        MediaExtractView::Sticker => "sticker".to_string(),
        MediaExtractView::Document { mime_type, .. } => {
            let mime = mime_type.as_deref().unwrap_or("");
            if mime.starts_with("video/") {
                "video".to_string()
            } else if mime.starts_with("audio/") {
                "audio".to_string()
            } else {
                "document".to_string()
            }
        }
        MediaExtractView::Contact => "contact".to_string(),
        MediaExtractView::Other => "unknown".to_string(),
    })
}

fn extract_media_metadata_from_view(msg: &dyn MessageExtractView) -> Option<serde_json::Value> {
    msg.media().map(|m| match m {
        MediaExtractView::Photo { photo_id } => serde_json::json!({
            "type": "photo",
            "photo_id": photo_id,
        }),
        MediaExtractView::Document {
            document_id,
            mime_type,
            file_name,
            size,
        } => serde_json::json!({
            "type": "document",
            "document_id": document_id,
            "mime_type": mime_type.unwrap_or_default(),
            "file_name": file_name.as_deref(),
            "size": size,
        }),
        _ => serde_json::json!({ "type": "other" }),
    })
}

pub(crate) fn extract_text(msg: &Message) -> Option<String> {
    extract_text_from_view(msg)
}

pub(crate) fn extract_media_type(msg: &Message) -> Option<String> {
    extract_media_type_from_view(msg)
}

pub(crate) fn extract_media_metadata(msg: &Message) -> Option<serde_json::Value> {
    extract_media_metadata_from_view(msg)
}

#[cfg(test)]
mod fake {
    use super::*;

    pub struct FakeMessage {
        pub text: String,
        pub media: Option<MediaExtractView>,
    }

    impl MessageExtractView for FakeMessage {
        fn text(&self) -> &str {
            &self.text
        }

        fn media(&self) -> Option<MediaExtractView> {
            self.media.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeMessage;
    use super::*;

    #[test]
    fn extract_text_returns_content() {
        let msg = FakeMessage {
            text: "hello".into(),
            media: None,
        };
        assert_eq!(extract_text_from_view(&msg).as_deref(), Some("hello"));
    }

    #[test]
    fn extract_text_skips_empty() {
        let msg = FakeMessage {
            text: String::new(),
            media: None,
        };
        assert!(extract_text_from_view(&msg).is_none());
    }

    #[test]
    fn extract_media_type_photo() {
        let msg = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Photo { photo_id: 42 }),
        };
        assert_eq!(extract_media_type_from_view(&msg).as_deref(), Some("image"));
    }

    #[test]
    fn extract_media_type_document_video_audio() {
        let video = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Document {
                document_id: 1,
                mime_type: Some("video/mp4".into()),
                file_name: Some("clip.mp4".into()),
                size: Some(1024),
            }),
        };
        assert_eq!(
            extract_media_type_from_view(&video).as_deref(),
            Some("video")
        );

        let audio = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Document {
                document_id: 2,
                mime_type: Some("audio/ogg".into()),
                file_name: None,
                size: None,
            }),
        };
        assert_eq!(
            extract_media_type_from_view(&audio).as_deref(),
            Some("audio")
        );

        let doc = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Document {
                document_id: 3,
                mime_type: Some("application/pdf".into()),
                file_name: Some("file.pdf".into()),
                size: Some(2048),
            }),
        };
        assert_eq!(
            extract_media_type_from_view(&doc).as_deref(),
            Some("document")
        );
    }

    #[test]
    fn extract_media_type_contact() {
        let msg = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Contact),
        };
        assert_eq!(
            extract_media_type_from_view(&msg).as_deref(),
            Some("contact")
        );
    }

    #[test]
    fn extract_media_metadata_photo_and_document() {
        let photo = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Photo { photo_id: 99 }),
        };
        let meta = extract_media_metadata_from_view(&photo).unwrap();
        assert_eq!(meta["type"], "photo");
        assert_eq!(meta["photo_id"], 99);

        let doc = FakeMessage {
            text: String::new(),
            media: Some(MediaExtractView::Document {
                document_id: 7,
                mime_type: Some("application/pdf".into()),
                file_name: Some("notes.pdf".into()),
                size: Some(512),
            }),
        };
        let meta = extract_media_metadata_from_view(&doc).unwrap();
        assert_eq!(meta["type"], "document");
        assert_eq!(meta["document_id"], 7);
        assert_eq!(meta["mime_type"], "application/pdf");
        assert_eq!(meta["file_name"], "notes.pdf");
        assert_eq!(meta["size"], 512);
    }
}
