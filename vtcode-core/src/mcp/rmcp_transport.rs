/// RMCP transport layer wrappers for VTCode
///
/// This module provides wrappers around rmcp's transport types to integrate
/// with VTCode's configuration and error handling.

use anyhow::{anyhow, Context, Result};
use rmcp::transport::TokioChildProcess;
use tokio::process::Command;

use vtcode_config::mcp::{McpTransportConfig, McpStdioServerConfig};

/// Create a stdio-based transport from configuration
///
/// # Arguments
/// * `stdio_config` - Stdio server configuration with command and args
/// * `env` - Environment variables to pass to the process
///
/// # Returns
/// A TokioChildProcess transport ready to use with RMCP client
pub fn create_stdio_transport(
    stdio_config: &McpStdioServerConfig,
    env: &std::collections::HashMap<String, String>,
) -> Result<TokioChildProcess> {
    let mut cmd = Command::new(&stdio_config.command);

    // Add arguments
    cmd.args(&stdio_config.args);

    // Set working directory if specified
    if let Some(working_dir) = &stdio_config.working_directory {
        cmd.current_dir(working_dir);
    }

    // Configure environment variables
    for (key, value) in env {
        cmd.env(key, value);
    }

    // Create the child process transport
    TokioChildProcess::new(cmd)
        .context("Failed to create child process for MCP server")
}

/// Create transport from MCP provider configuration
///
/// Phase 1 supports stdio transport only. HTTP transport will be added in Phase 2.
pub fn create_transport_from_config(
    transport_config: &McpTransportConfig,
    env: &std::collections::HashMap<String, String>,
) -> Result<TokioChildProcess> {
    match transport_config {
        McpTransportConfig::Stdio(stdio_config) => create_stdio_transport(stdio_config, env),
        McpTransportConfig::Http(http_config) => {
            Err(anyhow!(
                "HTTP transport not yet supported in Phase 1. Endpoint: {}",
                http_config.endpoint
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_creation() {
        // Test transport creation with configuration
        // Detailed tests in integration tests
    }
}
