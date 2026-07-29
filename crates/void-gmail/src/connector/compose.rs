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

/// Addressing headers for an outgoing RFC 2822 message.
///
/// `cc` / `bcc` are optional comma-separated address lists. They are emitted
/// immediately after `To` and before `Subject` / MIME headers so Gmail honors them.
#[derive(Debug, Clone, Copy, Default)]
pub struct ComposeRecipients<'a> {
    pub to: &'a str,
    pub cc: Option<&'a str>,
    pub bcc: Option<&'a str>,
}

impl<'a> ComposeRecipients<'a> {
    pub fn to_only(to: &'a str) -> Self {
        Self {
            to,
            cc: None,
            bcc: None,
        }
    }
}

/// Addressing for draft create: `to` may be omitted when `--reply-to` derives recipients.
#[derive(Debug, Clone, Copy, Default)]
pub struct DraftRecipients<'a> {
    pub to: Option<&'a str>,
    pub cc: Option<&'a str>,
    pub bcc: Option<&'a str>,
}

/// Reject CR/LF and other ASCII controls so address fields cannot inject headers.
fn reject_header_injection(field: &str, value: &str) -> anyhow::Result<()> {
    if value.bytes().any(|b| b.is_ascii_control()) {
        anyhow::bail!(
            "invalid {field}: address fields must not contain control characters (e.g. CR/LF)"
        );
    }
    Ok(())
}

fn push_address_headers(
    headers: &mut String,
    recipients: ComposeRecipients<'_>,
) -> anyhow::Result<()> {
    reject_header_injection("To", recipients.to)?;
    headers.push_str(&format!("To: {}\r\n", recipients.to));
    if let Some(cc) = recipients.cc.map(str::trim).filter(|s| !s.is_empty()) {
        reject_header_injection("Cc", cc)?;
        headers.push_str(&format!("Cc: {cc}\r\n"));
    }
    if let Some(bcc) = recipients.bcc.map(str::trim).filter(|s| !s.is_empty()) {
        reject_header_injection("Bcc", bcc)?;
        headers.push_str(&format!("Bcc: {bcc}\r\n"));
    }
    Ok(())
}

pub fn compose_rfc2822(
    to: &str,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
) -> anyhow::Result<String> {
    compose_rfc2822_ex(
        ComposeRecipients::to_only(to),
        subject,
        body,
        in_reply_to,
        references,
        None,
    )
}

/// Like [`compose_rfc2822`], but accepts Cc/Bcc and optional forced HTML handling.
pub fn compose_rfc2822_ex(
    recipients: ComposeRecipients<'_>,
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
    references: Option<&str>,
    body_is_html: Option<bool>,
) -> anyhow::Result<String> {
    let subject = encode_rfc2047(subject);

    let is_html = body_is_html.unwrap_or_else(|| looks_like_html_for_compose(body));
    let final_body = if is_html {
        body.to_string()
    } else {
        body.replace('\n', "<br>\n")
    };
    let content_type = "text/html";

    let mut headers = String::new();
    push_address_headers(&mut headers, recipients)?;
    headers.push_str(&format!(
        "Subject: {subject}\r\nContent-Type: {content_type}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n"
    ));
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
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n");

    headers.push_str(&format!("\r\n{body_wrapped}"));
    Ok(headers)
}

