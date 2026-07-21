use clap::Args;

#[derive(Debug, Args)]
pub struct McpArgs {}

pub async fn run(_args: &McpArgs) -> anyhow::Result<()> {
    crate::mcp::run_server().await
}
