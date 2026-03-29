use std::collections::HashMap;
use std::path::Path;

use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::chunking::{chunk_text, ChunkConfig};
use crate::db::KbDatabase;
use crate::embedding::Embedder;
use crate::models::{Document, SourceType, SyncFolder};

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "txt", "md", "rst", "json", "csv", "html", "xml", "toml", "yaml", "yml",
    "rs", "py", "js", "ts", "go", "java", "c", "cpp", "h", "hpp", "rb", "sh",
    "sql", "css", "scss", "lua", "zig", "swift", "kt", "r",
];

const BATCH_EMBED_SIZE: usize = 16;

/// Progress events emitted during sync.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Scanning folder for files.
    Scanning,
    /// Scan complete: total files found on disk.
    ScanComplete { total_files: usize },
    /// Diff computed: how many files to add, update, delete, and skip.
    DiffComputed {
        to_add: usize,
        to_update: usize,
        to_delete: usize,
        unchanged: usize,
    },
    /// Starting to process a file (add or update).
    FileStart {
        path: String,
        index: usize,
        total: usize,
    },
    /// Finished processing a file.
    FileDone {
        path: String,
        index: usize,
        total: usize,
        ok: bool,
    },
    /// All processing complete.
    Done,
}

pub type ProgressCallback = Box<dyn Fn(SyncEvent) + Send>;

fn noop_progress() -> ProgressCallback {
    Box::new(|_| {})
}

/// Scan a folder, detect changes, and update the KB accordingly.
pub fn sync_folder(
    db: &KbDatabase,
    embedder: &dyn Embedder,
    folder_path: &str,
) -> anyhow::Result<SyncReport> {
    sync_folder_with_progress(db, embedder, folder_path, noop_progress())
}