pub fn compose_rfc2822_with_attachment(
    recipients: ComposeRecipients<'_>,
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
        // STANDARD base64 alphabet is ASCII, so each chunk is valid UTF-8.
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
        .collect::<Vec<_>>()
        .join("\r\n");

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment");
    let mime = mime_type.unwrap_or("application/octet-stream");

    const BOUNDARY: &str = "void_boundary_001";

    let subject = encode_rfc2047(subject);
    let mut headers = String::new();
    push_address_headers(&mut headers, recipients)?;
    headers.push_str(&format!(
        "Subject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\r\n"
    ));
    if let Some(irt) = in_reply_to {
        headers.push_str(&format!("In-Reply-To: {irt}\r\n"));
    }
    if let Some(refs) = references {
        headers.push_str(&format!("References: {refs}\r\n"));
    }
    headers.push_str("\r\n");

    let (content_type, final_body) = if looks_like_html_for_compose(body) {
        ("text/html", body.to_string())
    } else {
        ("text/html", body.replace('\n', "<br>\n"))
    };

    let body_encoded = STANDARD.encode(final_body.as_bytes());
    let body_wrapped = body_encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).expect("base64 output is ASCII"))
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
            if looks_like_html_for_compose(c) {
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

/// Heuristic for whether a synced message body should be treated as HTML.
///
/// Used on the sync/display path (`html_to_markdown` vs store verbatim). Keeps a
/// stricter check than compose so bare `<br>` / `<a>` in plain-text MIME parts
/// are not re-parsed as HTML.
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

/// Heuristic for whether an outgoing body should be treated as HTML when composing.
///
/// Extends [`looks_like_html`] with bare `<br>` / `<a>` detection so draft bodies
/// and signature append skip newline→`<br>` conversion on already-HTML fragments.
pub fn looks_like_html_for_compose(text: &str) -> bool {
    if looks_like_html(text) {
        return true;
    }
    let trimmed = text.trim_start();
    // Draft bodies often use <br> / anchors without a wrapping <div>/<html>.
    trimmed.contains("<br")
        || trimmed.contains("<BR")
        || (trimmed.contains("<a ") && trimmed.contains("</a>"))
}

/// Whether to append a Gmail HTML signature when composing an outgoing message
/// (draft create/update, send, reply, or forward).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComposeSignature<'a> {
    /// Do not append a signature.
    #[default]
    None,
    /// Append the account default/primary send-as signature.
    Default,
    /// Append the signature for a specific send-as alias.
    From(&'a str),
}

impl<'a> ComposeSignature<'a> {
    /// Build from CLI `--signature` / `--signature-from` flags.
    pub fn from_flags(enabled: bool, from: Option<&'a str>) -> Self {
        if !enabled {
            Self::None
        } else if let Some(email) = from {
            Self::From(email)
        } else {
            Self::Default
        }
    }

    /// Send-as email for [`GmailApiClient::resolve_signature`](crate::api::GmailApiClient::resolve_signature),
    /// or `None` for the account default/primary. Only meaningful when this is not [`Self::None`].
    pub fn send_as_email(self) -> Option<&'a str> {
        match self {
            Self::None | Self::Default => None,
            Self::From(email) => Some(email),
        }
    }
}

/// Append a Gmail HTML signature to a message body.
///
/// Plain-text bodies are converted to HTML first so the signature renders correctly
/// (Gmail API compose paths do not auto-inject account signatures).
///
/// Not idempotent: if `body` already ends with a `gmail_signature` block, calling
/// again duplicates it. Callers should pass the message body without a signature.
pub fn append_gmail_signature(body: &str, signature_html: &str) -> String {
    let signature_html = signature_html.trim();
    if signature_html.is_empty() {
        return body.to_string();
    }

    let body_html = if looks_like_html_for_compose(body) {
        body.to_string()
    } else {
        body.replace('\n', "<br>\n")
    };

    format!(
        "{body_html}<br><br><div class=\"gmail_signature\" data-smartmail=\"gmail_signature\">{signature_html}</div>"
    )
}

/// Insert a Gmail signature into a forward body.
///
/// When the body already contains a `gmail_quote` block, the signature is placed
/// between the optional comment and the quote (matching Gmail UI). Otherwise the
/// signature is appended at the end. Plain-text forwards are converted to HTML.
pub fn apply_signature_to_forward(
    body: &str,
    is_html: bool,
    signature_html: &str,
) -> (String, bool) {
    let signature_html = signature_html.trim();
    if signature_html.is_empty() {
        return (body.to_string(), is_html);
    }

    let marker = "<div class=\"gmail_quote\">";
    if is_html {
        if let Some(idx) = body.find(marker) {
            let sig_block = format!(
                "<br><br><div class=\"gmail_signature\" data-smartmail=\"gmail_signature\">{signature_html}</div><br>"
            );
            let mut out = String::with_capacity(body.len() + sig_block.len());
            out.push_str(&body[..idx]);
            out.push_str(&sig_block);
            out.push_str(&body[idx..]);
            return (out, true);
        }
    }

    (append_gmail_signature(body, signature_html), true)
}

pub fn html_to_markdown(html: &str) -> String {
    // html-to-markdown-rs 3.x returns a ConversionResult whose `content` holds
    // the rendered text (None only in extraction-only mode, which we don't use).
    html_to_markdown_rs::convert(html, None)
        .ok()
        .and_then(|result| result.content)
        .unwrap_or_else(|| html.to_string())
}
