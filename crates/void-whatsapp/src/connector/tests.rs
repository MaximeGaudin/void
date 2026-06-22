use void_core::connector::Connector;
use void_core::models::{ConnectorType, MessageContent};

use super::send::build_wa_message;
use super::*;
use crate::CONNECTOR_ID;
use wa_rs::download::MediaType as WaMediaType;
use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

#[tokio::test]
async fn health_check_missing_session_db_suggests_setup() {
    let session_path =
        std::env::temp_dir().join(format!("void-wa-missing-{}.db", uuid::Uuid::new_v4()));
    let connector = WhatsAppConnector::new("test-conn", &session_path.to_string_lossy());

    let status = connector.health_check().await.unwrap();

    assert!(!status.ok);
    assert!(status.message.contains("setup"));
    assert_eq!(status.connection_id, "test-conn");
    assert_eq!(
        status.connector_type,
        ConnectorType::from_static(CONNECTOR_ID)
    );
}

#[tokio::test]
async fn health_check_empty_session_db_file_is_ok() {
    let session_path =
        std::env::temp_dir().join(format!("void-wa-empty-{}.db", uuid::Uuid::new_v4()));
    std::fs::File::create(&session_path).unwrap();
    let connector = WhatsAppConnector::new("test-conn", &session_path.to_string_lossy());

    let status = connector.health_check().await.unwrap();

    assert!(status.ok);
    assert!(status.message.contains("session"));
    assert_eq!(status.connection_id, "test-conn");
    assert_eq!(
        status.connector_type,
        ConnectorType::from_static(CONNECTOR_ID)
    );

    let _ = std::fs::remove_file(&session_path);
}

#[test]
fn parse_jid_phone_number() {
    let jid = parse_jid("33612345678").unwrap();
    assert_eq!(jid.to_string(), "33612345678@s.whatsapp.net");
}

#[test]
fn parse_jid_full_dm() {
    let jid = parse_jid("33612345678@s.whatsapp.net").unwrap();
    assert_eq!(jid.to_string(), "33612345678@s.whatsapp.net");
}

#[test]
fn parse_jid_group() {
    let jid = parse_jid("120363123456789@g.us").unwrap();
    assert_eq!(jid.to_string(), "120363123456789@g.us");
}

#[test]
fn normalize_phone_strips_prefix() {
    assert_eq!(normalize_phone("+33 6 12 34 56 78"), "33612345678");
}

#[test]
fn determine_media_type_from_extension() {
    assert_eq!(
        media::determine_media_type(None, "photo.jpg").0,
        WaMediaType::Image
    );
    assert_eq!(
        media::determine_media_type(None, "clip.mp4").0,
        WaMediaType::Video
    );
    assert_eq!(
        media::determine_media_type(None, "voice.ogg").0,
        WaMediaType::Audio
    );
    assert_eq!(
        media::determine_media_type(None, "doc.pdf").0,
        WaMediaType::Document
    );
    assert_eq!(
        media::determine_media_type(None, "file.unknown").0,
        WaMediaType::Document
    );
}

#[test]
fn extract_text_conversation() {
    let msg = WaMessage {
        conversation: Some("hello".into()),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("hello".into()));
}

