//! Resolution and first-run creation of the master key.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use aes_gcm::aead::{KeyInit, OsRng};
use aes_gcm::Aes256Gcm;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use zeroize::Zeroizing;

use super::SecretError;

const KEYRING_SERVICE: &str = "void";
const KEYRING_ACCOUNT: &str = "master-key";
const KEY_FILE: &str = "master.key";
const LOCK_FILE: &str = "master.key.lock";
/// Records that the key lives in the OS credential store.
const KEYRING_MARKER: &str = "master.key.keyring";
const ENV_KEY: &str = "VOID_MASTER_KEY";
const ENV_STORE: &str = "VOID_SECRET_STORE";

/// Where the master key came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    /// `VOID_MASTER_KEY` environment variable.
    Env,
    /// OS credential store (Keychain / Credential Manager / Secret Service).
    Keyring,
    /// Owner-only key file — fallback when no credential store is reachable.
    File(PathBuf),
    /// Fixed key used by tests.
    Test,
}

impl std::fmt::Display for KeySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeySource::Env => write!(f, "{ENV_KEY} environment variable"),
            KeySource::Keyring => write!(f, "OS credential store ({})", keyring_store_name()),
            KeySource::File(p) => write!(f, "key file {}", p.display()),
            KeySource::Test => write!(f, "test key"),
        }
    }
}

fn keyring_store_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS Keychain"
    } else if cfg!(windows) {
        "Windows Credential Manager"
    } else {
        "Secret Service"
    }
}

/// A 256-bit AES key plus its provenance. Zeroed on drop.
pub struct MasterKey {
    key: Zeroizing<[u8; 32]>,
    source: KeySource,
}

impl MasterKey {
    pub(super) fn bytes(&self) -> &[u8] {
        self.key.as_slice()
    }

    pub fn source(&self) -> &KeySource {
        &self.source
    }

    fn from_slice(bytes: &[u8], source: KeySource) -> Result<Self, String> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| format!("expected 32 bytes, got {}", bytes.len()))?;
        Ok(Self {
            key: Zeroizing::new(arr),
            source,
        })
    }

    fn from_b64(b64: &str, source: KeySource) -> Result<Self, String> {
        let bytes = Zeroizing::new(
            B64.decode(b64.trim())
                .map_err(|e| format!("invalid base64: {e}"))?,
        );
        Self::from_slice(&bytes, source)
    }

    fn generate(source: KeySource) -> Self {
        let key = Aes256Gcm::generate_key(OsRng);
        let mut arr = Zeroizing::new([0u8; 32]);
        arr.copy_from_slice(&key);
        Self { key: arr, source }
    }

    fn to_b64(&self) -> Zeroizing<String> {
        Zeroizing::new(B64.encode(self.key.as_slice()))
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MasterKey")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

static MASTER: OnceLock<Result<MasterKey, String>> = OnceLock::new();

const TEST_KEY: [u8; 32] = [0x42; 32];

/// Pin the process to a fixed, well-known key so tests never touch the real
/// OS credential store. Must be called before the first secret operation.
#[doc(hidden)]
pub fn use_test_master_key() {
    let _ = MASTER.set(Ok(MasterKey {
        key: Zeroizing::new(TEST_KEY),
        source: KeySource::Test,
    }));
}

/// The process-wide master key, resolved (and created on first run) lazily.
///
/// Only called when there is something to encrypt or decrypt, so configs
/// without credentials never touch the credential store.
pub fn master_key() -> Result<&'static MasterKey, SecretError> {
    MASTER
        .get_or_init(resolve)
        .as_ref()
        .map_err(|e| SecretError::Key(e.clone()))
}

fn resolve() -> Result<MasterKey, String> {
    #[cfg(test)]
    {
        Ok(MasterKey {
            key: Zeroizing::new(TEST_KEY),
            source: KeySource::Test,
        })
    }
    #[cfg(not(test))]
    {
        resolve_real()
    }
}

#[cfg_attr(test, allow(dead_code))]
fn resolve_real() -> Result<MasterKey, String> {
    if let Ok(b64) = std::env::var(ENV_KEY) {
        return MasterKey::from_b64(&b64, KeySource::Env).map_err(|e| format!("{ENV_KEY}: {e}"));
    }

    let dir = crate::config::config_dir();
    let key_file = dir.join(KEY_FILE);
    let marker = dir.join(KEYRING_MARKER);

    // Whichever backend first held the key stays authoritative: silently
    // switching (e.g. a daemon with a D-Bus session vs. an SSH shell without
    // one) would orphan everything encrypted under the other key.
    if key_file.exists() {
        return read_key_file(&key_file);
    }
    if marker.exists() {
        return keyring_get(true);
    }

    let force_file = std::env::var(ENV_STORE).is_ok_and(|v| v.eq_ignore_ascii_case("file"));
    with_creation_lock(&dir, || {
        if key_file.exists() {
            return read_key_file(&key_file);
        }
        if !force_file && keyring_reachable() {
            if let Some(key) = keyring_get_or_create()? {
                if let Err(e) = crate::config::write_secure(&marker, b"keyring\n") {
                    tracing::warn!(path = %marker.display(), error = %e, "failed to write key backend marker");
                }
                return Ok(key);
            }
        }
        create_key_file(&key_file)
    })
}

