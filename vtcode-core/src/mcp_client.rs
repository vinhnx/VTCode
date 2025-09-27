//! MCP Client implementation
//!
//! This module provides a high-level abstraction over the rmcp library,
//! managing MCP provider connections and tool execution.

use crate::config::mcp::{McpClientConfig, McpProviderConfig, McpTransportConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::{
    model::{CallToolRequestParam, ListToolsResult},
    transport::TokioChildProcess,
    ServiceExt,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// High-level MCP client that manages multiple providers
pub struct McpClient {
    config: McpClientConfig,
    providers: HashMap<String, Arc<McpProvider>>,
    active_connections: Arc<Mutex<HashMap<String, Arc<RunningMcpService>>>>,
}

impl McpClient {
    /// Create a new MCP client with the given configuration
    pub fn new(config: McpClientConfig) -> Self {
        Self {
            config,
            providers: HashMap::new(),
            active_connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Initialize the MCP client and connect to configured providers
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("MCP client is disabled in configuration");
            return Ok(());
        }

        info!("Initializing MCP client with {} configured providers", self.config.providers.len());

        for provider_config in &self.config.providers {
            if provider_config.enabled {
                info!("Initializing MCP provider '{}'", provider_config.name);

                match McpProvider::new(provider_config.clone()).await {
                    Ok(provider) => {
                        let provider = Arc::new(provider);
                        self.providers.insert(provider_config.name.clone(), provider);
                        info!("Successfully initialized MCP provider '{}'", provider_config.name);
                    }
                    Err(e) => {
                        error!("Failed to initialize MCP provider '{}': {}", provider_config.name, e);
                        // Continue with other providers instead of failing completely
                        continue;
                    }
                }
            } else {
                debug!("MCP provider '{}' is disabled, skipping", provider_config.name);
            }
        }

        info!("MCP client initialization complete. Active providers: {}", self.providers.len());

        // Clean up any providers with terminated processes
        let _ = self.cleanup_dead_providers().await;

        Ok(())
    }

    /// Kill any remaining MCP provider processes that may not have terminated properly
    async fn kill_remaining_mcp_processes(&self) {
        debug!("Checking for remaining MCP provider processes to clean up");

        // Try to find and kill any remaining MCP provider processes
        // This is a fallback for cases where the rmcp library doesn't properly terminate processes
        let process_cleanup_attempts = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            self.attempt_process_cleanup()
        ).await;

        match process_cleanup_attempts {
            Ok(Ok(cleaned_count)) => {
                if cleaned_count > 0 {
                    debug!("Cleaned up {} remaining MCP provider processes", cleaned_count);
                }
            }
            Ok(Err(e)) => {
                debug!("Error during MCP process cleanup (non-critical): {}", e);
            }
            Err(_) => {
                debug!("MCP process cleanup timed out (non-critical)");
            }
        }
    }

    /// Attempt to clean up MCP provider processes by finding and killing them
    async fn attempt_process_cleanup(&self) -> Result<usize> {
        use tokio::process::Command as TokioCommand;

        let mut cleaned_count = 0;

        // Get current process ID to avoid killing ourselves
        let current_pid = std::process::id();

        // Try to find MCP provider processes and kill them
        // This is a best-effort cleanup for processes that may have escaped proper termination
        for provider_config in &self.config.providers {
            if !provider_config.enabled {
                continue;
            }

            let provider_name = &provider_config.name;
            debug!("Attempting cleanup for MCP provider '{}'", provider_name);

            // Use pgrep-like logic to find processes by command name
            match TokioCommand::new("pgrep")
                .args(["-f", &format!("mcp-server-{}", provider_name)])
                .output()
                .await
            {
                Ok(output) if output.status.success() => {
                    let pids = String::from_utf8_lossy(&output.stdout);
                    for pid_str in pids.lines() {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if pid != current_pid && pid > 0 {
                                debug!("Killing MCP provider process {} for '{}'", pid, provider_name);
                                // Try to kill the process gracefully first
                                let _ = TokioCommand::new("kill")
                                    .args(["-TERM", &pid.to_string()])
                                    .output()
                                    .await;

                                // Give it a moment to terminate gracefully
                                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                                // Force kill if still running
                                let _ = TokioCommand::new("kill")
                                    .args(["-KILL", &pid.to_string()])
                                    .output()
                                    .await;

                                cleaned_count += 1;
                            }
                        }
                    }
                }
                _ => {
                    // pgrep not available or command failed, try alternative approach
                    debug!("pgrep not available, trying alternative cleanup for '{}'", provider_name);
                }
            }
        }

        Ok(cleaned_count)
    }


    /// Clean up providers with terminated processes
    pub async fn cleanup_dead_providers(&self) -> Result<()> {
        let mut dead_providers = Vec::new();

        for (provider_name, provider) in &self.providers {
            // Try to check if provider is still alive by attempting a quick operation
            let provider_health_check = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                provider.has_tool("ping")
            ).await;

            match provider_health_check {
                Ok(Ok(_)) => {
                    // Provider is responsive
                    debug!("MCP provider '{}' is healthy", provider_name);
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("No such process") || error_msg.contains("ESRCH") {
                        warn!("MCP provider '{}' has terminated process, marking for cleanup", provider_name);
                        dead_providers.push(provider_name.clone());
                    } else {
                        debug!("MCP provider '{}' returned error but process may be alive: {}", provider_name, e);
                    }
                }
                Err(_timeout) => {
                    warn!("MCP provider '{}' health check timed out, may be unresponsive", provider_name);
                    // Don't mark as dead on timeout, might just be slow
                }
            }
        }

        // Note: In a real implementation, we'd want to remove dead providers from the providers map
        // For now, we'll just log them
        if !dead_providers.is_empty() {
            warn!("Found {} dead MCP providers: {:?}", dead_providers.len(), dead_providers);
        }

        Ok(())
    }

    /// List all available MCP tools across all providers
    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        if !self.config.enabled {
            debug!("MCP client is disabled, returning empty tool list");
            return Ok(Vec::new());
        }

        if self.providers.is_empty() {
            debug!("No MCP providers configured, returning empty tool list");
            return Ok(Vec::new());
        }

        let mut all_tools = Vec::new();
        let mut errors = Vec::new();

        for (provider_name, provider) in &self.providers {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(10),
                provider.list_tools()
            ).await {
                Ok(Ok(tools)) => {
                    debug!("Provider '{}' has {} tools", provider_name, tools.tools.len());

                    for tool in tools.tools {
                        all_tools.push(McpToolInfo {
                            name: tool.name.to_string(),
                            description: tool.description.unwrap_or_default().to_string(),
                            provider: provider_name.clone(),
                            input_schema: serde_json::to_value(&*tool.input_schema).unwrap_or(Value::Null),
                        });
                    }
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("No such process") || error_msg.contains("ESRCH") ||
                       error_msg.contains("EPIPE") || error_msg.contains("Broken pipe") ||
                       error_msg.contains("write EPIPE") {
                        debug!("MCP provider '{}' process/pipe terminated during tool listing (normal during shutdown): {}", provider_name, e);
                    } else {
                        warn!("Failed to list tools for provider '{}': {}", provider_name, e);
                    }
                    let error_msg = format!("Failed to list tools for provider '{}': {}", provider_name, e);
                    errors.push(error_msg);
                }
                Err(_timeout) => {
                    warn!("MCP provider '{}' tool listing timed out", provider_name);
                    let error_msg = format!("Tool listing timeout for provider '{}'", provider_name);
                    errors.push(error_msg);
                }
            }
        }

        if !errors.is_empty() {
            warn!("Encountered {} errors while listing MCP tools: {:?}", errors.len(), errors);
        }

        info!("Found {} total MCP tools across all providers", all_tools.len());
        Ok(all_tools)
    }

    /// Execute a tool call on the appropriate MCP provider
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        if !self.config.enabled {
            return Err(anyhow::anyhow!("MCP client is disabled"));
        }

        if self.providers.is_empty() {
            return Err(anyhow::anyhow!("No MCP providers configured"));
        }

        let tool_name_owned = tool_name.to_string();
        debug!("Executing MCP tool '{}' with args: {}", tool_name, args);

        // Find the provider that has this tool
        let provider_name = {
            let mut found_provider = None;
            let mut provider_errors = Vec::new();

            for (name, provider) in &self.providers {
                match provider.has_tool(&tool_name_owned).await {
                    Ok(true) => {
                        found_provider = Some(name.clone());
                        break;
                    }
                    Ok(false) => continue,
                    Err(e) => {
                        let error_msg = format!("Error checking tool availability for provider '{}': {}", name, e);
                        warn!("{}", error_msg);
                        provider_errors.push(error_msg);
                    }
                }
            }

            found_provider.ok_or_else(|| {
                let error_msg = format!("Tool '{}' not found in any MCP provider. Provider errors: {:?}",
                    tool_name, provider_errors);
                anyhow::anyhow!(error_msg)
            })?
        };

        debug!("Found tool '{}' in provider '{}'", tool_name, provider_name);

        let provider = self.providers.get(&provider_name)
            .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found after discovery", provider_name))?;

        // Get or create connection for this provider
        let connection = match self.get_or_create_connection(provider).await {
            Ok(conn) => conn,
            Err(e) => {
                error!("Failed to establish connection to provider '{}': {}", provider_name, e);
                return Err(e);
            }
        };

        // Execute the tool call
        match connection.call_tool(CallToolRequestParam {
            name: tool_name_owned.into(),
            arguments: args.as_object().cloned(),
        }).await {
            Ok(result) => {
                info!("Successfully executed MCP tool '{}' via provider '{}'", tool_name, provider_name);
                Ok(serde_json::to_value(result).context("Failed to serialize MCP tool result")?)
            }
            Err(e) => {
                error!("MCP tool '{}' failed on provider '{}': {}", tool_name, provider_name, e);

                Err(anyhow::anyhow!("MCP tool execution failed: {}", e))
            }
        }
    }

    /// Get or create a connection to the specified provider
    async fn get_or_create_connection(&self, provider: &McpProvider) -> Result<Arc<RunningMcpService>> {
        let provider_name = &provider.config.name;
        debug!("Getting connection for MCP provider '{}'", provider_name);

        let mut connections = self.active_connections.lock().await;

        if !connections.contains_key(provider_name) {
            debug!("Creating new connection for provider '{}'", provider_name);

            // Add timeout for connection creation
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                provider.connect()
            ).await {
                Ok(Ok(connection)) => {
                    let connection = Arc::new(connection);
                    connections.insert(provider_name.clone(), Arc::clone(&connection));
                    debug!("Successfully created connection for provider '{}'", provider_name);
                    Ok(connection)
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("HTTP MCP server support") {
                        warn!("Provider '{}' uses HTTP transport which is not fully implemented: {}", provider_name, e);
                    } else {
                        error!("Failed to create connection for provider '{}': {}", provider_name, e);
                    }
                    Err(e)
                }
                Err(_timeout) => {
                    error!("Connection creation timed out for provider '{}'", provider_name);
                    Err(anyhow::anyhow!("Connection timeout for provider '{}'", provider_name))
                }
            }
        } else {
            // Validate existing connection is still healthy
            let existing_connection = connections.get(provider_name).unwrap().clone();

            // Quick health check - try to use the connection
            if let Err(e) = self.validate_connection(provider_name, &existing_connection).await {
                debug!("Existing connection for provider '{}' is unhealthy, creating new one: {}", provider_name, e);

                // Remove the unhealthy connection
                connections.remove(provider_name);

                // Create new connection
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(30),
                    provider.connect()
                ).await {
                    Ok(Ok(connection)) => {
                        let connection = Arc::new(connection);
                        connections.insert(provider_name.clone(), Arc::clone(&connection));
                        debug!("Successfully created new connection for provider '{}'", provider_name);
                        Ok(connection)
                    }
                    Ok(Err(e)) => {
                        error!("Failed to create replacement connection for provider '{}': {}", provider_name, e);
                        Err(e)
                    }
                    Err(_timeout) => {
                        error!("Replacement connection creation timed out for provider '{}'", provider_name);
                        Err(anyhow::anyhow!("Replacement connection timeout for provider '{}'", provider_name))
                    }
                }
            } else {
                debug!("Reusing existing healthy connection for provider '{}'", provider_name);
                Ok(existing_connection)
            }
        }
    }

    /// Validate that an existing connection is still healthy
    async fn validate_connection(&self, provider_name: &str, _connection: &RunningMcpService) -> Result<()> {
        // For now, we'll assume the connection is healthy if it exists
        // A full implementation would ping the server or check connection status
        debug!("Validating connection health for provider '{}'", provider_name);
        Ok(())
    }

    /// Shutdown all MCP connections
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down MCP client and all provider connections");

        let mut connections = self.active_connections.lock().await;

        if connections.is_empty() {
            info!("No active MCP connections to shutdown");
            return Ok(());
        }

        info!("Shutting down {} MCP provider connections", connections.len());

        let cancellation_tokens: Vec<(String, rmcp::service::RunningServiceCancellationToken)> =
            connections
                .iter()
                .map(|(provider_name, connection)| {
                    debug!(
                        "Initiating graceful shutdown for MCP provider '{}'",
                        provider_name
                    );
                    (
                        provider_name.clone(),
                        connection.cancellation_token(),
                    )
                })
                .collect();

        for (provider_name, token) in cancellation_tokens {
            debug!(
                "Cancelling MCP provider '{}' via cancellation token",
                provider_name
            );
            token.cancel();
        }

        // Give connections a grace period to shutdown cleanly
        let shutdown_timeout = tokio::time::Duration::from_secs(5);
        let shutdown_start = std::time::Instant::now();

        // Wait for graceful shutdown or timeout
        while shutdown_start.elapsed() < shutdown_timeout && !connections.is_empty() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            // Remove any connections that have been dropped
            connections.retain(|_, connection| {
                // Check if the connection is still valid
                Arc::strong_count(connection) > 1 // At least our reference and possibly others
            });
        }

        // Force shutdown any remaining connections
        let remaining_count = connections.len();
        if remaining_count > 0 {
            warn!("{} MCP provider connections did not shutdown gracefully within timeout, forcing shutdown", remaining_count);
        }

        // Clear all connections (this will drop them and should terminate processes)
        let drained_connections: Vec<_> = connections.drain().collect();
        drop(connections);

        for (provider_name, connection) in drained_connections {
            debug!("Force shutting down MCP provider '{}'", provider_name);

            if let Ok(connection) = Arc::try_unwrap(connection) {
                debug!(
                    "Awaiting MCP provider '{}' task cancellation after graceful request",
                    provider_name
                );

                match connection.cancel().await {
                    Ok(quit_reason) => {
                        debug!(
                            "MCP provider '{}' cancellation completed with reason: {:?}",
                            provider_name,
                            quit_reason
                        );
                    }
                    Err(err) => {
                        debug!(
                            "MCP provider '{}' cancellation join error (non-critical): {}",
                            provider_name,
                            err
                        );
                    }
                }
            } else {
                debug!(
                    "Additional references exist for MCP provider '{}'; dropping without awaiting",
                    provider_name
                );
            }
        }

        // Give processes time to terminate gracefully
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Additional cleanup: try to kill any remaining MCP provider processes
        // This handles cases where the rmcp library doesn't properly terminate processes
        self.kill_remaining_mcp_processes().await;

        info!("MCP client shutdown complete");
        Ok(())
    }
}

