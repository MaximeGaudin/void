//! KB command wiring: opening the store database, embedder selection, and CLI argument parsing helpers.

use void_core::config::{self, VoidConfig};
use void_kb::db::KbDatabase;
use void_kb::embedding::{Embedder, MockEmbedder};
use void_kb::models::MetadataEntry;

pub(super) fn open_kb_db() -> anyhow::Result<KbDatabase> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load_or_default(&config_path);
    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;
    let kb_path = store_path.join("kb.db");
    KbDatabase::open(&kb_path)
}

pub(super) fn build_embedder() -> anyhow::Result<Box<dyn Embedder>> {
    // Note: embeddings use the void-kb embedder pipeline; swap in a different embedder here when wired.
    // For now, use MockEmbedder so the full pipeline is testable end-to-end.
    Ok(Box::new(MockEmbedder::new(1024)))
}

pub(super) fn parse_metadata(raw: &[String]) -> anyhow::Result<Vec<MetadataEntry>> {
    let mut entries = Vec::new();
    for item in raw {
        let (key, value) = item.split_once(':').ok_or_else(|| {
            anyhow::anyhow!("Invalid metadata format: \"{item}\". Expected KEY:VALUE")
        })?;
        let key = key.trim();
        let value = value.trim();
        anyhow::ensure!(
            !key.is_empty(),
            "Metadata key cannot be empty in \"{item}\""
        );
        anyhow::ensure!(
            !value.is_empty(),
            "Metadata value cannot be empty in \"{item}\""
        );
        entries.push(MetadataEntry {
            key: key.to_string(),
            value: value.to_string(),
        });
    }
    Ok(entries)
}

pub(super) fn validate_expiration(exp: Option<&str>) -> anyhow::Result<Option<String>> {
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
