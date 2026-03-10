use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Create default config file
    Init,
    /// Show current configuration
    Show,
    /// Open config in $EDITOR
    Edit,
    /// Print config file path
    Path,
}

pub fn run(args: &ConfigArgs) -> anyhow::Result<()> {
    match &args.command {
        ConfigCommand::Init => eprintln!("void config init: not yet implemented"),
        ConfigCommand::Show => eprintln!("void config show: not yet implemented"),
        ConfigCommand::Edit => eprintln!("void config edit: not yet implemented"),
        ConfigCommand::Path => eprintln!("void config path: not yet implemented"),
    }
    Ok(())
}
