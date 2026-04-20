use super::{fts5_escape, KbDatabase};
use crate::models::*;

fn test_db() -> KbDatabase {
    KbDatabase::open_in_memory().unwrap()
}

fn make_doc(id: &str, content: &str) -> Document {
    let now = chrono::Utc::now().to_rfc3339();
    Document {
        id: id.to_string(),
        content: content.to_string(),
        source_type: SourceType::Text,
        source_path: None,
        content_hash: "abc123".to_string(),
        expiration: None,
        source_mtime: None,
        created_at: now.clone(),
        updated_at: now,
        metadata: vec![],
    }
}

#[test]
fn insert_and_get_document() {
    let db = test_db();
    let doc = make_doc("d1", "hello world");
    db.insert_document(&doc).unwrap();

    let fetched = db.get_document("d1").unwrap().unwrap();
    assert_eq!(fetched.id, "d1");
    assert_eq!(fetched.content, "hello world");
}

#[test]
fn get_missing_document_returns_none() {
    let db = test_db();
    assert!(db.get_document("nonexistent").unwrap().is_none());
}

#[test]
fn insert_with_metadata() {
    let db = test_db();
    let mut doc = make_doc("d2", "with metadata");
    doc.metadata = vec![
        MetadataEntry {
            key: "author".into(),
            value: "Alice".into(),
        },
        MetadataEntry {
            key: "tag".into(),
            value: "test".into(),
        },
    ];
    db.insert_document(&doc).unwrap();

    let fetched = db.get_document("d2").unwrap().unwrap();
    assert_eq!(fetched.metadata.len(), 2);
    assert_eq!(fetched.metadata[0].key, "author");
}

#[test]
fn delete_document_cascades() {
    let db = test_db();
    let doc = make_doc("d3", "to delete");
    db.insert_document(&doc).unwrap();

    let embedding = vec![0.0f32; 1024];
    db.insert_chunks_with_embeddings("d3", &[("chunk one".to_string(), 0, 0, 9)], &[embedding])
        .unwrap();

    assert!(db.delete_document("d3").unwrap());
    assert!(db.get_document("d3").unwrap().is_none());
}

#[test]
fn delete_missing_returns_false() {
    let db = test_db();
    assert!(!db.delete_document("nope").unwrap());
}

#[test]
fn list_documents_paginated() {
    let db = test_db();
    for i in 0..5 {
        db.insert_document(&make_doc(&format!("d{i}"), &format!("content {i}")))
            .unwrap();
    }

    let (docs, total) = db.list_documents(2, 0).unwrap();
    assert_eq!(total, 5);
    assert_eq!(docs.len(), 2);

    let (docs2, _) = db.list_documents(2, 2).unwrap();
    assert_eq!(docs2.len(), 2);
}

#[test]
fn insert_chunks_and_semantic_search() {
    let db = test_db();
    let doc = make_doc("d4", "semantic test");
    db.insert_document(&doc).unwrap();

    let mut emb = vec![0.0f32; 1024];
    emb[0] = 1.0;
    db.insert_chunks_with_embeddings(
        "d4",
        &[("hello semantic".to_string(), 0, 0, 14)],
        std::slice::from_ref(&emb),
    )
    .unwrap();

    let results = db.semantic_search(&emb, 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "d4");
}

#[test]
fn insert_chunks_and_grep_search() {
    let db = test_db();
    let doc = make_doc("d5", "grep test");
    db.insert_document(&doc).unwrap();

    let emb = vec![0.0f32; 1024];
    db.insert_chunks_with_embeddings(
        "d5",
        &[("The quick brown fox jumps".to_string(), 0, 0, 25)],
        &[emb],
    )
    .unwrap();

    let results = db.grep_search("quick brown", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "d5");
}

#[test]
fn grep_search_accent_insensitive() {
    let db = test_db();
    let doc = make_doc("d6", "accent test");
    db.insert_document(&doc).unwrap();

    let emb = vec![0.0f32; 1024];
    db.insert_chunks_with_embeddings(
        "d6",
        &[("Le café est délicieux".to_string(), 0, 0, 24)],
        &[emb],
    )
    .unwrap();

    let results = db.grep_search("cafe", 10).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].1, "d6");
}

#[test]
fn expiration_cleanup() {
    let db = test_db();
    let mut doc = make_doc("expired", "old content");
    doc.expiration = Some("2000-01-01T00:00:00Z".to_string());
    db.insert_document(&doc).unwrap();

    let mut doc2 = make_doc("fresh", "new content");
    doc2.expiration = Some("2099-01-01T00:00:00Z".to_string());
    db.insert_document(&doc2).unwrap();

    let cleaned = db.cleanup_expired().unwrap();
    assert_eq!(cleaned, 1);
    assert!(db.get_document("expired").unwrap().is_none());
    assert!(db.get_document("fresh").unwrap().is_some());
}

