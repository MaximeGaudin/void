use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct AccountsArgs {
    #[command(subcommand)]
    pub command: AccountsCommand,
}

#[derive(Debug, Subcommand)]
pub enum AccountsCommand {
    /// List configured accounts
    List,
    /// Add a new account
    Add {
        /// Connector type: whatsapp, slack, gmail, calendar
        connector_type: String,
        /// Account name
        name: Option<String>,
    },
    /// Remove an account
    Remove {
        /// Account ID to remove
        id: String,
    },
}

pub fn run(args: &AccountsArgs) -> anyhow::Result<()> {
    match &args.command {
        AccountsCommand::List => eprintln!("void accounts list: not yet implemented"),
        AccountsCommand::Add {
            connector_type,
            name,
        } => {
            eprintln!(
                "void accounts add {}{}: not yet implemented",
                connector_type,
                name.as_deref().map(|n| format!(" {n}")).unwrap_or_default()
            );
        }
        AccountsCommand::Remove { id } => {
            eprintln!("void accounts remove {id}: not yet implemented");
        }
    }
    Ok(())
}
