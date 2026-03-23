use void_core::models::MessageContent;

use super::send::{build_wa_message, parse_reply_id};
use super::*;
use wa_rs::download::MediaType as WaMediaType;
use wa_rs_proto::whatsapp::message::ExtendedTextMessage;
use wa_rs_proto::whatsapp::{ContextInfo, Message as WaMessage};

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
    let content = MessageContent::Text("test".into());
    let msg = build_wa_message(&content, None).unwrap();
    assert_eq!(msg.conversation, Some("test".into()));
    assert!(msg.extended_text_message.is_none());
}

#[test]
fn build_quoted_message() {
    let content = MessageContent::Text("reply text".into());
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
fn parse_reply_id_valid() {
    let (chat, msg) = parse_reply_id("33612345678@s.whatsapp.net:ABC123DEF").unwrap();
    assert_eq!(chat, "33612345678@s.whatsapp.net");
    assert_eq!(msg, "ABC123DEF");
}

#[test]
fn parse_reply_id_invalid() {
    assert!(parse_reply_id("no_colon_here").is_err());
}
