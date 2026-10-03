//! Encryption of credentials at rest.
//!
//! Tokens in `config.toml` (Slack, Reddit, GitHub, LinkedIn, Telegram …) and
//! secret-bearing sidecar files (Google OAuth token caches, Telegram session)
//! are sealed with AES-256-GCM under a per-user **master key**. The master key
//! never lives next to the data it protects when an OS credential store is
//! available:
//!
//! 1. `VOID_MASTER_KEY` — base64 of 32 bytes (headless servers, systemd
//!    `LoadCredential=`, CI). Never persisted by void.
//! 2. The OS credential store (macOS Keychain, Windows Credential Manager,
//!    Secret Service on Linux), service `void`, account `master-key`.
//! 3. Fallback when no credential store is reachable (e.g. a headless Linux
//!    box without a D-Bus session): `master.key` in the config directory,
//!    `0600`. Weaker — the key sits on the same disk — but a leaked
//!    `config.toml`, store backup, or remote-mode cache no longer exposes
//!    usable tokens. `void doctor` reports which backend is in use.
//!
//! `VOID_SECRET_STORE=file` forces the fallback (skips the credential store).
//!
//! Formats:
//! - Config values: `enc:v1:<base64(nonce ‖ ciphertext)>`, AAD = field name, so
//!   a ciphertext cannot be moved to another field.
//! - Files: `VOIDSEC1 ‖ nonce ‖ ciphertext`, AAD = magic.
//!
//! Reads accept legacy plaintext transparently so existing installs keep
//! working; callers re-save to migrate. Nothing is ever dropped when the key is
//! unavailable: values stay as they are (plaintext or ciphertext) and a warning
//! is logged.

mod master_key;

use std::io;
use std::path::Path;

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;

pub use master_key::{master_key, KeySource, MasterKey};

#[doc(hidden)]
pub use master_key::use_test_master_key;

/// Connection settings keys whose string values are credentials.
pub const SECRET_FIELDS: &[&str] = &[
    "app_token",
    "user_token",
    "config_refresh_token",
    "client_secret",
    "refresh_token",
    "api_key",
    "api_hash",
    "token",
    "password",
];

/// Prefix of an encrypted config value.
pub const ENC_PREFIX: &str = "enc:v1:";

/// Magic header of an encrypted file.
pub const FILE_MAGIC: &[u8; 8] = b"VOIDSEC1";

const NONCE_LEN: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("master key unavailable: {0}")]
    Key(String),
    #[error("malformed encrypted value")]
    Malformed,
    #[error("decryption failed (wrong master key or tampered data)")]
    Decrypt,
}

impl From<SecretError> for io::Error {
    fn from(e: SecretError) -> Self {
        io::Error::other(e)
    }
}

/// True when `key` names a credential field in connection settings.
pub fn is_secret_field(key: &str) -> bool {
    SECRET_FIELDS.contains(&key)
}

/// True when `value` is an `enc:v1:` ciphertext.
pub fn is_encrypted_value(value: &str) -> bool {
    value.starts_with(ENC_PREFIX)
}

/// True when `data` starts with the encrypted-file magic.
pub fn is_encrypted_file(data: &[u8]) -> bool {
    data.starts_with(FILE_MAGIC)
}

fn seal_with(key: &MasterKey, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
    let cipher = Aes256Gcm::new_from_slice(key.bytes()).map_err(|_| SecretError::Malformed)?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| SecretError::Decrypt)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

fn open_with(key: &MasterKey, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, SecretError> {
    if sealed.len() < NONCE_LEN {
        return Err(SecretError::Malformed);
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key.bytes()).map_err(|_| SecretError::Malformed)?;
    cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| SecretError::Decrypt)
}

/// Encrypt a config value bound to `field`. Returns `enc:v1:…`.
pub fn encrypt_value(field: &str, plaintext: &str) -> Result<String, SecretError> {
    let key = master_key()?;
    let sealed = seal_with(key, field.as_bytes(), plaintext.as_bytes())?;
    Ok(format!("{ENC_PREFIX}{}", B64.encode(sealed)))
}

/// Decrypt an `enc:v1:` config value bound to `field`.
pub fn decrypt_value(field: &str, value: &str) -> Result<String, SecretError> {
    let b64 = value
        .strip_prefix(ENC_PREFIX)
        .ok_or(SecretError::Malformed)?;
    let sealed = B64.decode(b64).map_err(|_| SecretError::Malformed)?;
    let key = master_key()?;
    let pt = open_with(key, field.as_bytes(), &sealed)?;
    String::from_utf8(pt).map_err(|_| SecretError::Malformed)
}

/// Seal bytes into the encrypted-file format (`VOIDSEC1 ‖ nonce ‖ ct`).
pub fn seal_file_bytes(plaintext: &[u8]) -> Result<Vec<u8>, SecretError> {
    let key = master_key()?;
    let sealed = seal_with(key, FILE_MAGIC, plaintext)?;
    let mut out = Vec::with_capacity(FILE_MAGIC.len() + sealed.len());
    out.extend_from_slice(FILE_MAGIC);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Open bytes in the encrypted-file format.
pub fn open_file_bytes(data: &[u8]) -> Result<Vec<u8>, SecretError> {
    let sealed = data
        .strip_prefix(FILE_MAGIC.as_slice())
        .ok_or(SecretError::Malformed)?;
    let key = master_key()?;
    open_with(key, FILE_MAGIC, sealed)
}

/// Contents of a secret file and whether it was still stored in plaintext.
pub struct SecretFile {
    pub contents: Vec<u8>,
    /// `true` for a legacy plaintext file; re-save it with
    /// [`write_secret_file`] to migrate.
    pub was_plaintext: bool,
}

/// Read a secret file, decrypting it if sealed; legacy plaintext passes through.
pub fn read_secret_file(path: &Path) -> io::Result<SecretFile> {
    let data = std::fs::read(path)?;
    if is_encrypted_file(&data) {
        Ok(SecretFile {
            contents: open_file_bytes(&data)?,
            was_plaintext: false,
        })
    } else {
        Ok(SecretFile {
            contents: data,
            was_plaintext: true,
        })
    }
}

/// Seal `contents` and write it owner-only (see [`crate::config::write_secure`]).
///
/// If the master key is unavailable the file is written in plaintext (still
/// `0600`) and a warning is logged — losing a fresh OAuth token would be worse.
pub fn write_secret_file(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    match seal_file_bytes(contents.as_ref()) {
        Ok(sealed) => crate::config::write_secure(path, sealed),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "cannot encrypt secret file; writing plaintext (owner-only)"
            );
            crate::config::write_secure(path, contents)
        }
    }
}

/// Read a secret file and, if it was legacy plaintext, re-save it sealed
/// (best effort). Returns the decrypted contents.
pub fn read_and_migrate_secret_file(path: &Path) -> io::Result<Vec<u8>> {
    let file = read_secret_file(path)?;
    if file.was_plaintext && master_key().is_ok() {
        if let Err(e) = write_secret_file(path, &file.contents) {
            tracing::warn!(path = %path.display(), error = %e, "failed to encrypt legacy secret file");
        }
    }
    Ok(file.contents)
}

#[cfg(test)]
mod tests;