/// Avoid D-Bus activation timeouts on headless Linux: without a session bus
/// there is no Secret Service to talk to.
fn keyring_reachable() -> bool {
    if cfg!(all(unix, not(target_os = "macos")))
        && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
    {
        return false;
    }
    keyring::Entry::store_status().is_ok()
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("{}: {e}", keyring_store_name()))
}

fn keyring_unreadable(e: impl std::fmt::Display) -> String {
    format!(
        "cannot read the void master key from {} ({KEYRING_SERVICE}/{KEYRING_ACCOUNT}): {e}. \
         On a session without access to it (SSH, cron), export it as {ENV_KEY}",
        keyring_store_name()
    )
}

/// Read the key from the credential store. With `required`, a missing or
/// unreachable entry is an error (the marker says the key lives there).
fn keyring_get(required: bool) -> Result<MasterKey, String> {
    if required && !keyring_reachable() {
        return Err(keyring_unreadable("credential store not reachable"));
    }
    let entry = keyring_entry()?;
    let b64 = Zeroizing::new(entry.get_password().map_err(keyring_unreadable)?);
    MasterKey::from_b64(&b64, KeySource::Keyring).map_err(keyring_unreadable)
}

/// Fetch the key from the credential store, creating it on first run.
/// `Ok(None)` when the store refuses the write, so the caller falls back to
/// the key file rather than leaving tokens in clear.
fn keyring_get_or_create() -> Result<Option<MasterKey>, String> {
    let entry = keyring_entry()?;
    match entry.get_password() {
        Ok(b64) => {
            let b64 = Zeroizing::new(b64);
            MasterKey::from_b64(&b64, KeySource::Keyring)
                .map(Some)
                .map_err(keyring_unreadable)
        }
        Err(keyring::Error::NoEntry) => {
            let key = MasterKey::generate(KeySource::Keyring);
            match entry.set_password(&key.to_b64()) {
                Ok(()) => {
                    tracing::info!("created void master key in {}", keyring_store_name());
                    Ok(Some(key))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "credential store refused the master key; using key file");
                    Ok(None)
                }
            }
        }
        // Any other read error (access denied, locked keychain) must not lead to
        // a fresh key: data encrypted under the stored one would become unreadable.
        Err(e) => Err(keyring_unreadable(e)),
    }
}

fn read_key_file(path: &Path) -> Result<MasterKey, String> {
    let content = Zeroizing::new(
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
    );
    MasterKey::from_b64(&content, KeySource::File(path.to_path_buf()))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn create_key_file(path: &Path) -> Result<MasterKey, String> {
    let key = MasterKey::generate(KeySource::File(path.to_path_buf()));
    crate::config::write_secure(path, key.to_b64().as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    tracing::warn!(
        path = %path.display(),
        "no OS credential store reachable; void master key stored in an owner-only file"
    );
    Ok(key)
}

/// Serialize first-run key creation across processes (sync daemon + CLI) so two
/// racing writers cannot each encrypt under a different key.
fn with_creation_lock<T>(dir: &Path, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lock = dir.join(LOCK_FILE);
    let deadline = SystemTime::now() + Duration::from_secs(10);
    loop {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age > Duration::from_secs(30));
                if stale {
                    let _ = std::fs::remove_file(&lock);
                    continue;
                }
                if SystemTime::now() > deadline {
                    return Err(format!("timed out waiting for {}", lock.display()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("{}: {e}", lock.display())),
        }
    }
    let result = f();
    let _ = std::fs::remove_file(&lock);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_roundtrip() {
        let k = MasterKey::generate(KeySource::Test);
        let back = MasterKey::from_b64(&k.to_b64(), KeySource::Test).unwrap();
        assert_eq!(k.bytes(), back.bytes());
    }

    #[test]
    fn rejects_wrong_length() {
        let err = MasterKey::from_b64(&B64.encode([1u8; 16]), KeySource::Env).unwrap_err();
        assert!(err.contains("32 bytes"));
    }

    #[test]
    fn key_file_roundtrip_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(KEY_FILE);
        let created = create_key_file(&path).unwrap();
        let read = read_key_file(&path).unwrap();
        assert_eq!(created.bytes(), read.bytes());
        assert_eq!(read.source(), &KeySource::File(path.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn creation_lock_is_released() {
        let dir = tempfile::tempdir().unwrap();
        with_creation_lock(dir.path(), || Ok(())).unwrap();
        assert!(!dir.path().join(LOCK_FILE).exists());
        with_creation_lock(dir.path(), || Ok(())).unwrap();
    }

    #[test]
    fn debug_never_prints_key() {
        let k = MasterKey::generate(KeySource::Test);
        let dbg = format!("{k:?}");
        assert!(!dbg.contains(&*k.to_b64()));
    }
}
