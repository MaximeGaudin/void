//! Conversation ignore/mute patterns stored in config.toml.

/// Returns true when a conversation matches any ignore pattern (case-insensitive
/// substring match on name or external ID).
pub fn conversation_matches_ignore(
    name: Option<&str>,
    external_id: &str,
    patterns: &[String],
) -> bool {
    patterns.iter().any(|pattern| {
        let pattern = pattern.to_lowercase();
        external_id.to_lowercase().contains(&pattern)
            || name.is_some_and(|n| n.to_lowercase().contains(&pattern))
    })
}