#[test]
fn sync_folder_crud() {
    let db = test_db();
    let now = chrono::Utc::now().to_rfc3339();
    let folder = SyncFolder {
        id: "sf1".into(),
        folder_path: "/tmp/docs".into(),
        interval_secs: 60,
        last_scan_at: None,
        created_at: now,
    };
    db.upsert_sync_folder(&folder).unwrap();

    let folders = db.list_sync_folders().unwrap();
    assert_eq!(folders.len(), 1);
    assert_eq!(folders[0].folder_path, "/tmp/docs");

    db.update_sync_folder_scan_time("/tmp/docs", "2025-01-01T00:00:00Z")
        .unwrap();
    let folders = db.list_sync_folders().unwrap();
    assert_eq!(
        folders[0].last_scan_at.as_deref(),
        Some("2025-01-01T00:00:00Z")
    );
}

#[test]
fn status_counts() {
    let db = test_db();
    let status = db.status().unwrap();
    assert_eq!(status.document_count, 0);

    db.insert_document(&make_doc("s1", "a")).unwrap();
    db.insert_document(&make_doc("s2", "b")).unwrap();

    let emb = vec![0.0f32; 1024];
    db.insert_chunks_with_embeddings(
        "s1",
        &[("chunk".into(), 0, 0, 5)],
        std::slice::from_ref(&emb),
    )
    .unwrap();

    let status = db.status().unwrap();
    assert_eq!(status.document_count, 2);
    assert_eq!(status.chunk_count, 1);
}

#[test]
fn duplicate_document_id_rejected() {
    let db = test_db();
    let doc = make_doc("dup", "first");
    db.insert_document(&doc).unwrap();
    assert!(db.insert_document(&doc).is_err());
}

#[test]
fn fts5_escape_basic() {
    assert_eq!(fts5_escape("hello world"), "\"hello\" \"world\"");
    assert_eq!(fts5_escape(""), "\"\"");
    assert_eq!(fts5_escape("a\"b"), "\"a\"\"b\"");
}

#[test]
fn find_by_source_path() {
    let db = test_db();
    let mut doc = make_doc("fp1", "file content");
    doc.source_type = SourceType::File;
    doc.source_path = Some("/tmp/test.txt".into());
    db.insert_document(&doc).unwrap();

    let found = db.find_document_by_source_path("/tmp/test.txt").unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, "fp1");

    assert!(db
        .find_document_by_source_path("/tmp/nope.txt")
        .unwrap()
        .is_none());
}

#[test]
fn remove_sync_folder_deletes_docs_and_registration() {
    let db = test_db();

    let now = chrono::Utc::now().to_rfc3339();
    let folder = SyncFolder {
        id: "sf-rm".into(),
        folder_path: "/data/docs".into(),
        interval_secs: 60,
        last_scan_at: None,
        created_at: now.clone(),
    };
    db.upsert_sync_folder(&folder).unwrap();

    let mut d1 = make_doc("sd1", "synced content A");
    d1.source_type = SourceType::Synced;
    d1.source_path = Some("/data/docs/a.txt".into());
    db.insert_document(&d1).unwrap();

    let emb = vec![0.0f32; 1024];
    db.insert_chunks_with_embeddings(
        "sd1",
        &[("chunk a".into(), 0, 0, 7)],
        std::slice::from_ref(&emb),
    )
    .unwrap();

    let mut d2 = make_doc("sd2", "synced content B");
    d2.source_type = SourceType::Synced;
    d2.source_path = Some("/data/docs/sub/b.md".into());
    db.insert_document(&d2).unwrap();
    db.insert_chunks_with_embeddings(
        "sd2",
        &[("chunk b".into(), 0, 0, 7)],
        std::slice::from_ref(&emb),
    )
    .unwrap();

    // Unrelated document that should survive
    let mut d3 = make_doc("other", "unrelated");
    d3.source_type = SourceType::Synced;
    d3.source_path = Some("/other/folder/x.txt".into());
    db.insert_document(&d3).unwrap();

    let removed = db.remove_sync_folder("/data/docs").unwrap();
    assert_eq!(removed, 2);

    assert!(db.get_document("sd1").unwrap().is_none());
    assert!(db.get_document("sd2").unwrap().is_none());
    assert!(db.get_document("other").unwrap().is_some());
    assert!(db.list_sync_folders().unwrap().is_empty());

    let status = db.status().unwrap();
    assert_eq!(status.sync_folder_count, 0);
}

#[test]
fn remove_sync_folder_nonexistent_returns_zero() {
    let db = test_db();
    let removed = db.remove_sync_folder("/no/such/folder").unwrap();
    assert_eq!(removed, 0);
}