#[test]
fn extract_text_extended() {
    let msg = WaMessage {
        extended_text_message: Some(Box::new(ExtendedTextMessage {
            text: Some("extended hello".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("extended hello".into()));
}

#[test]
fn extract_text_ephemeral_wrapper() {
    use wa_rs_proto::whatsapp::message::FutureProofMessage;
    let msg = WaMessage {
        ephemeral_message: Some(Box::new(FutureProofMessage {
            message: Some(Box::new(WaMessage {
                conversation: Some("ephemeral text".into()),
                ..Default::default()
            })),
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("ephemeral text".into()));
}

#[test]
fn extract_text_device_sent_wrapper() {
    use wa_rs_proto::whatsapp::message::DeviceSentMessage;
    let msg = WaMessage {
        device_sent_message: Some(Box::new(DeviceSentMessage {
            message: Some(Box::new(WaMessage {
                extended_text_message: Some(Box::new(ExtendedTextMessage {
                    text: Some("from other device".into()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        extract::extract_text(&msg),
        Some("from other device".into())
    );
}

#[test]
fn extract_text_view_once_wrapper() {
    use wa_rs_proto::whatsapp::message::{FutureProofMessage, ImageMessage};
    let msg = WaMessage {
        view_once_message: Some(Box::new(FutureProofMessage {
            message: Some(Box::new(WaMessage {
                image_message: Some(Box::new(ImageMessage {
                    caption: Some("view once caption".into()),
                    ..Default::default()
                })),
                ..Default::default()
            })),
        })),
        ..Default::default()
    };
    assert_eq!(
        extract::extract_text(&msg),
        Some("view once caption".into())
    );
}

#[test]
fn extract_text_edited_message_wrapper() {
    use wa_rs_proto::whatsapp::message::FutureProofMessage;
    let msg = WaMessage {
        edited_message: Some(Box::new(FutureProofMessage {
            message: Some(Box::new(WaMessage {
                conversation: Some("edited text".into()),
                ..Default::default()
            })),
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("edited text".into()));
}

#[test]
fn extract_text_protocol_message_returns_none() {
    use wa_rs_proto::whatsapp::message::ProtocolMessage;
    let msg = WaMessage {
        protocol_message: Some(Box::new(ProtocolMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), None);
}

#[test]
fn extract_text_image_caption() {
    use wa_rs_proto::whatsapp::message::ImageMessage;
    let msg = WaMessage {
        image_message: Some(Box::new(ImageMessage {
            caption: Some("photo caption".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("photo caption".into()));
}

#[test]
fn extract_text_sticker_fallback() {
    use wa_rs_proto::whatsapp::message::StickerMessage;
    let msg = WaMessage {
        sticker_message: Some(Box::new(StickerMessage::default())),
        ..Default::default()
    };
    assert_eq!(
        extract::extract_text(&msg),
        Some("\u{1f5bc}\u{fe0f} Sticker".into())
    );
}

#[test]
fn extract_text_audio_fallback() {
    use wa_rs_proto::whatsapp::message::AudioMessage;
    let msg = WaMessage {
        audio_message: Some(Box::new(AudioMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("\u{1f3b5} Audio".into()));
}

#[test]
fn extract_media_type_image() {
    use wa_rs_proto::whatsapp::message::ImageMessage;
    let msg = WaMessage {
        image_message: Some(Box::new(ImageMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_media_type(&msg), Some("image".into()));
}

#[test]
fn extract_media_type_through_ephemeral() {
    use wa_rs_proto::whatsapp::message::{FutureProofMessage, VideoMessage};
    let msg = WaMessage {
        ephemeral_message: Some(Box::new(FutureProofMessage {
            message: Some(Box::new(WaMessage {
                video_message: Some(Box::new(VideoMessage::default())),
                ..Default::default()
            })),
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_media_type(&msg), Some("video".into()));
}

#[test]
fn extract_media_type_none_for_text() {
    let msg = WaMessage {
        conversation: Some("just text".into()),
        ..Default::default()
    };
    assert_eq!(extract::extract_media_type(&msg), None);
}

#[test]
fn build_text_message_simple() {
    let content = MessageContent::from_text("test");
    let msg = build_wa_message(&content, None).unwrap();
    assert_eq!(msg.conversation, Some("test".into()));
    assert!(msg.extended_text_message.is_none());
}

#[test]
fn build_quoted_message() {
    let content = MessageContent::from_text("reply text");
    let ctx = ContextInfo {
        stanza_id: Some("orig_msg_123".into()),
        ..Default::default()
    };
    let msg = build_wa_message(&content, Some(ctx)).unwrap();
    assert!(msg.conversation.is_none());
    let ext = msg.extended_text_message.as_ref().unwrap();
    assert_eq!(ext.text, Some("reply text".into()));
    assert_eq!(
        ext.context_info.as_ref().unwrap().stanza_id,
        Some("orig_msg_123".into())
    );
}

#[test]
fn extract_text_reaction_returns_none() {
    use wa_rs_proto::whatsapp::message::ReactionMessage;
    let msg = WaMessage {
        reaction_message: Some(ReactionMessage {
            text: Some("❤️".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    // Reactions are handled via metadata on the target message, not as separate messages
    assert_eq!(extract::extract_text(&msg), None);
    assert_eq!(extract::extract_media_type(&msg), None);
}

#[test]
fn extract_text_poll() {
    use wa_rs_proto::whatsapp::message::PollCreationMessage;
    let msg = WaMessage {
        poll_creation_message: Some(Box::new(PollCreationMessage {
            name: Some("Favorite color?".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        extract::extract_text(&msg),
        Some("📊 Favorite color?".into())
    );
    assert_eq!(extract::extract_media_type(&msg), Some("poll".into()));
}

#[test]
fn extract_text_group_invite() {
    use wa_rs_proto::whatsapp::message::GroupInviteMessage;
    let msg = WaMessage {
        group_invite_message: Some(Box::new(GroupInviteMessage {
            group_name: Some("My Group".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        extract::extract_text(&msg),
        Some("👥 Group invite: My Group".into())
    );
    assert_eq!(extract::extract_media_type(&msg), Some("invite".into()));
}

#[test]
fn extract_text_call() {
    use wa_rs_proto::whatsapp::message::Call;
    let msg = WaMessage {
        call: Some(Box::new(Call::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📞 Call".into()));
    assert_eq!(extract::extract_media_type(&msg), Some("call".into()));
}

#[test]
fn extract_text_video_note() {
    use wa_rs_proto::whatsapp::message::VideoMessage;
    let msg = WaMessage {
        ptv_message: Some(Box::new(VideoMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("🎥 Video note".into()));
    assert_eq!(extract::extract_media_type(&msg), Some("video".into()));
}

#[test]
fn extract_text_event() {
    use wa_rs_proto::whatsapp::message::EventMessage;
    let msg = WaMessage {
        event_message: Some(Box::new(EventMessage {
            name: Some("Team meeting".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📅 Team meeting".into()));
    assert_eq!(extract::extract_media_type(&msg), Some("event".into()));
}

#[test]
fn is_system_message_sender_key_distribution() {
    use wa_rs_proto::whatsapp::message::SenderKeyDistributionMessage;
    let msg = WaMessage {
        sender_key_distribution_message: Some(SenderKeyDistributionMessage::default()),
        ..Default::default()
    };
    assert!(sync::is_system_message(&msg));
}

#[test]
fn is_system_message_protocol() {
    use wa_rs_proto::whatsapp::message::ProtocolMessage;
    let msg = WaMessage {
        protocol_message: Some(Box::new(ProtocolMessage::default())),
        ..Default::default()
    };
    assert!(sync::is_system_message(&msg));
}

#[test]
fn is_system_message_false_for_text() {
    let msg = WaMessage {
        conversation: Some("hello".into()),
        ..Default::default()
    };
    assert!(!sync::is_system_message(&msg));
}

#[test]
fn extract_text_image_no_caption_fallback() {
    use wa_rs_proto::whatsapp::message::ImageMessage;
    let msg = WaMessage {
        image_message: Some(Box::new(ImageMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📷 Image".into()));
}

#[test]
fn extract_text_document_fallback() {
    use wa_rs_proto::whatsapp::message::DocumentMessage;
    let msg = WaMessage {
        document_message: Some(Box::new(DocumentMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📄 Document".into()));
}

#[test]
fn extract_text_document_with_filename() {
    use wa_rs_proto::whatsapp::message::DocumentMessage;
    let msg = WaMessage {
        document_message: Some(Box::new(DocumentMessage {
            file_name: Some("report.pdf".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📄 report.pdf".into()));
}

#[test]
fn extract_media_metadata_document() {
    use wa_rs_proto::whatsapp::message::DocumentMessage;
    let msg = WaMessage {
        document_message: Some(Box::new(DocumentMessage {
            file_name: Some("report.pdf".into()),
            mimetype: Some("application/pdf".into()),
            file_length: Some(102400),
            page_count: Some(5),
            ..Default::default()
        })),
        ..Default::default()
    };
    let meta = extract::extract_media_metadata(&msg).unwrap();
    assert_eq!(meta["file_name"], "report.pdf");
    assert_eq!(meta["mimetype"], "application/pdf");
    assert_eq!(meta["file_size"], 102400);
    assert_eq!(meta["page_count"], 5);
}

#[test]
fn extract_media_metadata_image() {
    use wa_rs_proto::whatsapp::message::ImageMessage;
    let msg = WaMessage {
        image_message: Some(Box::new(ImageMessage {
            mimetype: Some("image/jpeg".into()),
            file_length: Some(50000),
            width: Some(1920),
            height: Some(1080),
            ..Default::default()
        })),
        ..Default::default()
    };
    let meta = extract::extract_media_metadata(&msg).unwrap();
    assert_eq!(meta["mimetype"], "image/jpeg");
    assert_eq!(meta["file_size"], 50000);
    assert_eq!(meta["width"], 1920);
    assert_eq!(meta["height"], 1080);
}

#[test]
fn extract_media_metadata_none_for_text() {
    let msg = WaMessage {
        conversation: Some("just text".into()),
        ..Default::default()
    };
    assert!(extract::extract_media_metadata(&msg).is_none());
}

#[test]
fn parse_jid_preserves_group_server() {
    let jid = parse_jid("120363@g.us").unwrap();
    assert_eq!(jid.to_string(), "120363@g.us");
}

#[test]
fn parse_jid_custom_server_passthrough() {
    let jid = parse_jid("12345@lid").unwrap();
    assert_eq!(jid.to_string(), "12345@lid");
}

#[test]
fn parse_jid_bare_number_defaults_to_whatsapp_net() {
    let jid = parse_jid("123").unwrap();
    assert_eq!(jid.to_string(), "123@s.whatsapp.net");
}

#[test]
fn parse_jid_with_at_does_not_append_default_server() {
    // Anything containing '@' is split rather than treated as a bare number.
    let jid = parse_jid("user@example.server").unwrap();
    assert_eq!(jid.to_string(), "user@example.server");
}

#[test]
fn normalize_phone_already_clean() {
    assert_eq!(normalize_phone("33612345678"), "33612345678");
}

#[test]
fn normalize_phone_strips_non_digits() {
    assert_eq!(normalize_phone("+1 (650) 555-0100"), "16505550100");
}

#[test]
fn normalize_phone_drops_letters() {
    assert_eq!(normalize_phone("33-ABC-612"), "33612");
}

#[test]
fn normalize_phone_empty() {
    assert_eq!(normalize_phone(""), "");
}

#[test]
fn determine_media_type_mime_priority_over_extension() {
    // MIME wins even when the extension suggests a different type.
    assert_eq!(
        media::determine_media_type(Some("image/png"), "file.pdf").0,
        WaMediaType::Image
    );
    assert_eq!(
        media::determine_media_type(Some("video/mp4"), "file.jpg").0,
        WaMediaType::Video
    );
    assert_eq!(
        media::determine_media_type(Some("audio/ogg"), "file.txt").0,
        WaMediaType::Audio
    );
}

#[test]
fn determine_media_type_mime_case_insensitive() {
    assert_eq!(
        media::determine_media_type(Some("IMAGE/JPEG"), "x").0,
        WaMediaType::Image
    );
}

#[test]
fn determine_media_type_unknown_mime_falls_back_to_extension() {
    // An unrecognized MIME type falls through to the extension check.
    assert_eq!(
        media::determine_media_type(Some("application/x-weird"), "clip.mov").0,
        WaMediaType::Video
    );
}

#[test]
fn determine_media_type_default_mime_strings() {
    assert_eq!(media::determine_media_type(None, "a.png").1, "image/jpeg");
    assert_eq!(media::determine_media_type(None, "a.mp4").1, "video/mp4");
    assert_eq!(
        media::determine_media_type(None, "a.ogg").1,
        "audio/ogg; codecs=opus"
    );
    assert_eq!(
        media::determine_media_type(None, "a.bin").1,
        "application/octet-stream"
    );
}

#[test]
fn determine_media_type_uppercase_extension() {
    // Extension is lowercased before matching.
    assert_eq!(
        media::determine_media_type(None, "PHOTO.JPG").0,
        WaMediaType::Image
    );
}

#[test]
fn extract_quoted_id_present() {
    let msg = WaMessage {
        extended_text_message: Some(Box::new(ExtendedTextMessage {
            text: Some("reply".into()),
            context_info: Some(Box::new(ContextInfo {
                stanza_id: Some("ORIGINAL_ID".into()),
                ..Default::default()
            })),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_quoted_id(&msg), Some("ORIGINAL_ID".into()));
}

#[test]
fn extract_quoted_id_absent_for_plain_text() {
    let msg = WaMessage {
        conversation: Some("hi".into()),
        ..Default::default()
    };
    assert_eq!(extract::extract_quoted_id(&msg), None);
}

#[test]
fn extract_quoted_id_none_when_no_context_info() {
    let msg = WaMessage {
        extended_text_message: Some(Box::new(ExtendedTextMessage {
            text: Some("no quote".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_quoted_id(&msg), None);
}

#[test]
fn extract_text_location_with_name() {
    use wa_rs_proto::whatsapp::message::LocationMessage;
    let msg = WaMessage {
        location_message: Some(Box::new(LocationMessage {
            name: Some("Eiffel Tower".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("Eiffel Tower".into()));
    assert_eq!(extract::extract_media_type(&msg), Some("location".into()));
}

#[test]
fn extract_text_location_without_name_fallback() {
    use wa_rs_proto::whatsapp::message::LocationMessage;
    let msg = WaMessage {
        location_message: Some(Box::new(LocationMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("📍 Location".into()));
}

#[test]
fn extract_text_contact() {
    use wa_rs_proto::whatsapp::message::ContactMessage;
    let msg = WaMessage {
        contact_message: Some(Box::new(ContactMessage {
            display_name: Some("Jane Doe".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("👤 Jane Doe".into()));
    assert_eq!(extract::extract_media_type(&msg), Some("contact".into()));
}

#[test]
fn extract_text_contact_without_name_fallback() {
    use wa_rs_proto::whatsapp::message::ContactMessage;
    let msg = WaMessage {
        contact_message: Some(Box::new(ContactMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_text(&msg), Some("👤 Contact".into()));
}

#[test]
fn extract_media_type_sticker() {
    use wa_rs_proto::whatsapp::message::StickerMessage;
    let msg = WaMessage {
        sticker_message: Some(Box::new(StickerMessage::default())),
        ..Default::default()
    };
    assert_eq!(extract::extract_media_type(&msg), Some("sticker".into()));
}

#[test]
fn extract_media_metadata_video() {
    use wa_rs_proto::whatsapp::message::VideoMessage;
    let msg = WaMessage {
        video_message: Some(Box::new(VideoMessage {
            mimetype: Some("video/mp4".into()),
            file_length: Some(2_000_000),
            seconds: Some(30),
            width: Some(1280),
            height: Some(720),
            ..Default::default()
        })),
        ..Default::default()
    };
    let meta = extract::extract_media_metadata(&msg).unwrap();
    assert_eq!(meta["mimetype"], "video/mp4");
    assert_eq!(meta["file_size"], 2_000_000);
    assert_eq!(meta["duration_secs"], 30);
    assert_eq!(meta["width"], 1280);
    assert_eq!(meta["height"], 720);
    assert_eq!(meta["media_type"], "video");
}

#[test]
fn extract_media_metadata_audio_voice_note() {
    use wa_rs_proto::whatsapp::message::AudioMessage;
    let msg = WaMessage {
        audio_message: Some(Box::new(AudioMessage {
            mimetype: Some("audio/ogg; codecs=opus".into()),
            file_length: Some(12345),
            seconds: Some(7),
            ptt: Some(true),
            ..Default::default()
        })),
        ..Default::default()
    };
    let meta = extract::extract_media_metadata(&msg).unwrap();
    assert_eq!(meta["mimetype"], "audio/ogg; codecs=opus");
    assert_eq!(meta["file_size"], 12345);
    assert_eq!(meta["duration_secs"], 7);
    assert_eq!(meta["voice_note"], true);
    assert_eq!(meta["media_type"], "audio");
}

#[test]
fn extract_media_metadata_encodes_download_fields_base64() {
    use wa_rs_proto::whatsapp::message::ImageMessage;
    let msg = WaMessage {
        image_message: Some(Box::new(ImageMessage {
            mimetype: Some("image/jpeg".into()),
            media_key: Some(vec![1, 2, 3]),
            file_sha256: Some(vec![4, 5, 6]),
            file_enc_sha256: Some(vec![7, 8, 9]),
            ..Default::default()
        })),
        ..Default::default()
    };
    let meta = extract::extract_media_metadata(&msg).unwrap();
    // base64(STANDARD) of [1,2,3] = "AQID", [4,5,6] = "BAUG", [7,8,9] = "BwgJ"
    assert_eq!(meta["media_key"], "AQID");
    assert_eq!(meta["file_sha256"], "BAUG");
    assert_eq!(meta["file_enc_sha256"], "BwgJ");
}

#[test]
fn build_wa_message_file_content_is_error() {
    let content = MessageContent::File {
        path: std::env::temp_dir().join("z.png"),
        caption: Some("cap".into()),
        mime_type: Some("image/png".into()),
        subject: None,
    };
    assert!(build_wa_message(&content, None).is_err());
}

#[test]
fn build_wa_message_text_with_context_uses_extended() {
    let content = MessageContent::from_text("hello");
    let ctx = ContextInfo {
        stanza_id: Some("q1".into()),
        ..Default::default()
    };
    let msg = build_wa_message(&content, Some(ctx)).unwrap();
    assert!(msg.conversation.is_none());
    let ext = msg.extended_text_message.as_ref().unwrap();
    assert_eq!(ext.text, Some("hello".into()));
    assert_eq!(
        ext.context_info.as_ref().unwrap().stanza_id,
        Some("q1".into())
    );
}

#[test]
fn is_system_message_keep_in_chat() {
    use wa_rs_proto::whatsapp::message::KeepInChatMessage;
    let msg = WaMessage {
        keep_in_chat_message: Some(KeepInChatMessage::default()),
        ..Default::default()
    };
    assert!(sync::is_system_message(&msg));
}

#[test]
fn is_system_message_pin_in_chat() {
    use wa_rs_proto::whatsapp::message::PinInChatMessage;
    let msg = WaMessage {
        pin_in_chat_message: Some(PinInChatMessage::default()),
        ..Default::default()
    };
    assert!(sync::is_system_message(&msg));
}
