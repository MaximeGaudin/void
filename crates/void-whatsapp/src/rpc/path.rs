//! Cross-platform IPC endpoint paths for the WhatsApp RPC server.

use std::path::{Path, PathBuf};

#[cfg(unix)]
fn store_hash(store_path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    store_path.hash(&mut hasher);
    hasher.finish()
}

/// Unix domain socket path. Uses `/tmp` to stay within `SUN_LEN` on macOS.
#[cfg(unix)]
pub fn endpoint_path(store_path: &Path) -> PathBuf {
    PathBuf::from(format!("/tmp/void-wa-{:x}.sock", store_hash(store_path)))
}

/// Named pipe identifier derived from the store path (Windows).
#[cfg(windows)]
pub fn endpoint_path(store_path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    store_path.hash(&mut hasher);
    format!(r"\\.\pipe\void-wa-rpc-{:x}", hasher.finish())
}

/// Remove a stale Unix socket before binding.
#[cfg(unix)]
pub fn remove_stale_endpoint(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).ok();
    }
}

#[cfg(windows)]
pub fn remove_stale_endpoint(_path: &str) {}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn endpoint_path_is_deterministic_for_same_store() {
        let store = PathBuf::from("/home/u/.void/void.db");
        assert_eq!(endpoint_path(&store), endpoint_path(&store));
    }

    #[test]
    fn endpoint_path_differs_for_distinct_stores() {
        let a = endpoint_path(&PathBuf::from("/home/a/void.db"));
        let b = endpoint_path(&PathBuf::from("/home/b/void.db"));
        assert_ne!(a, b);
    }

    #[test]
    fn endpoint_path_uses_tmp_socket_naming() {
        let p = endpoint_path(&PathBuf::from("/some/store.db"));
        let s = p.to_string_lossy();
        assert!(s.starts_with("/tmp/void-wa-"), "{s}");
        assert!(s.ends_with(".sock"), "{s}");
    }

    #[test]
    fn remove_stale_endpoint_deletes_existing_file() {
        let dir = std::env::temp_dir().join(format!("void-wa-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("stale.sock");
        std::fs::write(&file, b"x").unwrap();
        assert!(file.exists());

        remove_stale_endpoint(&file);
        assert!(!file.exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_stale_endpoint_is_noop_for_missing_file() {
        let missing =
            std::env::temp_dir().join(format!("void-wa-missing-{}", uuid::Uuid::new_v4()));
        // Should not panic when the path does not exist.
        remove_stale_endpoint(&missing);
        assert!(!missing.exists());
    }
}
