use std::path::PathBuf;

use clap::{Args, Subcommand};

#[derive(Debug, Args)]
pub struct KbArgs {
    #[command(subcommand)]
    pub command: KbCommand,
}

#[derive(Debug, Subcommand)]
pub enum KbCommand {
    /// Add content to the knowledge base (text or file)
    Add(AddArgs),
    /// Search the knowledge base
    Search(SearchArgs),
    /// Register and sync a folder with the knowledge base
    Sync(KbSyncArgs),
    /// Stop syncing a folder and remove all its indexed documents
    Unsync(UnsyncArgs),
    /// List all documents in the knowledge base
    List(ListArgs),
    /// Remove a document from the knowledge base
    Remove(RemoveArgs),
    /// Show knowledge base status and statistics
    Status,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Text content to add (mutually exclusive with --file)
    pub content: Option<String>,

    /// Path to a file to add (mutually exclusive with positional content)
    #[arg(long, conflicts_with = "content")]
    pub file: Option<PathBuf>,

    /// Metadata in KEY:VALUE format (repeatable)
    #[arg(long = "metadata", value_name = "KEY:VALUE")]
    pub metadata: Vec<String>,

    /// Expiration date in ISO 8601 / RFC 3339 format
    #[arg(long)]
    pub expiration: Option<String>,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Semantic search query (required)
    #[arg(long)]
    pub semantic_query: String,

    /// Grep term for lexical matching (optional)
    #[arg(long)]
    pub grep: Option<String>,

    /// Number of results to return
    #[arg(long, default_value = "10")]
    pub size: usize,
}

#[derive(Debug, Args)]
pub struct KbSyncArgs {
    /// Path to the folder to sync
    pub folder_path: String,
}

#[derive(Debug, Args)]
pub struct UnsyncArgs {
    /// Path to the folder to stop syncing
    pub folder_path: String,
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Number of results per page
    #[arg(long, short = 'n', default_value = "50")]
    pub size: i64,

    /// Page number (1-based)
    #[arg(long, default_value = "1")]
    pub page: i64,
}

#[derive(Debug, Args)]
pub struct RemoveArgs {
    /// Document ID to remove
    pub doc_id: String,
}
