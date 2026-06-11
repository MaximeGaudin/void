use anyhow::Context;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// RFC 2047 encode a header value if it contains non-ASCII characters.
pub fn encode_rfc2047(value: &str) -> String {
    if value.is_ascii() {
        return value.to_string();
    }
    let encoded = STANDARD.encode(value.as_bytes());
    format!("=?UTF-8?B?{encoded}?=")
}

pub fn compose_rfc2822(
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> String {
    compose_rfc2822_ex(to, subject, body, in_reply_to, references, None)
}

/// Like [`compose_rfc2822`], but `body_is_html` forces HTML handling when the body does not
/// start with HTML tags (e.g. a forward wrapper followed by quoted HTML).
pub fn compose_rfc2822_ex(
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
    body_is_html: Option<bool>,
) -> String {
    let subject = encode_rfc2047(subject);

    let is_html = body_is_html.unwrap_or_else(|| looks_like_html(body));
    let final_body = if is_html {
        body.to_string()
    } else {
        body.replace('\n', "<br>\n")
    };
    let content_type = "text/html";

    let mut headers = format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n"
    );
    if let Some(irt) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: {irt}\r\n"));
    }
    if let Some(refs) = references {
        headers.push_str(&format!("References: {refs}\r\n"));
    }
    let body_encoded = STANDARD.encode(final_body.as_bytes());
    let body_wrapped = body_encoded
        .as_bytes()
        .chunks(76)
        .map(|c| unsafe { std::str::from_utf8_unchecked(c) })
        .collect::<Vec<_>>()
        .join("\r\n");

    headers.push_str(&format!("\r\n{body_wrapped}"));
    headers
}

pub fn compose_rfc2822_with_attachment(
    to: &str,
    subject: &str,
    body: &str,
    file_path: &std::path::Path,
    mime_type: Option<&str>,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> anyhow::Result<String> {
    let file_bytes = std::fs::read(file_path)
        .with_context(|| format!("failed to read file {}", file_path.display()))?;
    let encoded = STANDARD.encode(&file_bytes);
    let wrapped = encoded
        .as_bytes()
        .chunks(76)
        .map(|c| {
            // SAFETY: STANDARD base64 alphabet is ASCII; each chunk is valid UTF-8.
            unsafe { std::str::from_utf8_unchecked(c) }
        })
        .collect::<Vec<_>>()
        .join("\r\n");

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment");
    let mime = mime_type.unwrap_or("application/octet-stream");

    const BOUNDARY: &str = "void_boundary_001";

    let subject = encode_rfc2047(subject);
    let mut headers = format!(
        "To: {to}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\r\n"
    );
    if let Some(irt) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: {irt}\r\n"));
    }
    if let Some(refs) = references {
        headers.push_str(&format!("References: {refs}\r\n"));
    }
    headers.push_str("\r\n");

    let (content_type, final_body) = if looks_like_html(body) {
        ("text/html", body.to_string())
    } else {
        ("text/html", body.replace('\n', "<br>\n"))
    };

    let body_encoded = STANDARD.encode(final_body.as_bytes());
    let body_wrapped = body_encoded
        .as_bytes()
        .chunks(76)
        .map(|c| unsafe { std::str::from_utf8_unchecked(c) })
        .collect::<Vec<_>>()
        .join("\r\n");

    let raw = format!(
        "{headers}--{BOUNDARY}\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{body_wrapped}\r\n--{BOUNDARY}\r\nContent-Type: {mime}; name=\"{filename}\"\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{wrapped}\r\n--{BOUNDARY}--"
    );
    Ok(raw)
}

pub fn parse_email_address(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[start + 1..].trim_end_matches('>').trim().to_string()
    } else {
        from.trim().to_string()
    }
}

pub fn parse_email_name(from: &str) -> String {
    if let Some(start) = from.find('<') {
        from[..start].trim().trim_matches('"').to_string()
    } else {
        from.to_string()
    }
}

fn escape_html_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn plain_text_to_html(text: &str) -> String {
    escape_html_text(text)
        .replace("\r\n", "<br>\n")
        .replace('\n', "<br>\n")
}

/// Build a forwarded-message body. Returns `(body, is_html)` for [`compose_rfc2822_ex`].
pub fn build_forward_body(
    comment: Option<&str>,
    orig_from: &str,
    orig_date: &str,
    orig_subject: &str,
    orig_to: &str,
    html_body: Option<&str>,
    text_body: Option<&str>,
) -> (String, bool) {
    if let Some(html) = html_body.filter(|s| !s.is_empty()) {
        let mut body = String::new();
        if let Some(c) = comment {
            if looks_like_html(c) {
                body.push_str(c);
            } else {
                body.push_str(&format!("<div dir=\"ltr\">{}</div>", plain_text_to_html(c)));
            }
            body.push_str("<br><br>");
        }
        body.push_str("<div class=\"gmail_quote\">");
        body.push_str("<div dir=\"ltr\" class=\"gmail_attr\">");
        body.push_str("---------- Forwarded message ---------<br>");
        body.push_str(&format!("From: {}<br>", escape_html_text(orig_from)));
        body.push_str(&format!("Date: {}<br>", escape_html_text(orig_date)));
        body.push_str(&format!("Subject: {}<br>", escape_html_text(orig_subject)));
        body.push_str(&format!("To: {}<br>", escape_html_text(orig_to)));
        body.push_str("</div><br>");
        body.push_str(html);
        body.push_str("</div>");
        (body, true)
    } else {
        let mut body = String::new();
        if let Some(c) = comment {
            body.push_str(c);
            body.push_str("\r\n\r\n");
        }
        body.push_str("---------- Forwarded message ---------\r\n");
        body.push_str(&format!("From: {orig_from}\r\n"));
        body.push_str(&format!("Date: {orig_date}\r\n"));
        body.push_str(&format!("Subject: {orig_subject}\r\n"));
        body.push_str(&format!("To: {orig_to}\r\n"));
        body.push_str("\r\n");
        body.push_str(text_body.unwrap_or(""));
        (body, false)
    }
}

pub fn looks_like_html(text: &str) -> bool {
    let trimmed = text.trim_start();
    trimmed.starts_with("<!DOCTYPE")
        || trimmed.starts_with("<!doctype")
        || trimmed.starts_with("<html")
        || trimmed.starts_with("<HTML")
        || (trimmed.contains("<div") && trimmed.contains("</div>"))
        || (trimmed.contains("<table") && trimmed.contains("</table>"))
        || (trimmed.contains("<body") && trimmed.contains("</body>"))
}

pub fn html_to_markdown(html: &str) -> String {
    // html-to-markdown-rs 3.x returns a ConversionResult whose `content` holds
    // the rendered text (None only in extraction-only mode, which we don't use).
    html_to_markdown_rs::convert(html, None)
        .ok()
        .and_then(|result| result.content)
        .unwrap_or_else(|| html.to_string())
}
