use std::io::Write;

use void_kb::chunking::{chunk_text, ChunkConfig};
use void_kb::db::KbDatabase;
use void_kb::embedding::{DeterministicEmbedder, Embedder, MockEmbedder};
use void_kb::models::{Document, MetadataEntry, SourceType};
use void_kb::search::hybrid_search;
use void_kb::sync::{hash_content, sync_folder};

fn make_doc(id: &str, content: &str) -> Document {
    let now = chrono::Utc::now().to_rfc3339();
    Document {
        id: id.to_string(),
        content: content.to_string(),
        source_type: SourceType::Text,
        source_path: None,
        content_hash: hash_content(content.as_bytes()),
        expiration: None,
        source_mtime: None,
        created_at: now.clone(),
        updated_at: now,
        metadata: vec![],
    }
}

fn ingest_doc(db: &KbDatabase, embedder: &dyn Embedder, doc: &Document) {
    db.insert_document(doc).unwrap();
    let chunks = chunk_text(&doc.content, &ChunkConfig::default());
    if chunks.is_empty() {
        return;
    }
    let chunk_data: Vec<(String, i64, i64, i64)> = chunks
        .iter()
        .map(|c| (c.text.clone(), c.index as i64, c.start_byte as i64, c.end_byte as i64))
        .collect();
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let embeddings = embedder.embed(&texts).unwrap();
    db.insert_chunks_with_embeddings(&doc.id, &chunk_data, &embeddings)
        .unwrap();
}

// ── End-to-end: add text -> search ─────────────────────────────

#[test]
fn add_text_then_semantic_search_finds_it() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let doc = make_doc("d1", "Rust programming language is great for systems programming");
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(
        &db,
        &embedder,
        "systems programming language",
        None,
        10,
    )
    .unwrap();

    assert!(!results.is_empty(), "search should return results");
    assert_eq!(results[0].document_id, "d1");
}

// ── End-to-end: add file -> search ─────────────────────────────

#[test]
fn add_file_then_search_finds_content() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let mut doc = make_doc("f1", "Machine learning with neural networks and deep learning");
    doc.source_type = SourceType::File;
    doc.source_path = Some("/tmp/ml.txt".to_string());
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(&db, &embedder, "neural networks", None, 10).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].source_type, SourceType::File);
    assert_eq!(results[0].source_path, Some("/tmp/ml.txt".to_string()));
}

// ── Metadata in search results ─────────────────────────────────

#[test]
fn metadata_appears_in_search_results() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let mut doc = make_doc("m1", "Kubernetes container orchestration platform");
    doc.metadata = vec![
        MetadataEntry { key: "author".into(), value: "Alice".into() },
        MetadataEntry { key: "topic".into(), value: "devops".into() },
    ];
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(&db, &embedder, "kubernetes containers", None, 10).unwrap();
    assert!(!results.is_empty());
    let meta = &results[0].metadata;
    assert_eq!(meta["author"], "Alice");
    assert_eq!(meta["topic"], "devops");
}

// ── Remove -> search no longer returns ─────────────────────────

#[test]
fn remove_document_then_search_returns_empty() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let doc = make_doc("r1", "PostgreSQL relational database management");
    ingest_doc(&db, &embedder, &doc);

    let before = hybrid_search(&db, &embedder, "postgresql database", None, 10).unwrap();
    assert!(!before.is_empty());

    db.delete_document("r1").unwrap();

    let after = hybrid_search(&db, &embedder, "postgresql database", None, 10).unwrap();
    assert!(after.is_empty());
}

// ── Expiration filtering ───────────────────────────────────────

#[test]
fn expired_document_not_returned_in_search() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let mut doc = make_doc("e1", "Temporary information about event");
    doc.expiration = Some("2000-01-01T00:00:00Z".to_string());
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(&db, &embedder, "temporary information event", None, 10).unwrap();
    assert!(results.is_empty(), "expired doc should not appear");
}

#[test]
fn non_expired_document_returned() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let mut doc = make_doc("e2", "Future meeting notes about project planning");
    doc.expiration = Some("2099-12-31T23:59:59Z".to_string());
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(&db, &embedder, "meeting notes project", None, 10).unwrap();
    assert!(!results.is_empty());
}

// ── Grep search (accent-insensitive) ───────────────────────────

#[test]
fn grep_search_accent_insensitive() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let doc = make_doc("a1", "Le café est délicieux et le résumé est prêt");
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(
        &db,
        &embedder,
        "french text about coffee",
        Some("cafe"),
        10,
    )
    .unwrap();
    assert!(!results.is_empty(), "accent-insensitive grep should match");
}

// ── Hybrid search: grep boost ──────────────────────────────────