/// Like `sync_folder` but accepts a progress callback.
pub fn sync_folder_with_progress(
    db: &KbDatabase,
    embedder: &dyn Embedder,
    folder_path: &str,
    progress: ProgressCallback,
) -> anyhow::Result<SyncReport> {
    let path = Path::new(folder_path);
    anyhow::ensure!(path.exists(), "folder does not exist: {folder_path}");
    anyhow::ensure!(path.is_dir(), "path is not a directory: {folder_path}");

    let canonical = dunce_canonicalize(path)?;
    let canonical_str = canonical.to_string_lossy().to_string();

    let folder = SyncFolder {
        id: uuid::Uuid::new_v4().to_string(),
        folder_path: canonical_str.clone(),
        interval_secs: 60,
        last_scan_at: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    db.upsert_sync_folder(&folder)?;

    let report = diff_and_apply(db, embedder, &canonical_str, &progress)?;

    db.update_sync_folder_scan_time(&canonical_str, &chrono::Utc::now().to_rfc3339())?;

    progress(SyncEvent::Done);
    Ok(report)
}

/// Compute diff between disk and DB, then apply changes.
pub fn diff_and_apply(
    db: &KbDatabase,
    embedder: &dyn Embedder,
    folder_path: &str,
    progress: &dyn Fn(SyncEvent),
) -> anyhow::Result<SyncReport> {
    progress(SyncEvent::Scanning);
    let disk_files = scan_folder(Path::new(folder_path))?;
    let db_entries = db.list_synced_paths_for_folder(folder_path)?;

    debug!(folder = folder_path, disk_count = disk_files.len(), db_count = db_entries.len(), "diff_and_apply");
    progress(SyncEvent::ScanComplete { total_files: disk_files.len() });

    let db_map: HashMap<String, (String, String)> = db_entries
        .into_iter()
        .map(|(id, path, hash)| (path, (id, hash)))
        .collect();

    let mut report = SyncReport::default();

    let mut to_add: Vec<(String, String)> = Vec::new();
    let mut to_update: Vec<(String, String, String)> = Vec::new();

    for (file_path, file_hash) in &disk_files {
        if let Some((doc_id, old_hash)) = db_map.get(file_path) {
            if old_hash != file_hash {
                debug!(file_path, old_hash, file_hash, "hash changed → update");
                to_update.push((doc_id.clone(), file_path.clone(), file_hash.clone()));
            } else {
                debug!(file_path, "hash unchanged → skip");
            }
        } else {
            debug!(file_path, "not in db → add");
            to_add.push((file_path.clone(), file_hash.clone()));
        }
    }

    let disk_paths: std::collections::HashSet<&String> = disk_files.keys().collect();
    let to_delete: Vec<(String, String)> = db_map
        .iter()
        .filter(|(path, _)| !disk_paths.contains(path))
        .map(|(path, (id, _))| (id.clone(), path.clone()))
        .collect();

    let unchanged = disk_files.len() - to_add.len() - to_update.len();
    progress(SyncEvent::DiffComputed {
        to_add: to_add.len(),
        to_update: to_update.len(),
        to_delete: to_delete.len(),
        unchanged,
    });

    for (doc_id, path) in &to_delete {
        info!(doc_id, path, "deleting removed file from KB");
        db.delete_document(doc_id)?;
        report.deleted += 1;
    }

    let total_to_process = to_update.len() + to_add.len();
    let mut processed: usize = 0;

    for (doc_id, file_path, new_hash) in &to_update {
        processed += 1;
        progress(SyncEvent::FileStart {
            path: file_path.clone(),
            index: processed,
            total: total_to_process,
        });
        info!(file_path, "re-indexing modified file");
        db.delete_document(doc_id)?;
        let ok = match ingest_file(db, embedder, file_path, new_hash) {
            Ok(_) => { report.updated += 1; true }
            Err(e) => {
                warn!(file_path, error = %e, "failed to re-index file");
                report.errors += 1;
                false
            }
        };
        progress(SyncEvent::FileDone {
            path: file_path.clone(),
            index: processed,
            total: total_to_process,
            ok,
        });
    }

    for (file_path, file_hash) in &to_add {
        processed += 1;
        progress(SyncEvent::FileStart {
            path: file_path.clone(),
            index: processed,
            total: total_to_process,
        });
        info!(file_path, "indexing new file");
        let ok = match ingest_file(db, embedder, file_path, file_hash) {
            Ok(_) => { report.added += 1; true }
            Err(e) => {
                warn!(file_path, error = %e, "failed to index file");
                report.errors += 1;
                false
            }
        };
        progress(SyncEvent::FileDone {
            path: file_path.clone(),
            index: processed,
            total: total_to_process,
            ok,
        });
    }

    Ok(report)
}

fn ingest_file(
    db: &KbDatabase,
    embedder: &dyn Embedder,
    file_path: &str,
    content_hash: &str,
) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(file_path)?;
    let now = chrono::Utc::now().to_rfc3339();
    let doc_id = uuid::Uuid::new_v4().to_string();

    let doc = Document {
        id: doc_id.clone(),
        content: content.clone(),
        source_type: SourceType::Synced,
        source_path: Some(file_path.to_string()),
        content_hash: content_hash.to_string(),
        expiration: None,
        created_at: now.clone(),
        updated_at: now,
        metadata: vec![],
    };
    db.insert_document(&doc)?;

    let config = ChunkConfig::default();
    let text_chunks = chunk_text(&content, &config);

    if text_chunks.is_empty() {
        return Ok(());
    }

    let chunk_data: Vec<(String, i64, i64, i64)> = text_chunks
        .iter()
        .map(|c| {
            (
                c.text.clone(),
                c.index as i64,
                c.start_byte as i64,
                c.end_byte as i64,
            )
        })
        .collect();

    let mut all_embeddings = Vec::with_capacity(text_chunks.len());
    for batch in text_chunks.chunks(BATCH_EMBED_SIZE) {
        let texts: Vec<&str> = batch.iter().map(|c| c.text.as_str()).collect();
        let embs = embedder.embed(&texts)?;
        all_embeddings.extend(embs);
    }

    db.insert_chunks_with_embeddings(&doc_id, &chunk_data, &all_embeddings)?;
    Ok(())
}

/// Recursively scan a folder and return file paths with their content hashes.
/// Respects `.gitignore` rules (including nested and global gitignore) when a
/// `.git` directory or `.gitignore` file is present.
pub fn scan_folder(root: &Path) -> anyhow::Result<HashMap<String, String>> {
    use ignore::WalkBuilder;

    let mut files = HashMap::new();

    let walker = WalkBuilder::new(root)
        .hidden(true) // skip dotfiles/dotdirs
        .git_ignore(true) // respect .gitignore
        .git_global(true) // respect global gitignore
        .git_exclude(true) // respect .git/info/exclude
        .require_git(false) // still walk even without a .git dir
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "error walking directory");
                continue;
            }
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        if !SUPPORTED_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
            continue;
        }

        match hash_file(path) {
            Ok(hash) => {
                files.insert(path.to_string_lossy().to_string(), hash);
            }
            Err(e) => {
                debug!(path = %path.display(), error = %e, "cannot hash file");
            }
        }
    }

    Ok(files)
}

fn hash_file(path: &Path) -> anyhow::Result<String> {
    let content = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn hash_content(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Cross-platform canonicalize that avoids UNC paths on Windows.
fn dunce_canonicalize(path: &Path) -> anyhow::Result<std::path::PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(std::path::PathBuf::from(stripped));
        }
    }
    Ok(canonical)
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub errors: usize,
}