/// Information about an MCP tool
#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub provider: String,
    pub input_schema: Value,
}

/// Individual MCP provider wrapper
pub struct McpProvider {
    config: McpProviderConfig,
    tools_cache: Arc<Mutex<Option<ListToolsResult>>>,
}

impl McpProvider {
    /// Create a new MCP provider
    pub async fn new(config: McpProviderConfig) -> Result<Self> {
        Ok(Self {
            config,
            tools_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// List tools available from this provider
    pub async fn list_tools(&self) -> Result<ListToolsResult> {
        let provider_name = &self.config.name;
        debug!("Listing tools for MCP provider '{}'", provider_name);

        // Check cache first
        {
            let cache = self.tools_cache.lock().await;
            if let Some(cached) = cache.as_ref() {
                debug!("Using cached tools for provider '{}'", provider_name);
                return Ok(cached.clone());
            }
        }

        debug!("Connecting to provider '{}' to fetch tools", provider_name);

        // Connect and get tools
        match self.connect().await {
            Ok(connection) => {
                match connection.list_tools(Default::default()).await {
                    Ok(tools) => {
                        debug!("Found {} tools for provider '{}'", tools.tools.len(), provider_name);

                        // Cache the result
                        {
                            let mut cache = self.tools_cache.lock().await;
                            *cache = Some(tools.clone());
                        }

                        Ok(tools)
                    }
                    Err(e) => {
                        error!("Failed to list tools for provider '{}': {}", provider_name, e);
                        Err(anyhow::anyhow!("Failed to list tools: {}", e))
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to provider '{}': {}", provider_name, e);
                Err(e)
            }
        }
    }

    /// Check if this provider has a specific tool
    pub async fn has_tool(&self, tool_name: &str) -> Result<bool> {
        let provider_name = &self.config.name;
        debug!("Checking if provider '{}' has tool '{}'", provider_name, tool_name);

        match tokio::time::timeout(
            tokio::time::Duration::from_secs(10),
            self.list_tools()
        ).await {
            Ok(Ok(tools)) => {
                let has_tool = tools.tools.iter().any(|tool| tool.name == tool_name);
                debug!("Provider '{}' {} tool '{}'", provider_name,
                       if has_tool { "has" } else { "does not have" }, tool_name);
                Ok(has_tool)
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                if error_msg.contains("No such process") || error_msg.contains("ESRCH") ||
                   error_msg.contains("EPIPE") || error_msg.contains("Broken pipe") ||
                   error_msg.contains("write EPIPE") {
                    debug!("MCP provider '{}' process/pipe terminated during tool check (normal during shutdown): {}", provider_name, e);
                } else {
                    warn!("Failed to check tool availability for provider '{}': {}", provider_name, e);
                }
                Err(e)
            }
            Err(_timeout) => {
                warn!("MCP provider '{}' tool check timed out", provider_name);
                Err(anyhow::anyhow!("Tool availability check timeout"))
            }
        }
    }

    /// Connect to this MCP provider
    pub async fn connect(&self) -> Result<RunningMcpService> {
        let provider_name = &self.config.name;
        info!("Connecting to MCP provider '{}'", provider_name);

        match &self.config.transport {
            McpTransportConfig::Stdio(stdio_config) => {
                debug!("Using stdio transport for provider '{}'", provider_name);
                self.connect_stdio(stdio_config).await
            }
            McpTransportConfig::Http(http_config) => {
                debug!("Using HTTP transport for provider '{}'", provider_name);
                self.connect_http(http_config).await
            }
        }
    }

    /// Connect using HTTP transport
    async fn connect_http(&self, config: &crate::config::mcp::McpHttpServerConfig) -> Result<RunningMcpService> {
        let provider_name = &self.config.name;
        debug!("Setting up HTTP connection for provider '{}'", provider_name);

        // Build the HTTP client with proper headers
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());

        // Add API key if provided
        if let Some(api_key_env) = &config.api_key_env {
            if let Ok(api_key) = std::env::var(api_key_env) {
                headers.insert("Authorization", format!("Bearer {}", api_key).parse().unwrap());
            } else {
                warn!("API key environment variable '{}' not found for provider '{}'", api_key_env, provider_name);
            }
        }

        // Add custom headers
        for (key, value) in &config.headers {
            if let (Ok(header_name), Ok(header_value)) = (
                key.parse::<HeaderName>(),
                value.parse::<HeaderValue>()
            ) {
                headers.insert(header_name, header_value);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        // Test basic connectivity first
        debug!("Testing HTTP MCP server connectivity at '{}'", config.endpoint);

        match client
            .get(&config.endpoint)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    debug!("HTTP MCP server at '{}' is reachable (status: {})", config.endpoint, status);

                    // For now, return an error indicating this needs full streamable HTTP implementation
                    // A complete implementation would use Server-Sent Events (SSE) for streaming MCP
                    Err(anyhow::anyhow!(
                        "HTTP MCP server support detected but requires full streamable implementation. \
                         Server is reachable at '{}' with status: {}. \
                         Consider using stdio transport or implement HTTP streaming support.",
                        config.endpoint, status
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server returned error status: {} at endpoint: {}",
                        status, config.endpoint
                    ))
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("dns") || error_msg.contains("Name resolution") {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server DNS resolution failed for '{}': {}",
                        config.endpoint, e
                    ))
                } else if error_msg.contains("Connection refused") || error_msg.contains("connect") {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server connection failed for '{}': {}",
                        config.endpoint, e
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server error for '{}': {}",
                        config.endpoint, e
                    ))
                }
            }
        }
    }

