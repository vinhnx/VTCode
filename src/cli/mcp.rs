use anyhow::Result;

/// Delegate MCP management commands to the core implementation.
pub async fn handle_mcp_command(command: vtcode_core::mcp::cli::McpCommands) -> Result<()> {
    vtcode_core::mcp::cli::handle_mcp_command(command).await
}
