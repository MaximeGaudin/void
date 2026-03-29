use std::collections::HashMap;

use crate::db::KbDatabase;
use crate::embedding::{format_query, Embedder};
use crate::models::SearchResult;

const RRF_K: f64 = 60.0;
const W_SEMANTIC: f64 = 1.0;
const W_GREP: f64 = 1.5;
const CANDIDATE_POOL: i64 = 100;

#[derive(Debug, Clone)]
struct CandidateChunk {
    #[allow(dead_code)]
    chunk_id: i64,
    document_id: String,
    chunk_content: String,
    semantic_rank: Option<usize>,
    grep_rank: Option<usize>,
    semantic_distance: Option<f64>,
    bm25_score: Option<f64>,
}

/// Run hybrid search: semantic KNN + optional grep FTS5, fused by weighted RRF.
pub fn hybrid_search(
    db: &KbDatabase,
    embedder: &dyn Embedder,
    semantic_query: &str,
    grep_query: Option<&str>,
    size: usize,
) -> anyhow::Result<Vec<SearchResult>> {
    db.cleanup_expired()?;

    let formatted = format_query(semantic_query);
    let query_embedding = embedder.embed(&[formatted.as_str()])?;
    anyhow::ensure!(!query_embedding.is_empty(), "embedding returned empty result");

    let semantic_results = db.semantic_search(&query_embedding[0], CANDIDATE_POOL)?;

    let grep_results = match grep_query {
        Some(q) if !q.trim().is_empty() => db.grep_search(q, CANDIDATE_POOL)?,
        _ => vec![],
    };

    let fused = fuse_results(&semantic_results, &grep_results);

    let mut doc_best: HashMap<String, (f64, CandidateChunk)> = HashMap::new();
    for candidate in &fused {
        let score = rrf_score(candidate);
        let entry = doc_best.entry(candidate.document_id.clone()).or_insert_with(|| {
            (score, candidate.clone())
        });
        if score > entry.0 {
            *entry = (score, candidate.clone());
        }
    }

    let mut ranked: Vec<(f64, CandidateChunk)> = doc_best.into_values().collect();
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let a_dist = a.1.semantic_distance.unwrap_or(f64::MAX);
                let b_dist = b.1.semantic_distance.unwrap_or(f64::MAX);
                a_dist.partial_cmp(&b_dist).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                let a_bm25 = a.1.bm25_score.unwrap_or(f64::MAX);
                let b_bm25 = b.1.bm25_score.unwrap_or(f64::MAX);
                a_bm25.partial_cmp(&b_bm25).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.1.document_id.cmp(&b.1.document_id))
    });

    ranked.truncate(size);

    let expired_ids: Vec<String> = ranked.iter().map(|(_, c)| c.document_id.clone()).collect();
    let expired_set: std::collections::HashSet<String> =
        db.filter_expired_ids(&expired_ids)?.into_iter().collect();

    let mut results = Vec::new();
    for (score, candidate) in ranked {
        if expired_set.contains(&candidate.document_id) {
            continue;
        }
        if let Some(doc) = db.get_document(&candidate.document_id)? {
            let metadata_map: serde_json::Value = doc
                .metadata
                .iter()
                .map(|m| (m.key.clone(), serde_json::Value::String(m.value.clone())))
                .collect::<serde_json::Map<String, serde_json::Value>>()
                .into();

            results.push(SearchResult {
                document_id: doc.id,
                content: doc.content,
                chunk: candidate.chunk_content,
                metadata: metadata_map,
                score,
                source_type: doc.source_type,
                source_path: doc.source_path,
            });
        }
    }

    Ok(results)
}

fn fuse_results(
    semantic: &[(i64, String, String, f64)],
    grep: &[(i64, String, String, f64)],
) -> Vec<CandidateChunk> {
    let mut chunks: HashMap<i64, CandidateChunk> = HashMap::new();

    for (rank, (chunk_id, doc_id, content, distance)) in semantic.iter().enumerate() {
        chunks.insert(*chunk_id, CandidateChunk {
            chunk_id: *chunk_id,
            document_id: doc_id.clone(),
            chunk_content: content.clone(),
            semantic_rank: Some(rank + 1),
            grep_rank: None,
            semantic_distance: Some(*distance),
            bm25_score: None,
        });
    }

    for (rank, (chunk_id, doc_id, content, bm25)) in grep.iter().enumerate() {
        chunks
            .entry(*chunk_id)
            .and_modify(|c| {
                c.grep_rank = Some(rank + 1);
                c.bm25_score = Some(*bm25);
            })
            .or_insert_with(|| CandidateChunk {
                chunk_id: *chunk_id,
                document_id: doc_id.clone(),
                chunk_content: content.clone(),
                semantic_rank: None,
                grep_rank: Some(rank + 1),
                semantic_distance: None,
                bm25_score: Some(*bm25),
            });
    }

    chunks.into_values().collect()
}

