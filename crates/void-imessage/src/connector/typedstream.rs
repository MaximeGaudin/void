//! Decoder for Apple's `typedstream` (NSArchiver) format.
//!
//! Recent macOS stores message bodies in `message.attributedBody` rather than
//! `message.text`, serialized as an `NSMutableAttributedString` in the legacy
//! `typedstream` format. Void only needs the plain text, not the attribute
//! runs, so this is a targeted reader rather than a general NSArchiver
//! implementation.
//!
//! Layout, as observed on macOS 26.6.2 (byte offsets from the start):
//!
//! ```text
//! 04 0b "streamtyped"   magic
//! 81 e8 03              system version 1000, as an extended-width integer
//! ...                   class chain: NSMutableAttributedString, NSString, ...
//! 84 01 2b              type descriptor of length 1, holding '+' (char string)
//! <len> <bytes>         the UTF-8 string itself
//! ```
//!
//! Integers are variable width: a byte below `0x81` is the value itself, `0x81`
//! introduces a little-endian `i16`, and `0x82` a little-endian `i32`. The
//! same encoding carries the version number and the string length, which is
//! why a 600 byte string reads as `81 58 02`.

use anyhow::{anyhow, bail, Context, Result};

const MAGIC: &[u8] = b"\x04\x0bstreamtyped";

/// Type descriptor introducing the text payload: a one character type string
/// holding `+`, NSArchiver's encoding for a C string.
const TEXT_MARKER: &[u8] = &[0x84, 0x01, b'+'];

const INT16_PREFIX: u8 = 0x81;
const INT32_PREFIX: u8 = 0x82;

/// Reads the variable-width integer at `*pos`, advancing it past the value.
fn read_int(blob: &[u8], pos: &mut usize) -> Result<i64> {
    let first = *blob
        .get(*pos)
        .ok_or_else(|| anyhow!("typedstream ended while reading an integer"))?;
    *pos += 1;

    match first {
        INT16_PREFIX => {
            let bytes = blob
                .get(*pos..*pos + 2)
                .ok_or_else(|| anyhow!("typedstream ended inside a 16-bit integer"))?;
            *pos += 2;
            Ok(i64::from(i16::from_le_bytes([bytes[0], bytes[1]])))
        }
        INT32_PREFIX => {
            let bytes = blob
                .get(*pos..*pos + 4)
                .ok_or_else(|| anyhow!("typedstream ended inside a 32-bit integer"))?;
            *pos += 4;
            Ok(i64::from(i32::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ])))
        }
        // Anything else is the value itself, as a signed byte.
        other => Ok(i64::from(other as i8)),
    }
}

/// Extract the plain text of an `NSAttributedString` from a `typedstream` blob.
///
/// Returns `Ok(Some(text))` on success, including `Ok(Some(String::new()))` for
/// an attributed string that is genuinely empty (an attachment-only message).
/// Returns `Err` when the blob is not a `typedstream` archive or is truncated:
/// callers run this over a live database, so a malformed row must never panic.
pub fn decode_text(blob: &[u8]) -> Result<Option<String>> {
    if blob.len() < MAGIC.len() || !blob.starts_with(MAGIC) {
        bail!("not a typedstream archive: missing the streamtyped magic");
    }

    // The class chain between the header and the payload varies with the
    // message (plain string, attributed string, attachment placeholder), so we
    // locate the text by its type descriptor rather than by a fixed offset.
    let marker = blob
        .windows(TEXT_MARKER.len())
        .position(|w| w == TEXT_MARKER)
        .context("typedstream holds no char-string payload")?;

    let mut pos = marker + TEXT_MARKER.len();
    let len = read_int(blob, &mut pos)?;
    let len = usize::try_from(len).map_err(|_| anyhow!("negative string length in typedstream"))?;

    let bytes = blob
        .get(pos..pos + len)
        .ok_or_else(|| anyhow!("typedstream declares {len} bytes of text but holds fewer"))?;

    // Messages writes UTF-8 here. Fall back to a lossy read rather than
    // dropping a message: an unreadable character is better than a lost row.
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(Some(text.to_string())),
        Err(_) => {
            tracing::warn!("typedstream payload was not valid UTF-8, decoding lossily");
            Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
        }
    }
}
