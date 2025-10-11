use anyhow::Result;

/// Delegate MCP management commands to the core implementation.
pub async fn handle_mcp_command(
    command: vtcode_core::cli::mcp_commands::McpCommands,
) -> Result<()> {
    vtcode_core::cli::mcp_commands::handle_mcp_command(command).await
}
