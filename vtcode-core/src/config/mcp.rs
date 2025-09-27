use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level MCP configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpClientConfig {
    /// Enable MCP functionality
    #[serde(default = "default_mcp_enabled")]
    pub enabled: bool,

    /// MCP UI display configuration
    #[serde(default)]
    pub ui: McpUiConfig,

    /// Configured MCP providers
    #[serde(default)]
    pub providers: Vec<McpProviderConfig>,

    /// MCP server configuration (for vtcode to expose tools)
    #[serde(default)]
    pub server: McpServerConfig,

    /// Maximum number of concurrent MCP connections
    #[serde(default = "default_max_concurrent_connections")]
    pub max_concurrent_connections: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,

    /// Connection retry attempts
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
}

impl Default for McpClientConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_enabled(),
            ui: McpUiConfig::default(),
            providers: Vec::new(),
            server: McpServerConfig::default(),
            max_concurrent_connections: default_max_concurrent_connections(),
            request_timeout_seconds: default_request_timeout_seconds(),
            retry_attempts: default_retry_attempts(),
        }
    }
}

/// UI configuration for MCP display
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpUiConfig {
    /// UI mode for MCP events: "compact" or "full"
    #[serde(default = "default_mcp_ui_mode")]
    pub mode: McpUiMode,

    /// Maximum number of MCP events to display
    #[serde(default = "default_max_mcp_events")]
    pub max_events: usize,

    /// Show MCP provider names in UI
    #[serde(default = "default_show_provider_names")]
    pub show_provider_names: bool,
}

impl Default for McpUiConfig {
    fn default() -> Self {
        Self {
            mode: default_mcp_ui_mode(),
            max_events: default_max_mcp_events(),
            show_provider_names: default_show_provider_names(),
        }
    }
}

/// UI mode for MCP event display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpUiMode {
    /// Compact mode - shows only event titles
    Compact,
    /// Full mode - shows detailed event logs
    Full,
}

impl Default for McpUiMode {
    fn default() -> Self {
        McpUiMode::Compact
    }
}

/// Configuration for a single MCP provider
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpProviderConfig {
    /// Provider name (used for identification)
    pub name: String,

    /// Transport configuration
    #[serde(flatten)]
    pub transport: McpTransportConfig,

    /// Provider-specific environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether this provider is enabled
    #[serde(default = "default_provider_enabled")]
    pub enabled: bool,

    /// Maximum number of concurrent requests to this provider
    #[serde(default = "default_provider_max_concurrent")]
    pub max_concurrent_requests: usize,
}

impl Default for McpProviderConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig::default()),
            env: HashMap::new(),
            enabled: default_provider_enabled(),
            max_concurrent_requests: default_provider_max_concurrent(),
        }
    }
}

/// Configuration for the MCP server (vtcode acting as an MCP server)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpServerConfig {
    /// Enable vtcode's MCP server capability
    #[serde(default = "default_mcp_server_enabled")]
    pub enabled: bool,

    /// Bind address for the MCP server
    #[serde(default = "default_mcp_server_bind")]
    pub bind_address: String,

    /// Port for the MCP server
    #[serde(default = "default_mcp_server_port")]
    pub port: u16,

    /// Server transport type
    #[serde(default = "default_mcp_server_transport")]
    pub transport: McpServerTransport,

    /// Server identifier
    #[serde(default = "default_mcp_server_name")]
    pub name: String,

    /// Server version
    #[serde(default = "default_mcp_server_version")]
    pub version: String,

    /// Tools exposed by the vtcode MCP server
    #[serde(default)]
    pub exposed_tools: Vec<String>,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_server_enabled(),
            bind_address: default_mcp_server_bind(),
            port: default_mcp_server_port(),
            transport: default_mcp_server_transport(),
            name: default_mcp_server_name(),
            version: default_mcp_server_version(),
            exposed_tools: Vec::new(),
        }
    }
}

/// MCP server transport types
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpServerTransport {
    /// Server Sent Events transport
    Sse,
    /// HTTP transport
    Http,
}

impl Default for McpServerTransport {
    fn default() -> Self {
        McpServerTransport::Sse
    }
}

/// Transport configuration for MCP providers
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum McpTransportConfig {
    /// Standard I/O transport (stdio)
    Stdio(McpStdioServerConfig),
    /// HTTP transport
    Http(McpHttpServerConfig),
}

/// Configuration for stdio-based MCP servers
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpStdioServerConfig {
    /// Command to execute
    pub command: String,

    /// Command arguments
    pub args: Vec<String>,

    /// Working directory for the command
    #[serde(default)]
    pub working_directory: Option<String>,
}

impl Default for McpStdioServerConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            working_directory: None,
        }
    }
}

/// Configuration for HTTP-based MCP servers
///
/// Note: HTTP transport is partially implemented. Basic connectivity testing is supported,
/// but full streamable HTTP MCP server support requires additional implementation
/// using Server-Sent Events (SSE) or WebSocket connections.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpHttpServerConfig {
    /// Server endpoint URL
    pub endpoint: String,

    /// API key environment variable name
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Protocol version
    #[serde(default = "default_mcp_protocol_version")]
    pub protocol_version: String,

    /// Headers to include in requests
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl Default for McpHttpServerConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            api_key_env: None,
            protocol_version: default_mcp_protocol_version(),
            headers: HashMap::new(),
        }
    }
}

/// Default value functions
fn default_mcp_enabled() -> bool {
    false
}

fn default_mcp_ui_mode() -> McpUiMode {
    McpUiMode::Compact
}

fn default_max_mcp_events() -> usize {
    50
}

fn default_show_provider_names() -> bool {
    true
}

fn default_max_concurrent_connections() -> usize {
    5
}

fn default_request_timeout_seconds() -> u64 {
    30
}

fn default_retry_attempts() -> u32 {
    3
}

fn default_provider_enabled() -> bool {
    true
}

fn default_provider_max_concurrent() -> usize {
    3
}

fn default_mcp_protocol_version() -> String {
    "2024-11-05".to_string()
}

fn default_mcp_server_enabled() -> bool {
    false
}

fn default_mcp_server_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_mcp_server_port() -> u16 {
    3000
}

fn default_mcp_server_transport() -> McpServerTransport {
    McpServerTransport::Sse
}

fn default_mcp_server_name() -> String {
    "vtcode-mcp-server".to_string()
}

fn default_mcp_server_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_config_defaults() {
        let config = McpClientConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.ui.mode, McpUiMode::Compact);
        assert_eq!(config.ui.max_events, 50);
        assert!(config.ui.show_provider_names);
        assert_eq!(config.max_concurrent_connections, 5);
        assert_eq!(config.request_timeout_seconds, 30);
        assert_eq!(config.retry_attempts, 3);
        assert!(config.providers.is_empty());
        assert!(!config.server.enabled);
    }
}
