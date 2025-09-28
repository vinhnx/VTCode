//! MCP Client implementation
//!
//! This module provides a high-level abstraction over the rmcp library,
//! managing MCP provider connections and tool execution.

use crate::config::mcp::{
    McpAllowListConfig, McpClientConfig, McpProviderConfig, McpTransportConfig,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rmcp::{
    ServiceExt,
    handler::client::ClientHandler,
    model::{
        CallToolRequestParam, CallToolResult, ClientCapabilities, ClientInfo, Implementation,
        ListToolsResult, LoggingLevel, LoggingMessageNotificationParam, RootsCapabilities,
    },
    transport::TokioChildProcess,
};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::future;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::{Level, debug, error, info, warn};

#[derive(Clone)]
struct LoggingClientHandler {
    provider_name: String,
    info: ClientInfo,
}

impl LoggingClientHandler {
    fn new(provider_name: &str) -> Self {
        let mut info = ClientInfo::default();
        info.capabilities = ClientCapabilities {
            roots: Some(RootsCapabilities {
                list_changed: Some(true),
            }),
            ..ClientCapabilities::default()
        };
        info.client_info = Implementation {
            name: "vtcode".to_string(),
            title: Some("VT Code MCP client".to_string()),
            version: env!("CARGO_PKG_VERSION").to_string(),
            icons: None,
            website_url: Some("https://github.com/modelcontextprotocol".to_string()),
        };

        Self {
            provider_name: provider_name.to_string(),
            info,
        }
    }

    fn handle_logging(&self, params: LoggingMessageNotificationParam) {
        let payload = params.data;
        let summary = payload
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| payload.to_string());

        match params.level {
            LoggingLevel::Debug => debug!(
                provider = self.provider_name.as_str(),
                summary = %summary,
                payload = ?payload,
                "MCP provider log"
            ),
            LoggingLevel::Info | LoggingLevel::Notice => info!(
                provider = self.provider_name.as_str(),
                summary = %summary,
                payload = ?payload,
                "MCP provider log"
            ),
            LoggingLevel::Warning => warn!(
                provider = self.provider_name.as_str(),
                summary = %summary,
                payload = ?payload,
                "MCP provider warning"
            ),
            LoggingLevel::Error
            | LoggingLevel::Critical
            | LoggingLevel::Alert
            | LoggingLevel::Emergency => error!(
                provider = self.provider_name.as_str(),
                summary = %summary,
                payload = ?payload,
                "MCP provider error"
            ),
        }
    }
}

impl ClientHandler for LoggingClientHandler {
    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::service::RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.handle_logging(params);
        future::ready(())
    }

    fn get_info(&self) -> ClientInfo {
        self.info.clone()
    }
}

/// High-level MCP client that manages multiple providers
pub struct McpClient {
    config: McpClientConfig,
    pub providers: HashMap<String, Arc<McpProvider>>,
    active_connections: Arc<Mutex<HashMap<String, Arc<RunningMcpService>>>>,
    allowlist: Arc<RwLock<McpAllowListConfig>>,
    tool_provider_index: Arc<RwLock<HashMap<String, String>>>,
}

