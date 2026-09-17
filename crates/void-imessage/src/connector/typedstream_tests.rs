//! Tests for the `typedstream` decoder.
//!
//! Fixtures in `fixtures/` were generated on macOS by `NSArchiver`, the same
//! encoder Messages uses to write `message.attributedBody`. They contain
//! synthetic strings only, never anyone's real messages. Regenerate with:
//!
//! ```swift
//! let s = NSMutableAttributedString(string: "Hello from a synthetic fixture")
//! let data = NSArchiver.archivedData(withRootObject: s)
//! ```

use super::typedstream::decode_text;

const ASCII: &[u8] = include_bytes!("fixtures/ts_ascii.bin");
const UNICODE: &[u8] = include_bytes!("fixtures/ts_unicode.bin");
const EMPTY: &[u8] = include_bytes!("fixtures/ts_empty.bin");
const LONG: &[u8] = include_bytes!("fixtures/ts_long.bin");
const NEWLINES: &[u8] = include_bytes!("fixtures/ts_newlines.bin");

#[test]
fn decodes_plain_ascii_body() {
    assert_eq!(
        decode_text(ASCII).unwrap(),
        Some("Hello from a synthetic fixture".to_string())
    );
}

#[test]
fn decodes_multibyte_unicode_body() {
    // Accented Latin, an em dash, an emoji outside the BMP, and CJK. If the
    // decoder walks bytes instead of UTF-8, this is where it breaks.
    assert_eq!(
        decode_text(UNICODE).unwrap(),
        Some("café — naïve 😀 你好".to_string())
    );
}

#[test]
fn decodes_empty_string_body_as_empty_not_none() {
    // An empty attributed string is a real message (an attachment-only one),
    // not a decode failure. The distinction matters: None means "could not
    // read", Some("") means "read it, there was no text".
    assert_eq!(decode_text(EMPTY).unwrap(), Some(String::new()));
}

#[test]
fn decodes_body_longer_than_one_byte_length_prefix() {
    // typedstream encodes lengths under 128 in a single byte and switches to
    // a wider form above that. 600 chars forces the wide path.
    let decoded = decode_text(LONG).unwrap().unwrap();
    assert_eq!(decoded.len(), 600);
    assert!(decoded.chars().all(|c| c == 'A'));
}

#[test]
fn preserves_newlines_and_tabs() {
    assert_eq!(
        decode_text(NEWLINES).unwrap(),
        Some("line one\nline two\ttabbed".to_string())
    );
}

#[test]
fn rejects_blob_without_streamtyped_magic() {
    let err = decode_text(b"this is not a typedstream archive").unwrap_err();
    assert!(
        err.to_string().contains("streamtyped"),
        "error should name the missing magic, got: {err}"
    );
}

#[test]
fn rejects_truncated_blob_instead_of_panicking() {
    // A truncated blob must produce an error, never an out-of-bounds panic:
    // this runs inside the sync loop over a live database.
    let truncated = &ASCII[..ASCII.len() / 2];
    assert!(decode_text(truncated).is_err());
}

#[test]
fn empty_input_is_an_error_not_a_panic() {
    assert!(decode_text(&[]).is_err());
}