    /// Connect using stdio transport
    async fn connect_stdio(&self, config: &crate::config::mcp::McpStdioServerConfig) -> Result<RunningMcpService> {
        let provider_name = &self.config.name;
        debug!("Setting up stdio connection for provider '{}'", provider_name);

        debug!("Command: {} with args: {:?}", config.command, config.args);

        let mut command = Command::new(&config.command);
        command.args(&config.args);

        // Set working directory if specified
        if let Some(working_dir) = &config.working_directory {
            debug!("Using working directory: {}", working_dir);
            command.current_dir(working_dir);
        }

        // Set environment variables if specified
        if !self.config.env.is_empty() {
            debug!("Setting environment variables for provider '{}'", provider_name);
            command.envs(&self.config.env);
        }

        // Create new process group to ensure proper cleanup
        command.process_group(0);

        debug!("Creating TokioChildProcess for provider '{}'", provider_name);

        match TokioChildProcess::new(command) {
            Ok(child_process) => {
                debug!("Successfully created child process for provider '{}'", provider_name);

                // Add timeout and better error handling for the MCP service
                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(30),
                    ().serve(child_process)
                ).await {
                    Ok(Ok(connection)) => {
                        info!("Successfully established connection to MCP provider '{}'", provider_name);
                        Ok(connection)
                    }
                    Ok(Err(e)) => {
                        // Check if this is a process-related error
                        let error_msg = e.to_string();
                        if error_msg.contains("No such process") || error_msg.contains("ESRCH") ||
                           error_msg.contains("EPIPE") || error_msg.contains("Broken pipe") ||
                           error_msg.contains("write EPIPE") {
                            debug!("MCP provider '{}' pipe/process error during connection (normal during shutdown): {}", provider_name, e);
                            Err(anyhow::anyhow!("MCP provider connection terminated: {}", e))
                        } else {
                            error!("Failed to establish MCP connection for provider '{}': {}", provider_name, e);
                            Err(anyhow::anyhow!("Failed to serve MCP connection: {}", e))
                        }
                    }
                    Err(_timeout) => {
                        warn!("MCP provider '{}' connection timed out after 30 seconds", provider_name);
                        Err(anyhow::anyhow!("MCP provider connection timeout"))
                    }
                }
            }
            Err(e) => {
                // Check if this is a process creation error
                let error_msg = e.to_string();
                if error_msg.contains("No such process") || error_msg.contains("ESRCH") {
                    error!("Failed to create child process for provider '{}' - process may have terminated: {}", provider_name, e);
                } else {
                    error!("Failed to create child process for provider '{}': {}", provider_name, e);
                }
                Err(anyhow::anyhow!("Failed to create child process: {}", e))
            }
        }
    }

}

