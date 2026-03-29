use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use clap::{Args, Subcommand};

use void_core::config::{self, VoidConfig};
use void_kb::chunking::{chunk_text, ChunkConfig};
use void_kb::db::KbDatabase;
use void_kb::embedding::{Embedder, MockEmbedder};
use void_kb::models::{Document, MetadataEntry, SourceType};
use void_kb::search::hybrid_search;
use void_kb::sync::{hash_content, sync_folder_with_progress, SyncEvent};

#[derive(Debug, Args)]
pub struct KbArgs {
    #[command(subcommand)]
    pub command: KbCommand,
}

#[derive(Debug, Subcommand)]
pub enum KbCommand {
    /// Add content to the knowledge base (text or file)
    Add(AddArgs),
    /// Search the knowledge base
    Search(SearchArgs),
    /// Register and sync a folder with the knowledge base
    Sync(SyncArgs),
    /// List all documents in the knowledge base
    List(ListArgs),
    /// Remove a document from the knowledge base
    Remove(RemoveArgs),
    /// Show knowledge base status and statistics
    Status,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Text content to add (mutually exclusive with --file)
    pub content: Option<String>,

    /// Path to a file to add (mutually exclusive with positional content)
    #[arg(long, conflicts_with = "content")]
    pub file: Option<PathBuf>,

    /// Metadata in KEY:VALUE format (repeatable)
    #[arg(long = "metadata", value_name = "KEY:VALUE")]
    pub metadata: Vec<String>,

    /// Expiration date in ISO 8601 / RFC 3339 format
    #[arg(long)]
    pub expiration: Option<String>,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Semantic search query (required)
    #[arg(long)]
    pub semantic_query: String,

    /// Grep term for lexical matching (optional)
    #[arg(long)]
    pub grep: Option<String>,

    /// Number of results to return
    #[arg(long, default_value = "10")]
    pub size: usize,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Path to the folder to sync
    pub folder_path: String,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Number of results per page
    #[arg(long, short = 'n', default_value = "50")]
    pub size: i64,

    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Document ID to remove
    pub doc_id: String,
}

pub fn run(args: &KbArgs) -> anyhow::Result<()> {
    match &args.command {
        KbCommand::Add(a) => run_add(a),
        KbCommand::Search(a) => run_search(a),
        KbCommand::Sync(a) => run_sync(a),
        KbCommand::List(a) => run_list(a),
        KbCommand::Remove(a) => run_remove(a),
        KbCommand::Status => run_status(),
    }
}

fn open_kb_db() -> anyhow::Result<KbDatabase> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load_or_default(&config_path);
    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;
    let kb_path = store_path.join("kb.db");
    KbDatabase::open(&kb_path)
}

fn build_embedder() -> anyhow::Result<Box<dyn Embedder>> {
    // TODO: Replace with real Qwen3 embedder when fastembed dependency is added.
    // For now, use MockEmbedder so the full pipeline is testable end-to-end.
    Ok(Box::new(MockEmbedder::new(1024)))
}

fn parse_metadata(raw: &[String]) -> anyhow::Result<Vec<MetadataEntry>> {
    let mut entries = Vec::new();
    for item in raw {
        let (key, value) = item.split_once(':').ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid metadata format: \"{item}\". Expected KEY:VALUE"
            )
        })?;
        let key = key.trim();
        let value = value.trim();
        anyhow::ensure!(!key.is_empty(), "Metadata key cannot be empty in \"{item}\"");
        anyhow::ensure!(!value.is_empty(), "Metadata value cannot be empty in \"{item}\"");
        entries.push(MetadataEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(entries)
}

fn validate_expiration(exp: Option<&str>) -> anyhow::Result<Option<String>> {
    match exp {
        None => Ok(None),
        Some(s) => {
            chrono::DateTime::parse_from_rfc3339(s).map_err(|e| {
                anyhow::anyhow!("Invalid expiration date \"{s}\": {e}. Expected ISO 8601 / RFC 3339 format (e.g. 2025-12-31T23:59:59Z)")
            })?;
            Ok(Some(s.to_string()))
        }
    }
}

