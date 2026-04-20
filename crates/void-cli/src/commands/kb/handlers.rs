use std::path::Path;

use void_core::config::{self, VoidConfig};
use void_kb::chunking::{chunk_text, ChunkConfig};
use void_kb::models::{Document, SourceType, SyncFolder};
use void_kb::search::hybrid_search;
use void_kb::sync::hash_content;

use super::runtime::{build_embedder, open_kb_db, parse_metadata, validate_expiration};
use super::{AddArgs, KbSyncArgs, ListArgs, RemoveArgs, SearchArgs, UnsyncArgs};

pub(super) fn run_add(args: &AddArgs) -> anyhow::Result<()> {
    let (content, source_type, source_path) = match (&args.content, &args.file) {
        (Some(text), None) => (text.clone(), SourceType::Text, None),
        (None, Some(path)) => {
            anyhow::ensure!(path.exists(), "File not found: {}", path.display());
            let text = std::fs::read_to_string(path)?;
            (
                text,
                SourceType::File,
                Some(path.to_string_lossy().to_string()),
            )
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

    let source_mtime = args.file.as_ref().and_then(|p| {
        let meta = std::fs::metadata(p).ok()?;
        let modified = meta.modified().ok()?;
        let dt: chrono::DateTime<chrono::Utc> = modified.into();
        Some(dt.to_rfc3339())
    });

    let doc = Document {
        id: doc_id.clone(),
        content: content.clone(),
        source_type,
        source_path,
        content_hash,
        expiration,
        source_mtime,
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

pub(super) fn run_search(args: &SearchArgs) -> anyhow::Result<()> {
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

pub(super) fn run_sync(args: &KbSyncArgs) -> anyhow::Result<()> {
    let path = Path::new(&args.folder_path);
    anyhow::ensure!(path.exists(), "Folder not found: {}", args.folder_path);
    anyhow::ensure!(path.is_dir(), "Not a directory: {}", args.folder_path);

    let canonical = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    let canonical = {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            std::path::PathBuf::from(stripped)
        } else {
            canonical
        }
    };
    let canonical_str = canonical.to_string_lossy().to_string();

    let db = open_kb_db()?;
    let now = chrono::Utc::now().to_rfc3339();

    let folder = SyncFolder {
        id: uuid::Uuid::new_v4().to_string(),
        folder_path: canonical_str.clone(),
        interval_secs: 60,
        last_scan_at: None,
        created_at: now,
    };
    db.upsert_sync_folder(&folder)?;

    let output = serde_json::json!({
        "data": {
            "folder_path": canonical_str,
            "registered": true,
            "message": "Folder registered for KB sync. Indexing will happen during `void sync`.",
        },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

pub(super) fn run_unsync(args: &UnsyncArgs) -> anyhow::Result<()> {
    let path = Path::new(&args.folder_path);
    let canonical = if path.exists() {
        std::fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    #[cfg(windows)]
    let canonical = {
        let s = canonical.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            std::path::PathBuf::from(stripped)
        } else {
            canonical
        }
    };
    let canonical_str = canonical.to_string_lossy().to_string();

    let db = open_kb_db()?;
    let removed = db.remove_sync_folder(&canonical_str)?;

    let output = serde_json::json!({
        "data": {
            "folder_path": canonical_str,
            "documents_removed": removed,
        },
        "error": null
    });
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

pub(super) fn run_list(args: &ListArgs) -> anyhow::Result<()> {
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

pub(super) fn run_remove(args: &RemoveArgs) -> anyhow::Result<()> {
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

pub(super) fn run_status() -> anyhow::Result<()> {
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
