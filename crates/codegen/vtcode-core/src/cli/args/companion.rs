use clap::{Args, Subcommand, ValueHint};
use std::path::PathBuf;

/// Manage a user's local VT Code Companion launch context.
#[derive(Debug, Clone, Subcommand)]
pub enum CompanionCommand {
    /// Select the VT Code executable and workspace used by the local plugin.
    Setup(CompanionSetupArgs),
}

/// Machine-specific Companion setup values.
#[derive(Debug, Clone, Args)]
pub struct CompanionSetupArgs {
    /// VT Code executable; defaults to the executable running this command.
    #[arg(long, value_name = "PATH", value_hint = ValueHint::FilePath)]
    pub binary: Option<PathBuf>,
}
