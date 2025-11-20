/// RMCP transport layer wrappers for VTCode
///
/// This module provides wrappers around rmcp's transport types to integrate
/// with VTCode's configuration and error handling.

use anyhow::{anyhow, Context, Result};
use rmcp::transport::TokioChildProcess;
use std::ffi::OsString;
use std::path::PathBuf;
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

/// Create a stdio transport from individual parameters (Phase 2 integration)
///
/// This is a convenience wrapper for use within RmcpClient where transport
/// parameters come from different configuration sources.
///
/// # Arguments
/// * `program` - Path to the executable
/// * `args` - Command arguments
/// * `working_dir` - Working directory (optional)
/// * `env` - Environment variables to pass
///
/// # Returns
/// A tuple of (TokioChildProcess transport, stderr reader)
/// The stderr reader can be passed to async logging tasks
pub fn create_stdio_transport_with_stderr(
    program: &OsString,
    args: &[OsString],
    working_dir: Option<&PathBuf>,
    env: &std::collections::HashMap<String, String>,
) -> Result<(TokioChildProcess, Option<tokio::process::ChildStderr>)> {
    let mut cmd = Command::new(program);

    cmd.kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_clear();

    // Add all environment variables
    for (key, value) in env {
        cmd.env(key, value);
    }

    // Set working directory if provided
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    // Add command arguments
    cmd.args(args);

    // Create transport with stderr capture for logging
    let builder = TokioChildProcess::builder(cmd);
    builder
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to create stdio transport with stderr capture")
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