#[test]
fn grep_boost_ranks_exact_match_higher() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = MockEmbedder::new(1024);

    let doc1 = make_doc("h1", "The quick brown fox jumps over the lazy dog");
    let doc2 = make_doc("h2", "A slow red cat sleeps under the happy tree");
    ingest_doc(&db, &embedder, &doc1);
    ingest_doc(&db, &embedder, &doc2);

    let results = hybrid_search(
        &db,
        &embedder,
        "animal jumping",
        Some("fox"),
        10,
    )
    .unwrap();

    assert!(!results.is_empty());
    assert_eq!(
        results[0].document_id, "h1",
        "doc with 'fox' should rank first due to grep boost"
    );
}

// ── Sync folder: add files -> search ───────────────────────────

#[test]
fn sync_folder_then_search() {
    let dir = tempfile::tempdir().unwrap();
    let mut f1 = std::fs::File::create(dir.path().join("rust.txt")).unwrap();
    writeln!(f1, "Rust is a systems programming language focused on safety").unwrap();
    let mut f2 = std::fs::File::create(dir.path().join("python.txt")).unwrap();
    writeln!(f2, "Python is great for data science and scripting").unwrap();

    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    sync_folder(&db, &embedder, dir.path().to_str().unwrap()).unwrap();

    let results = hybrid_search(&db, &embedder, "systems programming safety", None, 10).unwrap();
    assert!(!results.is_empty());
}

// ── Sync: modify file -> updated content found ─────────────────

#[test]
fn sync_modified_file_updates_search() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "Original content about databases").unwrap();

    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);
    let path = dir.path().to_str().unwrap();

    sync_folder(&db, &embedder, path).unwrap();

    std::fs::write(
        dir.path().join("notes.txt"),
        "Updated content about machine learning and AI",
    )
    .unwrap();

    sync_folder(&db, &embedder, path).unwrap();

    let status = db.status().unwrap();
    assert_eq!(status.document_count, 1, "should have exactly one doc after update");
}

// ── Sync: delete file -> gone from search ──────────────────────

#[test]
fn sync_deleted_file_removed_from_search() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("temp.txt"), "Temporary file content").unwrap();

    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = MockEmbedder::new(1024);
    let path = dir.path().to_str().unwrap();

    sync_folder(&db, &embedder, path).unwrap();
    assert_eq!(db.status().unwrap().document_count, 1);

    std::fs::remove_file(dir.path().join("temp.txt")).unwrap();
    sync_folder(&db, &embedder, path).unwrap();
    assert_eq!(db.status().unwrap().document_count, 0);
}

// ── Score ordering ─────────────────────────────────────────────

#[test]
fn results_sorted_by_score_descending() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    for i in 0..5 {
        let doc = make_doc(&format!("s{i}"), &format!("Document number {i} about topic {i}"));
        ingest_doc(&db, &embedder, &doc);
    }

    let results = hybrid_search(&db, &embedder, "document topic", None, 10).unwrap();
    for pair in results.windows(2) {
        assert!(
            pair[0].score >= pair[1].score,
            "results should be sorted descending by score"
        );
    }
}

// ── Size parameter limits results ──────────────────────────────

#[test]
fn size_parameter_limits_results() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    for i in 0..20 {
        let doc = make_doc(&format!("l{i}"), &format!("Content entry number {i} in the kb"));
        ingest_doc(&db, &embedder, &doc);
    }

    let results = hybrid_search(&db, &embedder, "content entry", None, 5).unwrap();
    assert!(results.len() <= 5);
}

// ── Empty search returns empty ─────────────────────────────────

#[test]
fn empty_kb_search_returns_empty() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = MockEmbedder::new(1024);

    let results = hybrid_search(&db, &embedder, "anything", None, 10).unwrap();
    assert!(results.is_empty());
}

// ── JSON output contract ───────────────────────────────────────

#[test]
fn search_result_serializes_correctly() {
    let db = KbDatabase::open_in_memory().unwrap();
    let embedder = DeterministicEmbedder::new(1024);

    let mut doc = make_doc("j1", "JSON serialization test document content");
    doc.metadata = vec![MetadataEntry {
        key: "format".into(),
        value: "json".into(),
    }];
    ingest_doc(&db, &embedder, &doc);

    let results = hybrid_search(&db, &embedder, "JSON test", None, 10).unwrap();
    assert!(!results.is_empty());

    let json = serde_json::to_value(&results[0]).unwrap();
    assert!(json.get("document_id").is_some());
    assert!(json.get("content").is_some());
    assert!(json.get("chunk").is_some());
    assert!(json.get("metadata").is_some());
    assert!(json.get("score").is_some());
    assert!(json.get("source_type").is_some());
}