impl std::fmt::Display for SyncReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "added: {}, updated: {}, deleted: {}, errors: {}",
            self.added, self.updated, self.deleted, self.errors
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbedder;
    use std::io::Write;

    fn setup_temp_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut f1 = std::fs::File::create(dir.path().join("doc1.txt")).unwrap();
        writeln!(f1, "Hello world document one").unwrap();
        let mut f2 = std::fs::File::create(dir.path().join("doc2.md")).unwrap();
        writeln!(f2, "Second document about Rust").unwrap();
        dir
    }

    #[test]
    fn scan_finds_supported_files() {
        let dir = setup_temp_dir();
        std::fs::write(dir.path().join("binary.exe"), b"not text").unwrap();
        std::fs::write(dir.path().join(".hidden.txt"), "hidden").unwrap();

        let files = scan_folder(dir.path()).unwrap();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn scan_recursive_finds_subdirs() {
        let dir = setup_temp_dir();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.txt"), "nested content").unwrap();

        let files = scan_folder(dir.path()).unwrap();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn scan_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden.txt"), "nope").unwrap();
        std::fs::write(dir.path().join("visible.txt"), "yes").unwrap();

        let hidden_dir = dir.path().join(".hidden_dir");
        std::fs::create_dir(&hidden_dir).unwrap();
        std::fs::write(hidden_dir.join("inside.txt"), "nope").unwrap();

        let files = scan_folder(dir.path()).unwrap();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn scan_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("keep.txt"), "keep me").unwrap();
        std::fs::write(dir.path().join("skip.txt"), "ignore me").unwrap();

        let build_dir = dir.path().join("build");
        std::fs::create_dir(&build_dir).unwrap();
        std::fs::write(build_dir.join("output.txt"), "build artifact").unwrap();

        std::fs::write(dir.path().join(".gitignore"), "skip.txt\nbuild/\n").unwrap();

        let files = scan_folder(dir.path()).unwrap();
        let names: Vec<&str> = files
            .keys()
            .filter_map(|p| Path::new(p).file_name()?.to_str())
            .collect();

        assert!(names.contains(&"keep.txt"), "keep.txt should be included");
        assert!(!names.contains(&"skip.txt"), "skip.txt should be gitignored");
        assert!(!names.contains(&"output.txt"), "build/ should be gitignored");
    }

    #[test]
    fn hash_content_deterministic() {
        let a = hash_content(b"hello");
        let b = hash_content(b"hello");
        assert_eq!(a, b);
        assert_ne!(a, hash_content(b"world"));
    }

    #[test]
    fn sync_initial_adds_files() {
        let dir = setup_temp_dir();
        let db = KbDatabase::open_in_memory().unwrap();
        let embedder = MockEmbedder::new(1024);

        let report = sync_folder(&db, &embedder, dir.path().to_str().unwrap()).unwrap();
        assert_eq!(report.added, 2);
        assert_eq!(report.deleted, 0);
        assert_eq!(report.updated, 0);

        let status = db.status().unwrap();
        assert_eq!(status.document_count, 2);
    }

    #[test]
    fn sync_detects_new_file() {
        let dir = setup_temp_dir();
        let db = KbDatabase::open_in_memory().unwrap();
        let embedder = MockEmbedder::new(1024);
        let path = dir.path().to_str().unwrap();

        sync_folder(&db, &embedder, path).unwrap();

        std::fs::write(dir.path().join("new.txt"), "brand new").unwrap();
        let report = sync_folder(&db, &embedder, path).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(status_count(&db), 3);
    }

    #[test]
    fn sync_detects_modified_file() {
        let dir = setup_temp_dir();
        let db = KbDatabase::open_in_memory().unwrap();
        let embedder = MockEmbedder::new(1024);
        let path = dir.path().to_str().unwrap();

        sync_folder(&db, &embedder, path).unwrap();

        std::fs::write(dir.path().join("doc1.txt"), "modified content").unwrap();
        let report = sync_folder(&db, &embedder, path).unwrap();
        assert_eq!(report.updated, 1);
    }

    #[test]
    fn sync_detects_deleted_file() {
        let dir = setup_temp_dir();
        let db = KbDatabase::open_in_memory().unwrap();
        let embedder = MockEmbedder::new(1024);
        let path = dir.path().to_str().unwrap();

        sync_folder(&db, &embedder, path).unwrap();

        std::fs::remove_file(dir.path().join("doc1.txt")).unwrap();
        let report = sync_folder(&db, &embedder, path).unwrap();
        assert_eq!(report.deleted, 1);
        assert_eq!(status_count(&db), 1);
    }

    #[test]
    fn sync_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let db = KbDatabase::open_in_memory().unwrap();
        let embedder = MockEmbedder::new(1024);

        let report = sync_folder(&db, &embedder, dir.path().to_str().unwrap()).unwrap();
        assert_eq!(report.added, 0);
    }

    fn status_count(db: &KbDatabase) -> i64 {
        db.status().unwrap().document_count
    }
}
