use super::types::{FileAttachment, GmailMessage, MessagePart, MessagePartBody, MessagePayload};

impl GmailMessage {
    pub fn get_header(&self, name: &str) -> Option<String> {
        self.payload
            .as_ref()?
            .headers
            .as_ref()?
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.clone())
    }

    /// Extract the plain text body by walking the MIME tree.
    pub fn text_body(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| extract_body_by_mime(p, "text/plain"))
    }

    /// Extract the HTML body by walking the MIME tree.
    pub fn html_body(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| extract_body_by_mime(p, "text/html"))
    }

    /// Return the attachment_id for the text/plain part when data is absent (large body).
    pub fn text_body_attachment_id(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| find_attachment_id_by_mime(p, "text/plain"))
    }

    /// Return the attachment_id for the text/html part when data is absent (large body).
    pub fn html_body_attachment_id(&self) -> Option<String> {
        self.payload
            .as_ref()
            .and_then(|p| find_attachment_id_by_mime(p, "text/html"))
    }

    /// Extract all file attachments (parts with a non-empty filename and an attachment_id).
    pub fn file_attachments(&self) -> Vec<FileAttachment> {
        let mut result = Vec::new();
        if let Some(payload) = &self.payload {
            if let Some(parts) = &payload.parts {
                for part in parts {
                    collect_file_attachments(part, &mut result);
                }
            }
        }
        result
    }
}

fn extract_body_by_mime(payload: &MessagePayload, target_mime: &str) -> Option<String> {
    if let Some(mime) = &payload.mime_type {
        if mime == target_mime {
            return decode_body_data(&payload.body);
        }
    }

    if let Some(parts) = &payload.parts {
        for part in parts {
            if let Some(result) = extract_body_from_part(part, target_mime) {
                return Some(result);
            }
        }
    }

    None
}

fn extract_body_from_part(part: &MessagePart, target_mime: &str) -> Option<String> {
    if let Some(mime) = &part.mime_type {
        if mime == target_mime {
            return decode_body_data(&part.body);
        }
    }

    if let Some(sub_parts) = &part.parts {
        for sub in sub_parts {
            if let Some(result) = extract_body_from_part(sub, target_mime) {
                return Some(result);
            }
        }
    }

    None
}

fn decode_body_data(body: &Option<MessagePartBody>) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let data = body.as_ref()?.data.as_deref()?;
    let bytes = URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).ok()?;
    String::from_utf8(bytes).ok()
}

pub fn decode_attachment_data(data: &str) -> Option<String> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;

    let bytes = URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).ok()?;
    String::from_utf8(bytes).ok()
}

fn collect_file_attachments(part: &MessagePart, out: &mut Vec<FileAttachment>) {
    if let Some(filename) = &part.filename {
        if !filename.is_empty() {
            if let Some(aid) = part.body.as_ref().and_then(|b| b.attachment_id.as_ref()) {
                out.push(FileAttachment {
                    filename: filename.clone(),
                    mime_type: part.mime_type.clone(),
                    size: part.body.as_ref().and_then(|b| b.size),
                    attachment_id: aid.clone(),
                });
            }
        }
    }
    if let Some(sub_parts) = &part.parts {
        for sub in sub_parts {
            collect_file_attachments(sub, out);
        }
    }
}

fn find_attachment_id_by_mime(payload: &MessagePayload, target_mime: &str) -> Option<String> {
    if let Some(mime) = &payload.mime_type {
        if mime == target_mime {
            if let Some(body) = &payload.body {
                if body.data.is_none() {
                    return body.attachment_id.clone();
                }
            }
        }
    }
    if let Some(parts) = &payload.parts {
        for part in parts {
            if let Some(id) = find_attachment_id_in_part(part, target_mime) {
                return Some(id);
            }
        }
    }
    None
}

fn find_attachment_id_in_part(part: &MessagePart, target_mime: &str) -> Option<String> {
    if let Some(mime) = &part.mime_type {
        if mime == target_mime {
            if let Some(body) = &part.body {
                if body.data.is_none() {
                    return body.attachment_id.clone();
                }
            }
        }
    }
    if let Some(sub_parts) = &part.parts {
        for sub in sub_parts {
            if let Some(id) = find_attachment_id_in_part(sub, target_mime) {
                return Some(id);
            }
        }
    }
    None
}
