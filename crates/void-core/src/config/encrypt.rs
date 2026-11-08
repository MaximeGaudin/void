//! Encryption of credential fields in `config.toml` (see [`crate::secrets`]).

use super::connection::ConnectionConfig;
use super::VoidConfig;
use crate::secrets::{self, is_encrypted_value, is_secret_field};

/// Where each credential field of a config stands.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SecretsReport {
    /// Number of credential fields stored as `enc:v1:` ciphertext.
    pub encrypted: usize,
    /// `(connection id, field)` still stored in plaintext.
    pub plaintext: Vec<(String, String)>,
    /// `(connection id, field)` whose ciphertext cannot be opened with the
    /// current master key.
    pub undecryptable: Vec<(String, String)>,
}

fn secret_strings_mut(conn: &mut ConnectionConfig) -> impl Iterator<Item = (&String, &mut String)> {
    conn.settings.iter_mut().filter_map(|(k, v)| match v {
        toml::Value::String(s) if is_secret_field(k) && !s.is_empty() => Some((k, s)),
        _ => None,
    })
}

impl VoidConfig {
    /// Replace `enc:v1:` values with their plaintext, in memory.
    ///
    /// A value that cannot be decrypted (key unavailable, wrong key) is left as
    /// ciphertext and logged, so a later `save` writes it back unchanged rather
    /// than destroying it.
    pub(super) fn decrypt_secrets(&mut self) {
        for conn in &mut self.connections {
            let id = conn.id.clone();
            for (field, value) in secret_strings_mut(conn) {
                if !is_encrypted_value(value) {
                    continue;
                }
                match secrets::decrypt_value(field, value) {
                    Ok(pt) => *value = pt,
                    Err(e) => tracing::warn!(
                        connection = %id,
                        field = %field,
                        error = %e,
                        "cannot decrypt credential; leaving it encrypted"
                    ),
                }
            }
        }
    }

    /// Copy of `self` with every plaintext credential sealed, for writing.
    ///
    /// If the master key is unavailable the plaintext is kept (and logged):
    /// writing nothing would lose the user's token.
    pub(super) fn with_encrypted_secrets(&self) -> Self {
        let mut out = self.clone();
        let mut warned = false;
        for conn in &mut out.connections {
            for (field, value) in secret_strings_mut(conn) {
                if is_encrypted_value(value) {
                    continue;
                }
                match secrets::encrypt_value(field, value) {
                    Ok(enc) => *value = enc,
                    Err(e) => {
                        if !warned {
                            tracing::warn!(error = %e, "cannot encrypt credentials; saving them in plaintext");
                            warned = true;
                        }
                    }
                }
            }
        }
        out
    }

    /// Classify the credential fields of a config parsed with [`VoidConfig::parse`]
    /// (i.e. as stored on disk). Tries to decrypt ciphertexts to detect a key
    /// mismatch, which touches the master key only if ciphertexts exist.
    pub fn secrets_report(&self) -> SecretsReport {
        let mut report = SecretsReport::default();
        for conn in &self.connections {
            for (field, value) in &conn.settings {
                let Some(value) = value.as_str() else {
                    continue;
                };
                if !is_secret_field(field) || value.is_empty() {
                    continue;
                }
                if !is_encrypted_value(value) {
                    report.plaintext.push((conn.id.clone(), field.clone()));
                } else if secrets::decrypt_value(field, value).is_ok() {
                    report.encrypted += 1;
                } else {
                    report.undecryptable.push((conn.id.clone(), field.clone()));
                }
            }
        }
        report
    }
}

/// Seal plaintext credentials in raw config text, preserving comments and
/// layout. Returns `None` when there is nothing to seal or the master key is
/// unavailable (the file is then left untouched).
pub(super) fn encrypt_plaintext_in_text(content: &str) -> Option<String> {
    let mut doc: toml_edit::DocumentMut = content.parse().ok()?;
    let connections = doc
        .get_mut("connections")
        .and_then(|item| item.as_array_of_tables_mut())?;

    let mut changed = false;
    for table in connections.iter_mut() {
        for (key, item) in table.iter_mut() {
            if !is_secret_field(key.get()) {
                continue;
            }
            let Some(value) = item.as_value_mut() else {
                continue;
            };
            let Some(plain) = value.as_str() else {
                continue;
            };
            if plain.is_empty() || is_encrypted_value(plain) {
                continue;
            }
            match secrets::encrypt_value(key.get(), plain) {
                Ok(enc) => {
                    let decor = value.decor().clone();
                    *value = toml_edit::Value::from(enc);
                    *value.decor_mut() = decor;
                    changed = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "cannot encrypt plaintext credentials in config");
                    return None;
                }
            }
        }
    }
    changed.then(|| doc.to_string())
}