/// Type alias for running MCP service
type RunningMcpService = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

/// Status information about the MCP client
#[derive(Debug, Clone)]
pub struct McpClientStatus {
    pub enabled: bool,
    pub provider_count: usize,
    pub active_connections: usize,
    pub configured_providers: Vec<String>,
}

impl McpClient {
    /// Get MCP client status information
    pub fn get_status(&self) -> McpClientStatus {
        McpClientStatus {
            enabled: self.config.enabled,
            provider_count: self.providers.len(),
            active_connections: self.active_connections.try_lock()
                .map(|connections| connections.len())
                .unwrap_or(0),
            configured_providers: self.providers.keys().cloned().collect(),
        }
    }
}

/// Trait for MCP tool execution
#[async_trait]
pub trait McpToolExecutor: Send + Sync {
    /// Execute an MCP tool
    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value>;

    /// List available MCP tools
    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>>;

    /// Check if an MCP tool exists
    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool>;

    /// Get MCP client status information
    fn get_status(&self) -> McpClientStatus;
}

#[async_trait]
impl McpToolExecutor for McpClient {
    async fn execute_mcp_tool(&self, tool_name: &str, args: Value) -> Result<Value> {
        self.execute_tool(tool_name, args).await
    }

    async fn list_mcp_tools(&self) -> Result<Vec<McpToolInfo>> {
        self.list_tools().await
    }

