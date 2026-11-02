//! CLI commands for managing Model Context Protocol providers.

mod args;
mod auth;
mod config_store;
mod providers;
mod resources;

use anyhow::Result;
pub use args::{
    AddArgs, AddMcpStdioArgs, AddMcpStreamableHttpArgs, AddMcpTransportArgs, AppArgs, GetArgs, ListArgs, LoginArgs,
    LogoutArgs, RemoveArgs, ResourcesArgs,
};
use clap::Subcommand;
pub use config_store::set_global_config_path_override_for_tests;
use std::path::Path;

/// Subcommands exposed by the `vtcode mcp` entrypoint.
#[derive(Debug, Clone, Subcommand)]
pub enum McpCommands {
    /// List configured MCP providers.
    List(ListArgs),

    /// Show details for a single MCP provider.
    Get(GetArgs),

    /// Add or update an MCP provider definition.
    Add(AddArgs),

    /// Remove an MCP provider definition.
    Remove(RemoveArgs),

    /// Start OAuth login for an HTTP MCP provider.
    Login(LoginArgs),

    /// Clear stored OAuth credentials for an HTTP MCP provider.
    Logout(LogoutArgs),

    /// List resources exposed by connected MCP providers.
    Resources(ResourcesArgs),

    /// Inspect an MCP resource. Deep view for MCP Apps `ui://` resources.
    App(AppArgs),
}

/// Entry point for the `vtcode mcp` command group.
pub async fn handle_mcp_command(command: McpCommands, workspace: &Path) -> Result<()> {
    match command {
        McpCommands::List(args) => providers::run_list(args).await,
        McpCommands::Get(args) => providers::run_get(args).await,
        McpCommands::Add(args) => providers::run_add(args).await,
        McpCommands::Remove(args) => providers::run_remove(args).await,
        McpCommands::Login(args) => auth::run_login(args).await,
        McpCommands::Logout(args) => auth::run_logout(args).await,
        McpCommands::Resources(args) => resources::run_resources(args, workspace).await,
        McpCommands::App(args) => resources::run_app(args, workspace).await,
    }
}