fn run_add(args: &AddArgs) -> anyhow::Result<()> {
    let (content, source_type, source_path) = match (&args.content, &args.file) {
        (Some(text), None) => (text.clone(), SourceType::Text, None),
        (None, Some(path)) => {
            anyhow::ensure!(path.exists(), "File not found: {}", path.display());
            let text = std::fs::read_to_string(path)?;
            (text, SourceType::File, Some(path.to_string_lossy().to_string()))
        }
        (None, None) => anyhow::bail!("Provide either text content or --file <PATH>"),
        (Some(_), Some(_)) => unreachable!("clap conflicts_with prevents this"),
    };

    let metadata = parse_metadata(&args.metadata)?;
    let expiration = validate_expiration(args.expiration.as_deref())?;

    let db = open_kb_db()?;
    let embedder = build_embedder()?;

    let now = chrono::Utc::now().to_rfc3339();
    let doc_id = uuid::Uuid::new_v4().to_string();
    let content_hash = hash_content(content.as_bytes());

    let doc = Document {
        id: doc_id.clone(),
        content: content.clone(),
        source_type,
        source_path,
        content_hash,
        expiration,
        created_at: now.clone(),
        updated_at: now,
        metadata,
    };
    db.insert_document(&doc)?;

    let config = ChunkConfig::default();
    let text_chunks = chunk_text(&content, &config);

    if !text_chunks.is_empty() {
        let chunk_data: Vec<(String, i64, i64, i64)> = text_chunks
            .iter()
            .map(|c| (c.text.clone(), c.index as i64, c.start_byte as i64, c.end_byte as i64))
            .collect();

        let mut all_embeddings = Vec::with_capacity(text_chunks.len());
        for batch in text_chunks.chunks(16) {
            let texts: Vec<&str> = batch.iter().map(|c| c.text.as_str()).collect();
            let embs = embedder.embed(&texts)?;
            all_embeddings.extend(embs);
        }

        db.insert_chunks_with_embeddings(&doc_id, &chunk_data, &all_embeddings)?;
    }

    let output = serde_json::json!({
        "data": {
            "document_id": doc_id,
            "chunks": text_chunks.len(),
        },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_search(args: &SearchArgs) -> anyhow::Result<()> {
    let db = open_kb_db()?;
    let embedder = build_embedder()?;

    let results = hybrid_search(
        &db,
        embedder.as_ref(),
        &args.semantic_query,
        args.grep.as_deref(),
        args.size,
    )?;

    let output = serde_json::json!({
        "data": results,
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_sync(args: &SyncArgs) -> anyhow::Result<()> {
    let db = open_kb_db()?;
    let embedder = build_embedder()?;

    let start = Instant::now();
    let file_start_time: Mutex<Option<Instant>> = Mutex::new(None);
    let cumulative_ms: Mutex<u128> = Mutex::new(0);

    let progress: Box<dyn Fn(SyncEvent) + Send> = Box::new(move |event| {
        match event {
            SyncEvent::Scanning => {
                eprint!("\r\x1b[2KScanning folder...");
            }
            SyncEvent::ScanComplete { total_files } => {
                eprintln!("\r\x1b[2KFound {total_files} file(s) on disk. Computing diff...");
            }
            SyncEvent::DiffComputed { to_add, to_update, to_delete, unchanged } => {
                let total = to_add + to_update;
                if total == 0 && to_delete == 0 {
                    eprintln!("Nothing to do ({unchanged} file(s) unchanged).");
                } else {
                    let mut parts = Vec::new();
                    if to_add > 0 { parts.push(format!("{to_add} new")); }
                    if to_update > 0 { parts.push(format!("{to_update} modified")); }
                    if to_delete > 0 { parts.push(format!("{to_delete} deleted")); }
                    if unchanged > 0 { parts.push(format!("{unchanged} unchanged")); }
                    eprintln!("Sync plan: {}. Indexing {total} file(s)...", parts.join(", "));
                }
            }
            SyncEvent::FileStart { path, index, total } => {
                *file_start_time.lock().unwrap() = Some(Instant::now());
                let short = short_path(&path);
                let pct = if total > 0 { index * 100 / total } else { 0 };
                let eta = eta_string(index - 1, total, &cumulative_ms);
                eprint!("\r\x1b[2K[{index}/{total}] {pct}% {short}{eta}");
            }
            SyncEvent::FileDone { index, total, .. } => {
                if let Some(t) = file_start_time.lock().unwrap().take() {
                    *cumulative_ms.lock().unwrap() += t.elapsed().as_millis();
                }
                if index == total {
                    eprintln!();
                }
            }
            SyncEvent::Done => {
                let elapsed = start.elapsed();
                eprintln!("Sync completed in {}", format_duration(elapsed));
            }
        }
    });

    let report = sync_folder_with_progress(&db, embedder.as_ref(), &args.folder_path, progress)?;

    let output = serde_json::json!({
        "data": {
            "added": report.added,
            "updated": report.updated,
            "deleted": report.deleted,
            "errors": report.errors,
        },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn short_path(path: &str) -> &str {
    path.rsplit_once('/')
        .map(|(_, name)| name)
        .or_else(|| path.rsplit_once('\\').map(|(_, name)| name))
        .unwrap_or(path)
}

fn eta_string(completed: usize, total: usize, cumulative_ms: &Mutex<u128>) -> String {
    if completed == 0 {
        return String::new();
    }
    let elapsed_ms = *cumulative_ms.lock().unwrap();
    let avg_ms = elapsed_ms / completed as u128;
    let remaining = (total - completed) as u128;
    let eta_ms = avg_ms * remaining;
    let eta_dur = std::time::Duration::from_millis(eta_ms as u64);
    format!("  ETA {}", format_duration(eta_dur))
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m {:02}s", secs / 3600, (secs % 3600) / 60, secs % 60)
    }
}

fn run_list(args: &ListArgs) -> anyhow::Result<()> {
    let db = open_kb_db()?;
    db.cleanup_expired()?;

    let offset = (args.page - 1).max(0) * args.size;
    let (docs, total) = db.list_documents(args.size, offset)?;

    let total_pages = (total + args.size - 1) / args.size;
    let output = serde_json::json!({
        "data": docs,
        "pagination": {
            "current_page": args.page,
            "page_size": args.size,
            "total_elements": total,
            "total_pages": total_pages,
        },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_remove(args: &RemoveArgs) -> anyhow::Result<()> {
    let db = open_kb_db()?;
    let deleted = db.delete_document(&args.doc_id)?;

    if !deleted {
        anyhow::bail!("Document not found: {}", args.doc_id);
    }

    let output = serde_json::json!({
        "data": { "deleted": args.doc_id },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_status() -> anyhow::Result<()> {
    let db = open_kb_db()?;
    let mut status = db.status()?;

    let config_path = config::default_config_path();
    let cfg = VoidConfig::load_or_default(&config_path);
    let kb_path = cfg.store_path().join("kb.db");
    if let Ok(meta) = std::fs::metadata(&kb_path) {
        status.db_size_bytes = meta.len();
    }

    let output = serde_json::json!({
        "data": status,
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_metadata_valid() {
        let raw = vec!["author:Alice".into(), "tag:test".into()];
        let result = parse_metadata(&raw).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].key, "author");
        assert_eq!(result[0].value, "Alice");
    }

    #[test]
    fn parse_metadata_with_colons_in_value() {
        let raw = vec!["url:https://example.com".into()];
        let result = parse_metadata(&raw).unwrap();
        assert_eq!(result[0].key, "url");
        assert_eq!(result[0].value, "https://example.com");
    }

    #[test]
    fn parse_metadata_empty_key_rejected() {
        let raw = vec![":value".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn parse_metadata_empty_value_rejected() {
        let raw = vec!["key:".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn parse_metadata_no_colon_rejected() {
        let raw = vec!["novalue".into()];
        assert!(parse_metadata(&raw).is_err());
    }

    #[test]
    fn validate_expiration_valid() {
        let result = validate_expiration(Some("2025-12-31T23:59:59Z")).unwrap();
        assert_eq!(result, Some("2025-12-31T23:59:59Z".to_string()));
    }

    #[test]
    fn validate_expiration_invalid() {
        assert!(validate_expiration(Some("not-a-date")).is_err());
    }

    #[test]
    fn validate_expiration_none() {
        assert_eq!(validate_expiration(None).unwrap(), None);
    }

    #[test]
    fn validate_expiration_with_offset() {
        let result = validate_expiration(Some("2025-06-15T10:00:00+02:00")).unwrap();
        assert!(result.is_some());
    }
}