    async fn has_mcp_tool(&self, tool_name: &str) -> Result<bool> {
        if self.providers.is_empty() {
            return Ok(false);
        }

        let mut provider_errors = Vec::new();

        for (provider_name, provider) in &self.providers {
            match provider.has_tool(tool_name).await {
                Ok(true) => return Ok(true),
                Ok(false) => continue,
                Err(e) => {
                    let error_msg = format!("Error checking provider '{}': {}", provider_name, e);
                    warn!("{}", error_msg);
                    provider_errors.push(error_msg);
                }
            }
        }

        if !provider_errors.is_empty() {
            debug!("Encountered {} errors while checking tool availability: {:?}",
                   provider_errors.len(), provider_errors);
        }

        Ok(false)
    }

    fn get_status(&self) -> McpClientStatus {
        self.get_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mcp::{McpStdioServerConfig, McpTransportConfig};

    #[test]
    fn test_mcp_client_creation() {
        let config = McpClientConfig::default();
        let client = McpClient::new(config);
        assert!(!client.config.enabled);
        assert!(client.providers.is_empty());
    }

    #[test]
    fn test_mcp_tool_info() {
        let tool_info = McpToolInfo {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            provider: "test_provider".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            }),
        };

        assert_eq!(tool_info.name, "test_tool");
        assert_eq!(tool_info.provider, "test_provider");
    }

    #[test]
    fn test_provider_config() {
        let config = McpProviderConfig {
            name: "test".to_string(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                working_directory: None,
            }),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 3,
        };

        assert_eq!(config.name, "test");
        assert!(config.enabled);
        assert_eq!(config.max_concurrent_requests, 3);
    }

    #[test]
    fn test_tool_info_creation() {
        let tool_info = McpToolInfo {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            provider: "test_provider".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string"}
                }
            }),
        };

        assert_eq!(tool_info.name, "test_tool");
        assert_eq!(tool_info.provider, "test_provider");
    }
}
