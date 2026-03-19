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

pub fn compose_rfc2822(to: &str, subject: &str, body: &str) -> String {
    let subject = encode_rfc2047(subject);
    format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
    )
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
        .map(|c| std::str::from_utf8(c).unwrap())
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

    let raw = format!(
        "{headers}--{BOUNDARY}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n--{BOUNDARY}\r\nContent-Type: {mime}; name=\"{filename}\"\r\nContent-Disposition: attachment; filename=\"{filename}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n{wrapped}\r\n--{BOUNDARY}--"
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
    html_to_markdown_rs::convert(html, None).unwrap_or_else(|_| html.to_string())
}
