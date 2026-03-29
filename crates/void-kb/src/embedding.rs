/// Trait for embedding text into vectors, allowing mock implementations in tests.
pub trait Embedder: Send + Sync {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>>;
    fn dimension(&self) -> usize;
}

/// Mock embedder for tests: returns zero vectors of fixed dimension.
pub struct MockEmbedder {
    dim: usize,
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for MockEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0f32; self.dim]).collect())
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Deterministic embedder for testing: produces a unique vector per text based on char sums.
pub struct DeterministicEmbedder {
    dim: usize,
}

impl DeterministicEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for DeterministicEmbedder {
    fn embed(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = vec![0.0f32; self.dim];
                for (i, c) in t.chars().enumerate() {
                    v[i % self.dim] += c as u32 as f32 / 10000.0;
                }
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    v.iter_mut().for_each(|x| *x /= norm);
                }
                v
            })
            .collect())
    }

    fn dimension(&self) -> usize {
        self.dim
    }
}

/// Query instruction prefix for Qwen3 embedding retrieval.
pub const QUERY_INSTRUCTION: &str =
    "Instruct: Given a user query, retrieve relevant passages from the knowledge base\nQuery:";

/// Format a user query with the retrieval instruction prefix.
pub fn format_query(user_query: &str) -> String {
    format!("{QUERY_INSTRUCTION}{user_query}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_embedder_dimension() {
        let e = MockEmbedder::new(1024);
        assert_eq!(e.dimension(), 1024);
    }

    #[test]
    fn mock_embedder_output_shape() {
        let e = MockEmbedder::new(128);
        let result = e.embed(&["hello", "world"]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 128);
    }

    #[test]
    fn deterministic_embedder_consistent() {
        let e = DeterministicEmbedder::new(64);
        let a = e.embed(&["hello"]).unwrap();
        let b = e.embed(&["hello"]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_embedder_different_texts_different_vectors() {
        let e = DeterministicEmbedder::new(64);
        let a = e.embed(&["hello"]).unwrap();
        let b = e.embed(&["world"]).unwrap();
        assert_ne!(a[0], b[0]);
    }

    #[test]
    fn deterministic_embedder_normalized() {
        let e = DeterministicEmbedder::new(64);
        let result = e.embed(&["test text"]).unwrap();
        let norm: f32 = result[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn format_query_prefix() {
        let q = format_query("what is rust?");
        assert!(q.starts_with("Instruct:"));
        assert!(q.contains("what is rust?"));
    }
}
