use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "arus-cli",
    version,
    about = "Inspect sources and collect public data."
)]
pub(crate) struct Cli {
    /// Read the source catalog (default: catalog/sources.json).
    #[arg(long, global = true, default_value = "catalog/sources.json")]
    pub catalog: PathBuf,
    /// Read collection definitions (default: catalog/collections.json).
    #[arg(long, global = true, default_value = "catalog/collections.json")]
    pub collections: PathBuf,
    /// Store acquisition state and raw data (default: data/acquisition).
    #[arg(long, global = true, default_value = "data/acquisition")]
    pub data_dir: PathBuf,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Inspect source metadata and collection definitions.
    Catalog {
        #[command(subcommand)]
        command: CatalogCommand,
    },
    /// Plan, collect, and inspect acquisition state.
    Data {
        #[command(subcommand)]
        command: DataCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CatalogCommand {
    /// List source metadata without fetching data.
    List,
    /// Check catalog and collection definitions without fetching data.
    Check,
    /// Show one source's metadata.
    Show { source_id: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum DataCommand {
    /// Show collection eligibility and refresh decisions without fetching data.
    Plan {
        #[command(flatten)]
        usage: UsageDeclaration,
    },
    /// Show persisted acquisition state without fetching data.
    Status,
    /// Collect selected collections, or enabled collections when none are selected.
    Sync {
        /// Select a collection. Repeat for additional collections.
        #[arg(long = "collection", value_name = "ID")]
        collection_ids: Vec<String>,
        /// Bypass refresh cadence while preserving access and rate limits.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        usage: UsageDeclaration,
    },
    /// Collect due enabled collections once, then exit.
    Refresh {
        #[command(flatten)]
        usage: UsageDeclaration,
    },
}

#[derive(Debug, Args)]
pub(crate) struct UsageDeclaration {
    /// Declare noncommercial use for this invocation only.
    #[arg(long)]
    pub accept_noncommercial: bool,
}
