mod cli;
mod commands;
pub mod connectors;
pub mod context;
pub mod output;

pub(crate) use cli::Command;

fn main() -> anyhow::Result<()> {
    cli::run()
}
