//! Shared business logic for CLI commands and the upcoming MCP server.

pub mod reads;
pub mod writes;

use serde_json::Value;

/// Render a JSON envelope the same way CLI commands print to stdout.
pub fn render(value: &Value) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}