impl McpClient {
    /// Create a new MCP client with the given configuration
    pub fn new(config: McpClientConfig) -> Self {
        let allowlist = Arc::new(RwLock::new(config.allowlist.clone()));
        Self {
            config,
            providers: HashMap::new(),
            active_connections: Arc::new(Mutex::new(HashMap::new())),
            allowlist,
            tool_provider_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn record_tool_provider(&self, provider: &str, tool: &str) {
        debug!("Recording tool '{}' -> provider '{}'", tool, provider);
        self.tool_provider_index
            .write()
            .insert(tool.to_string(), provider.to_string());
    }

    /// Retrieve provider reference for a known tool name
    pub fn provider_for_tool(&self, tool_name: &str) -> Option<String> {
        let index = self.tool_provider_index.read();
        if let Some(provider) = index.get(tool_name) {
            // Validate that the provider still exists and is enabled
            if self.providers.contains_key(provider) {
                debug!("Found tool '{}' in provider '{}'", tool_name, provider);
                Some(provider.clone())
            } else {
                debug!(
                    "Tool '{}' references non-existent provider '{}'",
                    tool_name, provider
                );
                None
            }
        } else {
            debug!("Tool '{}' not found in provider index", tool_name);
            None
        }
    }

    /// Replace the in-memory MCP allow list with the provided configuration
    pub fn update_allowlist(&self, allowlist: McpAllowListConfig) {
        *self.allowlist.write() = allowlist;
    }

    /// Get a clone of the current allow list
    pub fn current_allowlist(&self) -> McpAllowListConfig {
        self.allowlist.read().clone()
    }

    fn format_tool_result(
        provider_name: &str,
        tool_name: &str,
        result: CallToolResult,
    ) -> Result<Value> {
        let is_error = result.is_error.unwrap_or(false);
        let text_summary = result
            .content
            .iter()
            .find_map(|content| content.as_text().map(|text| text.text.clone()));

        if is_error {
            let detail = result
                .structured_content
                .as_ref()
                .and_then(|value| value.get("message").and_then(Value::as_str))
                .map(str::to_owned)
                .or_else(|| {
                    result
                        .structured_content
                        .as_ref()
                        .map(|value| value.to_string())
                })
                .or(text_summary)
                .unwrap_or_else(|| "Unknown MCP tool error".to_string());

            return Err(anyhow::anyhow!(
                "MCP tool '{}' on provider '{}' reported an error: {}",
                tool_name,
                provider_name,
                detail
            ));
        }

        let mut payload = Map::new();
        payload.insert("provider".into(), Value::String(provider_name.to_string()));
        payload.insert("tool".into(), Value::String(tool_name.to_string()));

        if let Some(meta) = result.meta {
            if let Ok(meta_value) = serde_json::to_value(&meta) {
                if !meta_value.is_null() {
                    payload.insert("meta".into(), meta_value);
                }
            }
        }

        if let Some(structured) = result.structured_content {
            match structured {
                Value::Object(mut object) => {
                    object
                        .entry("provider")
                        .or_insert_with(|| Value::String(provider_name.to_string()));
                    object
                        .entry("tool")
                        .or_insert_with(|| Value::String(tool_name.to_string()));

                    if let Some(meta_value) = payload.remove("meta") {
                        object.entry("meta").or_insert(meta_value);
                    }

                    return Ok(Value::Object(object));
                }
                other => {
                    payload.insert("structured_content".into(), other);
                }
            }
        }

        if let Some(summary) = text_summary {
            payload.insert("message".into(), Value::String(summary));
        }

        if !result.content.is_empty() {
            if let Ok(content_value) = serde_json::to_value(&result.content) {
                payload.insert("content".into(), content_value);
            }
        }

        Ok(Value::Object(payload))
    }

    /// Initialize the MCP client and connect to configured providers
    pub async fn initialize(&mut self) -> Result<()> {
        if !self.config.enabled {
            info!("MCP client is disabled in configuration");
            return Ok(());
        }

        info!(
            "Initializing MCP client with {} configured providers",
            self.config.providers.len()
        );

        for provider_config in &self.config.providers {
            if provider_config.enabled {
                info!("Initializing MCP provider '{}'", provider_config.name);

                match McpProvider::new(provider_config.clone()).await {
                    Ok(provider) => {
                        let provider = Arc::new(provider);
                        self.providers
                            .insert(provider_config.name.clone(), provider);
                        info!(
                            "Successfully initialized MCP provider '{}'",
                            provider_config.name
                        );
                        self.audit_log(
                            Some(provider_config.name.as_str()),
                            "mcp.provider_initialized",
                            Level::INFO,
                            format!("Provider '{}' initialized", provider_config.name),
                        );
                    }
                    Err(e) => {
                        error!(
                            "Failed to initialize MCP provider '{}': {}",
                            provider_config.name, e
                        );
                        self.audit_log(
                            Some(provider_config.name.as_str()),
                            "mcp.provider_initialization_failed",
                            Level::WARN,
                            format!(
                                "Failed to initialize provider '{}' due to error: {}",
                                provider_config.name, e
                            ),
                        );
                        // Continue with other providers instead of failing completely
                        continue;
                    }
                }
            } else {
                debug!(
                    "MCP provider '{}' is disabled, skipping",
                    provider_config.name
                );
            }
        }

        info!(
            "MCP client initialization complete. Active providers: {}",
            self.providers.len()
        );

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
            tokio::time::Duration::from_secs(5),
            self.attempt_process_cleanup(),
        )
        .await;

        match process_cleanup_attempts {
            Ok(Ok(cleaned_count)) => {
                if cleaned_count > 0 {
                    info!(
                        "Cleaned up {} remaining MCP provider processes",
                        cleaned_count
                    );
                    self.audit_log(
                        None,
                        "mcp.process_cleanup",
                        Level::INFO,
                        format!(
                            "Cleaned up {} remaining MCP provider processes",
                            cleaned_count
                        ),
                    );
                } else {
                    debug!("No remaining MCP provider processes to clean up");
                }
            }
            Ok(Err(e)) => {
                warn!("Error during MCP process cleanup (non-critical): {}", e);
                self.audit_log(
                    None,
                    "mcp.process_cleanup_error",
                    Level::WARN,
                    format!("Error during MCP process cleanup: {}", e),
                );
            }
            Err(_) => {
                warn!("MCP process cleanup timed out (non-critical)");
                self.audit_log(
                    None,
                    "mcp.process_cleanup_timeout",
                    Level::WARN,
                    "MCP process cleanup timed out".to_string(),
                );
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

            // Try multiple approaches to find and kill processes
            let mut provider_cleaned = 0;

            // Approach 1: Use pgrep with command pattern
            if let Ok(output) = TokioCommand::new("pgrep")
                .args(["-f", &format!("mcp-server-{}", provider_name)])
                .output()
                .await
            {
                if output.status.success() {
                    let pids = String::from_utf8_lossy(&output.stdout);
                    for pid_str in pids.lines() {
                        if let Ok(pid) = pid_str.trim().parse::<u32>() {
                            if pid != current_pid && pid > 0 {
                                if self.kill_process_gracefully(pid).await {
                                    provider_cleaned += 1;
                                }
                            }
                        }
                    }
                }
            }

            // Approach 2: If pgrep failed, try ps with grep
            if provider_cleaned == 0 {
                if let Ok(output) = TokioCommand::new("ps").args(["aux"]).output().await {
                    if output.status.success() {
                        let processes = String::from_utf8_lossy(&output.stdout);
                        for line in processes.lines() {
                            // Look for lines containing the provider name and MCP-related terms
                            if line.contains(provider_name)
                                && (line.contains("mcp")
                                    || line.contains("node")
                                    || line.contains("python"))
                            {
                                // Extract PID from ps output (first column)
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if let Some(pid_str) = parts.first() {
                                    if let Ok(pid) = pid_str.parse::<u32>() {
                                        if pid != current_pid && pid > 0 {
                                            if self.kill_process_gracefully(pid).await {
                                                provider_cleaned += 1;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if provider_cleaned > 0 {
                debug!(
                    "Cleaned up {} processes for MCP provider '{}'",
                    provider_cleaned, provider_name
                );
                cleaned_count += provider_cleaned;
                // Clear the tool provider index when we kill processes
                self.tool_provider_index.write().clear();
            }
        }

        Ok(cleaned_count)
    }

    /// Kill a process gracefully with TERM first, then KILL if needed
    async fn kill_process_gracefully(&self, pid: u32) -> bool {
        debug!("Killing process {} gracefully", pid);

        // Try graceful termination first
        let _ = tokio::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .await;

        // Give it a moment to terminate gracefully
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check if process is still running
        if let Ok(output) = tokio::process::Command::new("kill")
            .args(["-0", &pid.to_string()]) // Check if process exists
            .output()
            .await
        {
            if output.status.success() {
                // Process still exists, force kill it
                debug!("Process {} still running, force killing", pid);
                let _ = tokio::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .output()
                    .await;
                true
            } else {
                // Process already terminated
                debug!("Process {} already terminated", pid);
                true
            }
        } else {
            // kill -0 command failed, assume process doesn't exist
            debug!("Process {} check failed, assuming terminated", pid);
            true
        }
    }

    /// Clean up providers with terminated processes
    pub async fn cleanup_dead_providers(&self) -> Result<()> {
        let mut dead_providers = Vec::new();

        for (provider_name, provider) in &self.providers {
            // Try to check if provider is still alive by attempting a quick operation
            let provider_health_check = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                provider.has_tool("ping"),
            )
            .await;

            match provider_health_check {
                Ok(Ok(_)) => {
                    // Provider is responsive
                    debug!("MCP provider '{}' is healthy", provider_name);
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("No such process") || error_msg.contains("ESRCH") {
                        warn!(
                            "MCP provider '{}' has terminated process, marking for cleanup",
                            provider_name
                        );
                        dead_providers.push(provider_name.clone());
                    } else {
                        debug!(
                            "MCP provider '{}' returned error but process may be alive: {}",
                            provider_name, e
                        );
                    }
                }
                Err(_timeout) => {
                    warn!(
                        "MCP provider '{}' health check timed out, may be unresponsive",
                        provider_name
                    );
                    // Don't mark as dead on timeout, might just be slow
                }
            }
        }

        // Note: In a real implementation, we'd want to remove dead providers from the providers map
        // For now, we'll just log them
        if !dead_providers.is_empty() {
            warn!(
                "Found {} dead MCP providers: {:?}",
                dead_providers.len(),
                dead_providers
            );
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

        let allowlist_snapshot = self.allowlist.read().clone();

        for (provider_name, provider) in &self.providers {
            let provider_id = provider_name.as_str();
            match tokio::time::timeout(tokio::time::Duration::from_secs(15), provider.list_tools())
                .await
            {
                Ok(Ok(tools)) => {
                    debug!(
                        "Provider '{}' has {} tools",
                        provider_name,
                        tools.tools.len()
                    );

                    for tool in tools.tools {
                        let tool_name = tool.name.as_ref();

                        if allowlist_snapshot.is_tool_allowed(provider_id, tool_name) {
                            self.record_tool_provider(provider_id, tool_name);
                            all_tools.push(McpToolInfo {
                                name: tool_name.to_string(),
                                description: tool.description.unwrap_or_default().to_string(),
                                provider: provider_name.clone(),
                                input_schema: serde_json::to_value(&*tool.input_schema)
                                    .unwrap_or(Value::Null),
                            });
                        } else {
                            self.audit_log(
                                Some(provider_id),
                                "mcp.tool_filtered",
                                Level::DEBUG,
                                format!(
                                    "Filtered tool '{}' from provider '{}' due to allow list",
                                    tool_name, provider_id
                                ),
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("No such process")
                        || error_msg.contains("ESRCH")
                        || error_msg.contains("EPIPE")
                        || error_msg.contains("Broken pipe")
                        || error_msg.contains("write EPIPE")
                    {
                        debug!(
                            "MCP provider '{}' process/pipe terminated during tool listing (normal during shutdown): {}",
                            provider_name, e
                        );
                    } else {
                        warn!(
                            "Failed to list tools for provider '{}': {}",
                            provider_name, e
                        );
                    }
                    let error_msg = format!(
                        "Failed to list tools for provider '{}': {}",
                        provider_name, e
                    );
                    errors.push(error_msg);
                }
                Err(_timeout) => {
                    warn!("MCP provider '{}' tool listing timed out", provider_name);
                    let error_msg =
                        format!("Tool listing timeout for provider '{}'", provider_name);
                    errors.push(error_msg);
                }
            }
        }

        if !errors.is_empty() {
            warn!(
                "Encountered {} errors while listing MCP tools: {:?}",
                errors.len(),
                errors
            );
        }

        info!(
            "Found {} total MCP tools across all providers",
            all_tools.len()
        );
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
                        let error_msg = format!(
                            "Error checking tool availability for provider '{}': {}",
                            name, e
                        );
                        warn!("{}", error_msg);
                        provider_errors.push(error_msg);
                    }
                }
            }

            found_provider.ok_or_else(|| {
                let error_msg = format!(
                    "Tool '{}' not found in any MCP provider. Provider errors: {:?}",
                    tool_name, provider_errors
                );
                anyhow::anyhow!(error_msg)
            })?
        };

        debug!("Found tool '{}' in provider '{}'", tool_name, provider_name);

        if !self
            .allowlist
            .read()
            .is_tool_allowed(provider_name.as_str(), tool_name)
        {
            let message = format!(
                "Tool '{}' from provider '{}' is not permitted by the MCP allow list",
                tool_name, provider_name
            );
            self.audit_log(
                Some(provider_name.as_str()),
                "mcp.tool_denied",
                Level::WARN,
                message.as_str(),
            );
            return Err(anyhow::anyhow!(message));
        }

        self.record_tool_provider(provider_name.as_str(), tool_name);

        let provider = self.providers.get(&provider_name).ok_or_else(|| {
            anyhow::anyhow!("Provider '{}' not found after discovery", provider_name)
        })?;

        // Get or create connection for this provider
        let connection = match self.get_or_create_connection(provider).await {
            Ok(conn) => conn,
            Err(e) => {
                error!(
                    "Failed to establish connection to provider '{}': {}",
                    provider_name, e
                );
                return Err(e);
            }
        };

        // Execute the tool call
        match connection
            .call_tool(CallToolRequestParam {
                name: tool_name_owned.into(),
                arguments: args.as_object().cloned(),
            })
            .await
        {
            Ok(result) => match Self::format_tool_result(provider_name.as_str(), tool_name, result)
            {
                Ok(serialized) => {
                    info!(
                        "Successfully executed MCP tool '{}' via provider '{}'",
                        tool_name, provider_name
                    );
                    self.audit_log(
                        Some(provider_name.as_str()),
                        "mcp.tool_execution",
                        Level::INFO,
                        format!(
                            "Successfully executed MCP tool '{}' via provider '{}'",
                            tool_name, provider_name
                        ),
                    );
                    Ok(serialized)
                }
                Err(err) => {
                    let err_message = err.to_string();
                    warn!(
                        "MCP tool '{}' via provider '{}' returned an error payload: {}",
                        tool_name, provider_name, err_message
                    );
                    self.audit_log(
                        Some(provider_name.as_str()),
                        "mcp.tool_failed",
                        Level::WARN,
                        format!(
                            "MCP tool '{}' via provider '{}' returned an error payload: {}",
                            tool_name, provider_name, err_message
                        ),
                    );
                    Err(err)
                }
            },
            Err(e) => {
                let error_message = e.to_string();

                error!(
                    "MCP tool '{}' failed on provider '{}': {}",
                    tool_name, provider_name, error_message
                );
                self.audit_log(
                    Some(provider_name.as_str()),
                    "mcp.tool_failed",
                    Level::WARN,
                    format!(
                        "MCP tool '{}' failed on provider '{}': {}",
                        tool_name, provider_name, error_message
                    ),
                );

                // Handle different types of connection errors
                if error_message.contains("EPIPE")
                    || error_message.contains("Broken pipe")
                    || error_message.contains("write EPIPE")
                    || error_message.contains("No such process")
                    || error_message.contains("ESRCH")
                {
                    // Drop the stale connection so a fresh process can be created next time
                    let mut connections = self.active_connections.lock().await;
                    connections.remove(&provider_name);
                    // Remove cached tool-provider mapping so it is refreshed on reconnect
                    self.tool_provider_index
                        .write()
                        .retain(|_, provider| provider != &provider_name);

                    return Err(anyhow::anyhow!(
                        "MCP provider '{}' disconnected unexpectedly while executing '{}'. The provider process may have terminated. Try re-running the command to restart the provider.",
                        provider_name,
                        tool_name
                    ));
                } else if error_message.contains("timeout") || error_message.contains("Timeout") {
                    // Drop the stale connection on timeout
                    let mut connections = self.active_connections.lock().await;
                    connections.remove(&provider_name);

                    return Err(anyhow::anyhow!(
                        "MCP tool '{}' execution timed out on provider '{}'. The provider may be unresponsive. Try re-running the command.",
                        tool_name,
                        provider_name
                    ));
                } else if error_message.contains("permission")
                    || error_message.contains("Permission denied")
                {
                    return Err(anyhow::anyhow!(
                        "Permission denied executing MCP tool '{}' on provider '{}': {}",
                        tool_name,
                        provider_name,
                        error_message
                    ));
                } else if error_message.contains("network")
                    || error_message.contains("Connection refused")
                {
                    return Err(anyhow::anyhow!(
                        "Network error executing MCP tool '{}' on provider '{}': {}",
                        tool_name,
                        provider_name,
                        error_message
                    ));
                }

                Err(anyhow::anyhow!(
                    "MCP tool execution failed: {}",
                    error_message
                ))
            }
        }
    }

    /// Get or create a connection to the specified provider
    async fn get_or_create_connection(
        &self,
        provider: &McpProvider,
    ) -> Result<Arc<RunningMcpService>> {
        let provider_name = &provider.config.name;
        debug!("Getting connection for MCP provider '{}'", provider_name);

        let mut connections = self.active_connections.lock().await;

        if !connections.contains_key(provider_name) {
            debug!("Creating new connection for provider '{}'", provider_name);

            // Add timeout for connection creation
            match tokio::time::timeout(tokio::time::Duration::from_secs(30), provider.connect())
                .await
            {
                Ok(Ok(connection)) => {
                    let connection = Arc::new(connection);
                    connections.insert(provider_name.clone(), Arc::clone(&connection));
                    debug!(
                        "Successfully created connection for provider '{}'",
                        provider_name
                    );
                    Ok(connection)
                }
                Ok(Err(e)) => {
                    let error_msg = e.to_string();
                    if error_msg.contains("HTTP MCP server support") {
                        warn!(
                            "Provider '{}' uses HTTP transport which is not fully implemented: {}",
                            provider_name, e
                        );
                        Err(anyhow::anyhow!(
                            "HTTP MCP transport not fully implemented for provider '{}'. Consider using stdio transport instead.",
                            provider_name
                        ))
                    } else if error_msg.contains("command not found")
                        || error_msg.contains("No such file")
                    {
                        error!("Command not found for provider '{}': {}", provider_name, e);
                        Err(anyhow::anyhow!(
                            "Command '{}' not found for MCP provider '{}'. Please ensure the MCP server is installed and accessible.",
                            self.config
                                .providers
                                .iter()
                                .find(|p| p.name == *provider_name)
                                .map(|p| match &p.transport {
                                    McpTransportConfig::Stdio(stdio) => stdio.command.as_str(),
                                    _ => "unknown",
                                })
                                .unwrap_or("unknown"),
                            provider_name
                        ))
                    } else if error_msg.contains("permission")
                        || error_msg.contains("Permission denied")
                    {
                        error!(
                            "Permission denied creating connection for provider '{}': {}",
                            provider_name, e
                        );
                        Err(anyhow::anyhow!(
                            "Permission denied executing MCP server for provider '{}': {}",
                            provider_name,
                            error_msg
                        ))
                    } else {
                        error!(
                            "Failed to create connection for provider '{}': {}",
                            provider_name, e
                        );
                        Err(anyhow::anyhow!(
                            "Failed to create connection for MCP provider '{}': {}",
                            provider_name,
                            error_msg
                        ))
                    }
                }
                Err(_timeout) => {
                    error!(
                        "Connection creation timed out for provider '{}' after {} seconds",
                        provider_name, 30
                    );
                    Err(anyhow::anyhow!(
                        "Connection creation timed out for MCP provider '{}' after {} seconds. The provider may be slow to start or unresponsive.",
                        provider_name,
                        30
                    ))
                }
            }
        } else {
            // Validate existing connection is still healthy
            let existing_connection = connections.get(provider_name).unwrap().clone();

            // Quick health check - try to use the connection
            if let Err(e) = self
                .validate_connection(provider_name, &existing_connection)
                .await
            {
                debug!(
                    "Existing connection for provider '{}' is unhealthy, creating new one: {}",
                    provider_name, e
                );

                // Remove the unhealthy connection
                connections.remove(provider_name);

                // Create new connection
                match tokio::time::timeout(tokio::time::Duration::from_secs(30), provider.connect())
                    .await
                {
                    Ok(Ok(connection)) => {
                        let connection = Arc::new(connection);
                        connections.insert(provider_name.clone(), Arc::clone(&connection));
                        debug!(
                            "Successfully created new connection for provider '{}'",
                            provider_name
                        );
                        Ok(connection)
                    }
                    Ok(Err(e)) => {
                        error!(
                            "Failed to create replacement connection for provider '{}': {}",
                            provider_name, e
                        );
                        Err(e)
                    }
                    Err(_timeout) => {
                        error!(
                            "Replacement connection creation timed out for provider '{}'",
                            provider_name
                        );
                        Err(anyhow::anyhow!(
                            "Replacement connection timeout for provider '{}'",
                            provider_name
                        ))
                    }
                }
            } else {
                debug!(
                    "Reusing existing healthy connection for provider '{}'",
                    provider_name
                );
                Ok(existing_connection)
            }
        }
    }

    /// Validate that an existing connection is still healthy
    async fn validate_connection(
        &self,
        provider_name: &str,
        connection: &RunningMcpService,
    ) -> Result<()> {
        debug!(
            "Validating connection health for provider '{}'",
            provider_name
        );

        // Try to ping the connection with a simple tool check
        // Use a very short timeout to avoid blocking
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            connection.list_tools(Default::default()),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!(
                    "Connection health check passed for provider '{}'",
                    provider_name
                );
                Ok(())
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                debug!(
                    "Connection health check failed for provider '{}': {}",
                    provider_name, error_msg
                );
                Err(anyhow::anyhow!(
                    "Connection health check failed for provider '{}': {}",
                    provider_name,
                    error_msg
                ))
            }
            Err(_) => {
                debug!(
                    "Connection health check timed out for provider '{}'",
                    provider_name
                );
                Err(anyhow::anyhow!(
                    "Connection health check timed out for provider '{}'",
                    provider_name
                ))
            }
        }
    }

    fn audit_log(
        &self,
        provider: Option<&str>,
        channel: &str,
        level: Level,
        message: impl AsRef<str>,
    ) {
        let logging_allowed = {
            let allowlist = self.allowlist.read();
            allowlist.is_logging_channel_allowed(provider, channel)
        };

        if !logging_allowed {
            return;
        }

        let msg = message.as_ref();
        match level {
            Level::ERROR => error!(target: "mcp", "[{}] {}", channel, msg),
            Level::WARN => warn!(target: "mcp", "[{}] {}", channel, msg),
            Level::INFO => info!(target: "mcp", "[{}] {}", channel, msg),
            Level::DEBUG => debug!(target: "mcp", "[{}] {}", channel, msg),
            _ => debug!(target: "mcp", "[{}] {}", channel, msg),
        }
    }

    /// Shutdown all MCP connections
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down MCP client and all provider connections");

        let mut connections = self.active_connections.lock().await;

        if connections.is_empty() {
            info!("No active MCP connections to shutdown");
            return Ok(());
        }

        info!(
            "Shutting down {} MCP provider connections",
            connections.len()
        );

        let cancellation_tokens: Vec<(String, rmcp::service::RunningServiceCancellationToken)> =
            connections
                .iter()
                .map(|(provider_name, connection)| {
                    debug!(
                        "Initiating graceful shutdown for MCP provider '{}'",
                        provider_name
                    );
                    (provider_name.clone(), connection.cancellation_token())
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
            warn!(
                "{} MCP provider connections did not shutdown gracefully within timeout, forcing shutdown",
                remaining_count
            );
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
                            provider_name, quit_reason
                        );
                    }
                    Err(err) => {
                        debug!(
                            "MCP provider '{}' cancellation join error (non-critical): {}",
                            provider_name, err
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
                        debug!(
                            "Found {} tools for provider '{}'",
                            tools.tools.len(),
                            provider_name
                        );

                        // Cache the result
                        {
                            let mut cache = self.tools_cache.lock().await;
                            *cache = Some(tools.clone());
                        }

                        Ok(tools)
                    }
                    Err(e) => {
                        error!(
                            "Failed to list tools for provider '{}': {}",
                            provider_name, e
                        );
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
        debug!(
            "Checking if provider '{}' has tool '{}'",
            provider_name, tool_name
        );

        match tokio::time::timeout(tokio::time::Duration::from_secs(10), self.list_tools()).await {
            Ok(Ok(tools)) => {
                let has_tool = tools.tools.iter().any(|tool| tool.name == tool_name);
                debug!(
                    "Provider '{}' {} tool '{}'",
                    provider_name,
                    if has_tool { "has" } else { "does not have" },
                    tool_name
                );
                Ok(has_tool)
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                if error_msg.contains("No such process")
                    || error_msg.contains("ESRCH")
                    || error_msg.contains("EPIPE")
                    || error_msg.contains("Broken pipe")
                    || error_msg.contains("write EPIPE")
                {
                    debug!(
                        "MCP provider '{}' process/pipe terminated during tool check (normal during shutdown): {}",
                        provider_name, e
                    );
                } else {
                    warn!(
                        "Failed to check tool availability for provider '{}': {}",
                        provider_name, e
                    );
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
    async fn connect(&self) -> Result<RunningMcpService> {
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
    async fn connect_http(
        &self,
        config: &crate::config::mcp::McpHttpServerConfig,
    ) -> Result<RunningMcpService> {
        let provider_name = &self.config.name;
        debug!(
            "Setting up HTTP connection for provider '{}'",
            provider_name
        );

        // Build the HTTP client with proper headers
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());

        // Add API key if provided
        if let Some(api_key_env) = &config.api_key_env {
            if let Ok(api_key) = std::env::var(api_key_env) {
                headers.insert(
                    "Authorization",
                    format!("Bearer {}", api_key).parse().unwrap(),
                );
            } else {
                warn!(
                    "API key environment variable '{}' not found for provider '{}'",
                    api_key_env, provider_name
                );
            }
        }

        // Add custom headers
        for (key, value) in &config.headers {
            if let (Ok(header_name), Ok(header_value)) =
                (key.parse::<HeaderName>(), value.parse::<HeaderValue>())
            {
                headers.insert(header_name, header_value);
            }
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to build HTTP client")?;

        // Test basic connectivity first
        debug!(
            "Testing HTTP MCP server connectivity at '{}'",
            config.endpoint
        );

        match client.get(&config.endpoint).send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    debug!(
                        "HTTP MCP server at '{}' is reachable (status: {})",
                        config.endpoint, status
                    );

                    // Check if the server supports MCP by looking for the MCP endpoint
                    // According to MCP spec, servers should expose tools at /mcp endpoint
                    let mcp_endpoint = if config.endpoint.ends_with('/') {
                        format!("{}mcp", config.endpoint)
                    } else {
                        format!("{}/mcp", config.endpoint)
                    };

                    debug!("Attempting to connect to MCP endpoint: {}", mcp_endpoint);

                    // Try to connect to the MCP endpoint
                    match client.get(&mcp_endpoint).send().await {
                        Ok(mcp_response) => {
                            if mcp_response.status().is_success() {
                                debug!(
                                    "MCP endpoint '{}' is available (status: {})",
                                    mcp_endpoint,
                                    mcp_response.status()
                                );

                                // For now, return an error indicating this needs full streamable HTTP implementation
                                // A complete implementation would use Server-Sent Events (SSE) for streaming MCP
                                Err(anyhow::anyhow!(
                                    "HTTP MCP server detected at '{}' but full streamable HTTP implementation is required. \
                                     MCP endpoint is available at '{}'. \
                                     Consider using stdio transport or implement HTTP streaming support with Server-Sent Events.",
                                    config.endpoint,
                                    mcp_endpoint
                                ))
                            } else {
                                debug!(
                                    "MCP endpoint '{}' returned status: {}",
                                    mcp_endpoint,
                                    mcp_response.status()
                                );
                                Err(anyhow::anyhow!(
                                    "HTTP MCP server at '{}' does not support MCP protocol. \
                                     Expected MCP endpoint at '{}' but got status: {}. \
                                     Consider using stdio transport instead.",
                                    config.endpoint,
                                    mcp_endpoint,
                                    mcp_response.status()
                                ))
                            }
                        }
                        Err(e) => {
                            let error_msg = e.to_string();
                            debug!(
                                "Failed to connect to MCP endpoint '{}': {}",
                                mcp_endpoint, error_msg
                            );

                            Err(anyhow::anyhow!(
                                "HTTP MCP server at '{}' does not properly support MCP protocol. \
                                 Could not connect to MCP endpoint at '{}': {}. \
                                 Consider using stdio transport instead.",
                                config.endpoint,
                                mcp_endpoint,
                                error_msg
                            ))
                        }
                    }
                } else {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server returned error status: {} at endpoint: {}",
                        status,
                        config.endpoint
                    ))
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("dns") || error_msg.contains("Name resolution") {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server DNS resolution failed for '{}': {}",
                        config.endpoint,
                        e
                    ))
                } else if error_msg.contains("Connection refused") || error_msg.contains("connect")
                {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server connection failed for '{}': {}",
                        config.endpoint,
                        e
                    ))
                } else {
                    Err(anyhow::anyhow!(
                        "HTTP MCP server error for '{}': {}",
                        config.endpoint,
                        e
                    ))
                }
            }
        }
    }

    /// Connect using stdio transport
    async fn connect_stdio(
        &self,
        config: &crate::config::mcp::McpStdioServerConfig,
    ) -> Result<RunningMcpService> {
        let provider_name = &self.config.name;
        debug!(
            "Setting up stdio connection for provider '{}'",
            provider_name
        );

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
            debug!(
                "Setting environment variables for provider '{}'",
                provider_name
            );
            command.envs(&self.config.env);
        }

        // Create new process group to ensure proper cleanup
        command.process_group(0);

        debug!(
            "Creating TokioChildProcess for provider '{}'",
            provider_name
        );

        match TokioChildProcess::new(command) {
            Ok(child_process) => {
                debug!(
                    "Successfully created child process for provider '{}'",
                    provider_name
                );

                // Add timeout and better error handling for the MCP service
                let handler = LoggingClientHandler::new(provider_name);

                match tokio::time::timeout(
                    tokio::time::Duration::from_secs(30),
                    handler.serve(child_process),
                )
                .await
                {
                    Ok(Ok(connection)) => {
                        info!(
                            "Successfully established connection to MCP provider '{}'",
                            provider_name
                        );
                        Ok(connection)
                    }
                    Ok(Err(e)) => {
                        // Check if this is a process-related error
                        let error_msg = e.to_string();
                        if error_msg.contains("No such process")
                            || error_msg.contains("ESRCH")
                            || error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            debug!(
                                "MCP provider '{}' pipe/process error during connection (normal during shutdown): {}",
                                provider_name, e
                            );
                            Err(anyhow::anyhow!("MCP provider connection terminated: {}", e))
                        } else {
                            error!(
                                "Failed to establish MCP connection for provider '{}': {}",
                                provider_name, e
                            );
                            Err(anyhow::anyhow!("Failed to serve MCP connection: {}", e))
                        }
                    }
                    Err(_timeout) => {
                        warn!(
                            "MCP provider '{}' connection timed out after 30 seconds",
                            provider_name
                        );
                        Err(anyhow::anyhow!("MCP provider connection timeout"))
                    }
                }
            }
            Err(e) => {
                // Check if this is a process creation error
                let error_msg = e.to_string();
                if error_msg.contains("No such process") || error_msg.contains("ESRCH") {
                    error!(
                        "Failed to create child process for provider '{}' - process may have terminated: {}",
                        provider_name, e
                    );
                } else {
                    error!(
                        "Failed to create child process for provider '{}': {}",
                        provider_name, e
                    );
                }
                Err(anyhow::anyhow!("Failed to create child process: {}", e))
            }
        }
    }
}

/// Type alias for running MCP service
type RunningMcpService =
    rmcp::service::RunningService<rmcp::service::RoleClient, LoggingClientHandler>;

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
            active_connections: self
                .active_connections
                .try_lock()
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
            let provider_id = provider_name.as_str();
            match provider.has_tool(tool_name).await {
                Ok(true) => {
                    if self
                        .allowlist
                        .read()
                        .is_tool_allowed(provider_id, tool_name)
                    {
                        self.record_tool_provider(provider_id, tool_name);
                        return Ok(true);
                    }

                    self.audit_log(
                        Some(provider_id),
                        "mcp.tool_denied",
                        Level::DEBUG,
                        format!(
                            "Tool '{}' exists on provider '{}' but is blocked by allow list",
                            tool_name, provider_id
                        ),
                    );
                }
                Ok(false) => continue,
                Err(e) => {
                    let error_msg = format!("Error checking provider '{}': {}", provider_name, e);
                    warn!("{}", error_msg);
                    provider_errors.push(error_msg);
                }
            }
        }

        if !provider_errors.is_empty() {
            debug!(
                "Encountered {} errors while checking tool availability: {:?}",
                provider_errors.len(),
                provider_errors
            );
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
    use rmcp::model::{Content, Meta};
    use serde_json::json;

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

    #[test]
    fn test_format_tool_result_success() {
        let mut result = CallToolResult::structured(json!({
            "value": 42,
            "status": "ok"
        }));
        let mut meta = Meta::new();
        meta.insert("query".to_string(), Value::String("tokio".to_string()));
        result.meta = Some(meta);

        let serialized = McpClient::format_tool_result("test", "demo", result).unwrap();
        assert_eq!(
            serialized.get("provider").and_then(Value::as_str),
            Some("test")
        );
        assert_eq!(serialized.get("tool").and_then(Value::as_str), Some("demo"));
        assert_eq!(serialized.get("status").and_then(Value::as_str), Some("ok"));
        assert_eq!(serialized.get("value").and_then(Value::as_i64), Some(42));
        assert_eq!(
            serialized
                .get("meta")
                .and_then(Value::as_object)
                .and_then(|map| map.get("query"))
                .and_then(Value::as_str),
            Some("tokio")
        );
    }

    #[test]
    fn test_format_tool_result_error_detection() {
        let result = CallToolResult::structured_error(json!({
            "message": "something went wrong"
        }));

        let error = McpClient::format_tool_result("test", "demo", result).unwrap_err();
        assert!(error.to_string().contains("something went wrong"));
    }

    #[test]
    fn test_format_tool_result_error_from_text_content() {
        let result = CallToolResult::error(vec![Content::text("plain failure")]);

        let error = McpClient::format_tool_result("test", "demo", result).unwrap_err();
        assert!(error.to_string().contains("plain failure"));
    }
}
