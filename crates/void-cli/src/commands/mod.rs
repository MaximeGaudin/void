pub mod archive;
pub mod calendar;
pub mod channels;
pub mod connector_factory;
pub mod contacts;
pub mod doctor;
pub mod forward;
pub mod gdrive;
pub mod gmail;
pub mod hackernews;
pub mod hook;
pub mod inbox;
pub mod linkedin;
pub mod messages;
pub mod mute;
pub mod pagination;
pub mod remote;
pub mod reply;
pub mod resolve;
pub mod search;
pub mod send;
pub mod setup;
pub mod slack;
pub mod status;
pub mod sync;
pub mod telegram;
pub mod whatsapp;

/// Write downloaded bytes to `path`, creating parent directories as needed.
/// Download commands receive staging paths in remote mode; the staging
/// directory may not exist yet on a fresh store.
pub(crate) fn write_download(path: &str, data: &[u8]) -> std::io::Result<()> {
    let path = std::path::Path::new(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(path, data)
}

#[cfg(test)]
mod tests {
    use super::write_download;

    #[test]
    fn write_download_creates_parent_directories() {
        let dir = std::env::temp_dir().join(format!("void-write-dl-{}", uuid::Uuid::new_v4()));
        let out = dir.join("staging").join("file.bin");
        write_download(out.to_str().unwrap(), b"data").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"data");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn write_download_handles_bare_filename() {
        let dir = std::env::temp_dir().join(format!("void-write-dl-cwd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("plain.bin");
        write_download(out.to_str().unwrap(), b"x").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(dir);
    }
}
