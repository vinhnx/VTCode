/// RMCP transport layer wrappers for VT Code
///
/// This module provides wrappers around rmcp's transport types to integrate
/// with VT Code's configuration and error handling.
use anyhow::{Context, Result};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::TokioChildProcess;
use rmcp_reqwest::header::HeaderMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use tokio::process::Command;

use vtcode_config::mcp::McpStdioServerConfig;

/// Type alias for HTTP transport
pub type HttpTransport = StreamableHttpClientTransport<rmcp_reqwest::Client>;

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
    env: &hashbrown::HashMap<OsString, OsString>,
) -> Result<TokioChildProcess> {
    let mut cmd = Command::new(&stdio_config.command);

    // Add arguments
    let _command = cmd.args(&stdio_config.args);

    // Set working directory if specified
    if let Some(working_dir) = &stdio_config.working_directory {
        let _command = cmd.current_dir(working_dir);
    }

    // Match `create_stdio_transport_with_stderr`: the child receives the
    // caller-resolved environment only, never vtcode's full process environment.
    let _command = cmd.env_clear();

    // Configure environment variables
    for (key, value) in env {
        let _command = cmd.env(key, value);
    }

    // Create the child process transport
    TokioChildProcess::new(cmd).context("Failed to create child process for MCP server")
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
///
/// Borrows the executable and optional directory only while configuring the child.
/// The child receives exactly `env`, without inheriting the parent environment.
///
/// # Errors
/// Returns the spawn failure with transport context if the child cannot be started.
pub fn create_stdio_transport_with_stderr(
    program: &OsStr,
    args: &[OsString],
    working_dir: Option<&Path>,
    env: &hashbrown::HashMap<OsString, OsString>,
) -> Result<(TokioChildProcess, Option<tokio::process::ChildStderr>)> {
    let mut cmd = Command::new(program);

    let _command = cmd
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_clear();

    // Add all environment variables
    for (key, value) in env {
        let _command = cmd.env(key, value);
    }

    // Set working directory if provided
    if let Some(dir) = working_dir {
        let _command = cmd.current_dir(dir);
    }

    // Add command arguments
    let _command = cmd.args(args);

    // Create transport with stderr capture for logging
    let builder = TokioChildProcess::builder(cmd);
    builder
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to create stdio transport with stderr capture")
}

/// Create an HTTP-based transport from endpoint URL (Phase 3.2)
///
/// # Arguments
/// * `endpoint` - HTTP endpoint URL (e.g., `https://api.example.com/mcp`)
/// * `bearer_token` - Optional bearer token for authentication
/// * `headers` - Custom HTTP headers to include in requests
///
/// # Returns
/// A StreamableHttpClientTransport ready to use with RMCP client
///
/// # Note
/// This is a convenience wrapper for HTTP transport creation. The actual
/// transport construction delegates to rmcp's StreamableHttpClientTransport
/// following the pattern used in RmcpClient::new_streamable_http_client().
///
/// # Example
/// ```ignore
/// let transport = create_http_transport(
///     "https://api.example.com/mcp",
///     Some("auth_token"),
///     &HeaderMap::new()
/// )?;
/// ```
pub fn create_http_transport(
    _endpoint: &str,
    _bearer_token: Option<&str>,
    _headers: &HeaderMap,
) -> Result<HttpTransport> {
    // Phase 3.2: HTTP transport wrapper
    // NOTE: Full implementation requires direct use of rmcp APIs
    // See RmcpClient::new_streamable_http_client() for reference implementation
    // This function provides the interface; actual HTTP transport is created via:
    // StreamableHttpClientTransport::with_client(http_client, config)

    anyhow::bail!(
        "HTTP transport creation requires rmcp's StreamableHttpClientTransport. \
         Use RmcpClient::new_streamable_http_client() for full implementation."
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn child_dump_waits_for_completed_output() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("delayed-dump.txt");
        tokio::fs::write(&dump, "partial").await.unwrap();
        let args = [
            OsString::from("-c"),
            OsString::from("/bin/sleep 0.2; printf '%s' 'complete-output' > \"$MCP_DUMP\""),
        ];
        let env = hashbrown::HashMap::from_iter([(OsString::from("MCP_DUMP"), dump.as_os_str().to_owned())]);
        let (transport, _stderr) =
            create_stdio_transport_with_stderr(OsStr::new("/bin/sh"), &args, None, &env).unwrap();
        let contents = child_env_dump(transport, &dump).await.unwrap();
        assert_eq!(contents, "complete-output");
    }

    #[tokio::test]
    async fn stdio_transport_accepts_borrowed_executable_and_optional_directory() {
        let dir = tempfile::tempdir().unwrap();
        let args = [
            OsString::from("-c"),
            OsString::from("printf '%s' \"$PWD\" > \"$MCP_DUMP\""),
        ];
        for working_dir in [None, Some(dir.path())] {
            let dump = dir.path().join(if working_dir.is_some() {
                "explicit-cwd.txt"
            } else {
                "default-cwd.txt"
            });
            let env = hashbrown::HashMap::from_iter([(OsString::from("MCP_DUMP"), dump.as_os_str().to_owned())]);
            let (transport, _stderr) =
                create_stdio_transport_with_stderr(OsStr::new("/bin/sh"), &args, working_dir, &env).unwrap();
            let contents = child_env_dump(transport, &dump).await.unwrap();
            let expected = working_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap());
            assert_eq!(
                vtcode_commons::canonicalize(contents).unwrap(),
                vtcode_commons::canonicalize(expected).unwrap()
            );
        }
    }

    /// MCP stdio children must receive the caller-resolved environment only. Before
    /// this was pinned, this transport inherited vtcode's whole process environment
    /// while its `_with_stderr` sibling already cleared it.
    #[tokio::test]
    async fn stdio_transport_child_receives_only_the_resolved_environment() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("child-env.txt");
        let process_has_home = std::env::var_os("HOME").is_some();

        let config = McpStdioServerConfig {
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "env > \"$MCP_DUMP\"".to_string()],
            working_directory: None,
        };
        let mut env = hashbrown::HashMap::new();
        drop(env.insert(
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/bin:/bin")),
        ));
        drop(env.insert(OsString::from("MCP_MARKER"), OsString::from("marker-value")));
        drop(env.insert(OsString::from("MCP_DUMP"), dump.as_os_str().to_os_string()));
        // HOME is present in this process but deliberately absent from the map, so
        // inheritance stays observable in the dump below.
        assert!(!env.contains_key(&OsString::from("HOME")));

        let transport = create_stdio_transport(&config, &env).unwrap();
        let contents = child_env_dump(transport, &dump).await.unwrap();

        assert!(contents.contains("MCP_MARKER=marker-value"), "resolved map not applied: {contents}");
        assert!(contents.lines().any(|line| line.starts_with("PATH=")), "resolved PATH not applied: {contents}");
        if process_has_home {
            assert!(
                !contents.lines().any(|line| line.starts_with("HOME=")),
                "MCP stdio child inherited the process environment: {contents}"
            );
        }
    }

    /// Reap the child before reading a dump that may exist before writing finishes.
    async fn child_env_dump(transport: TokioChildProcess, path: &Path) -> Result<String> {
        let timeout = std::time::Duration::from_secs(5);
        let mut child = transport.into_inner().context("transport must own its child")?;
        let status = match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => status.context("wait for dump child")?,
            Err(error) => {
                tokio::time::timeout(timeout, Box::into_pin(child.kill()))
                    .await
                    .context("dump child cleanup timed out")?
                    .context("kill and reap dump child")?;
                return Err(error).context("MCP dump child did not exit before its deadline");
            }
        };
        anyhow::ensure!(status.success(), "MCP dump child failed: {status}");
        tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read completed child dump {}", path.display()))
    }
}