fn rrf_score(candidate: &CandidateChunk) -> f64 {
    let sem = candidate
        .semantic_rank
        .map(|r| W_SEMANTIC / (RRF_K + r as f64))
        .unwrap_or(0.0);
    let grep = candidate
        .grep_rank
        .map(|r| W_GREP / (RRF_K + r as f64))
        .unwrap_or(0.0);
    sem + grep
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_score_semantic_only() {
        let c = CandidateChunk {
            chunk_id: 1,
            document_id: "d1".into(),
            chunk_content: "test".into(),
            semantic_rank: Some(1),
            grep_rank: None,
            semantic_distance: Some(0.1),
            bm25_score: None,
        };
        let score = rrf_score(&c);
        let expected = 1.0 / (60.0 + 1.0);
        assert!((score - expected).abs() < 1e-10);
    }

    #[test]
    fn rrf_score_grep_only() {
        let c = CandidateChunk {
            chunk_id: 1,
            document_id: "d1".into(),
            chunk_content: "test".into(),
            semantic_rank: None,
            grep_rank: Some(1),
            semantic_distance: None,
            bm25_score: Some(-5.0),
        };
        let score = rrf_score(&c);
        let expected = 1.5 / (60.0 + 1.0);
        assert!((score - expected).abs() < 1e-10);
    }

    #[test]
    fn rrf_score_both() {
        let c = CandidateChunk {
            chunk_id: 1,
            document_id: "d1".into(),
            chunk_content: "test".into(),
            semantic_rank: Some(1),
            grep_rank: Some(2),
            semantic_distance: Some(0.1),
            bm25_score: Some(-3.0),
        };
        let score = rrf_score(&c);
        let expected = 1.0 / (60.0 + 1.0) + 1.5 / (60.0 + 2.0);
        assert!((score - expected).abs() < 1e-10);
    }

    #[test]
    fn rrf_grep_boost_outranks_semantic() {
        let sem_only = CandidateChunk {
            chunk_id: 1,
            document_id: "d1".into(),
            chunk_content: "a".into(),
            semantic_rank: Some(1),
            grep_rank: None,
            semantic_distance: Some(0.05),
            bm25_score: None,
        };
        let grep_only = CandidateChunk {
            chunk_id: 2,
            document_id: "d2".into(),
            chunk_content: "b".into(),
            semantic_rank: None,
            grep_rank: Some(1),
            semantic_distance: None,
            bm25_score: Some(-10.0),
        };
        assert!(rrf_score(&grep_only) > rrf_score(&sem_only));
    }

    #[test]
    fn rrf_k60_manual_verification() {
        let c = CandidateChunk {
            chunk_id: 1,
            document_id: "d".into(),
            chunk_content: "t".into(),
            semantic_rank: Some(5),
            grep_rank: Some(10),
            semantic_distance: None,
            bm25_score: None,
        };
        let expected = 1.0 / 65.0 + 1.5 / 70.0;
        let score = rrf_score(&c);
        assert!((score - expected).abs() < 1e-10);
    }

    #[test]
    fn fuse_deduplicates_chunks() {
        let semantic = vec![(1i64, "d1".into(), "text".into(), 0.1)];
        let grep = vec![(1i64, "d1".into(), "text".into(), -5.0)];
        let fused = fuse_results(&semantic, &grep);
        assert_eq!(fused.len(), 1);
        assert!(fused[0].semantic_rank.is_some());
        assert!(fused[0].grep_rank.is_some());
    }

    #[test]
    fn fuse_separate_chunks() {
        let semantic = vec![(1i64, "d1".into(), "a".into(), 0.1)];
        let grep = vec![(2i64, "d2".into(), "b".into(), -5.0)];
        let fused = fuse_results(&semantic, &grep);
        assert_eq!(fused.len(), 2);
    }

    #[test]
    fn tie_break_deterministic() {
        let a = CandidateChunk {
            chunk_id: 1,
            document_id: "aaa".into(),
            chunk_content: "x".into(),
            semantic_rank: Some(1),
            grep_rank: None,
            semantic_distance: Some(0.5),
            bm25_score: None,
        };
        let b = CandidateChunk {
            chunk_id: 2,
            document_id: "bbb".into(),
            chunk_content: "y".into(),
            semantic_rank: Some(1),
            grep_rank: None,
            semantic_distance: Some(0.5),
            bm25_score: None,
        };
        assert_eq!(rrf_score(&a), rrf_score(&b));
        assert!(a.document_id < b.document_id);
    }
}
