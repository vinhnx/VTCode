//! ACP capabilities and initialization types
//!
//! This module implements the capability negotiation as defined by ACP:
//! - Protocol version negotiation
//! - Feature capability exchange
//! - Agent information structures
//!
//! Reference: https://agentclientprotocol.com/llms.txt

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Current ACP protocol version supported by this implementation
pub const PROTOCOL_VERSION: &str = "2025-01-01";

/// Supported protocol versions (newest first)
pub const SUPPORTED_VERSIONS: &[&str] = &["2025-01-01", "2024-11-01"];

// ============================================================================
// Initialize Request/Response
// ============================================================================

/// Parameters for the initialize method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Protocol versions the client supports (newest first)
    pub protocol_versions: Vec<String>,

    /// Client capabilities
    pub capabilities: ClientCapabilities,

    /// Client information
    pub client_info: ClientInfo,
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self {
            protocol_versions: SUPPORTED_VERSIONS.iter().map(|s| s.to_string()).collect(),
            capabilities: ClientCapabilities::default(),
            client_info: ClientInfo::default(),
        }
    }
}

/// Result of the initialize method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Negotiated protocol version
    pub protocol_version: String,

    /// Agent capabilities
    pub capabilities: AgentCapabilities,

    /// Agent information
    pub agent_info: AgentInfo,

    /// Authentication requirements (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_requirements: Option<AuthRequirements>,
}

// ============================================================================
// Client Capabilities
// ============================================================================

/// Capabilities the client (IDE/host) provides to the agent
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    /// File system operations
    #[serde(default)]
    pub filesystem: FilesystemCapabilities,

    /// Terminal/shell capabilities
    #[serde(default)]
    pub terminal: TerminalCapabilities,

    /// UI/notification capabilities
    #[serde(default)]
    pub ui: UiCapabilities,

    /// MCP server connections the client can provide
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mcp_servers: Vec<McpServerCapability>,

    /// Extension points for custom capabilities
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extensions: HashMap<String, Value>,
}

/// File system operation capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemCapabilities {
    /// Can read files
    #[serde(default)]
    pub read: bool,

    /// Can write files
    #[serde(default)]
    pub write: bool,

    /// Can list directories
    #[serde(default)]
    pub list: bool,

    /// Can search files (grep/find)
    #[serde(default)]
    pub search: bool,

    /// Can watch for file changes
    #[serde(default)]
    pub watch: bool,
}

/// Terminal operation capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCapabilities {
    /// Can create terminal sessions
    #[serde(default)]
    pub create: bool,

    /// Can send input to terminals
    #[serde(default)]
    pub input: bool,

    /// Can read terminal output
    #[serde(default)]
    pub output: bool,

    /// Supports PTY (pseudo-terminal)
    #[serde(default)]
    pub pty: bool,
}

/// UI/notification capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCapabilities {
    /// Can show notifications
    #[serde(default)]
    pub notifications: bool,

    /// Can show progress indicators
    #[serde(default)]
    pub progress: bool,

    /// Can prompt for user input
    #[serde(default)]
    pub input_prompt: bool,

    /// Can show file diffs
    #[serde(default)]
    pub diff_view: bool,
}

/// MCP server connection capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCapability {
    /// Server name/identifier
    pub name: String,

    /// Server transport type (stdio, http, sse)
    pub transport: String,

    /// Tools this server provides
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
}

// ============================================================================
// Agent Capabilities
// ============================================================================

/// Capabilities the agent provides
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    /// Available tools
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolCapability>,

    /// Supported features
    #[serde(default)]
    pub features: AgentFeatures,

    /// Model information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelInfo>,

    /// Extension points
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extensions: HashMap<String, Value>,
}

/// A tool the agent can execute
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapability {
    /// Tool name
    pub name: String,

    /// Tool description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Input schema (JSON Schema)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,

    /// Whether tool requires user confirmation
    #[serde(default)]
    pub requires_confirmation: bool,
}

/// Agent feature flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFeatures {
    /// Supports streaming responses
    #[serde(default)]
    pub streaming: bool,

    /// Supports multi-turn conversations
    #[serde(default)]
    pub multi_turn: bool,

    /// Supports session persistence
    #[serde(default)]
    pub session_persistence: bool,

    /// Supports image/vision input
    #[serde(default)]
    pub vision: bool,

    /// Supports code execution
    #[serde(default)]
    pub code_execution: bool,

    /// Supports subagent spawning
    #[serde(default)]
    pub subagents: bool,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Model identifier
    pub id: String,

    /// Model name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Provider name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Context window size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
}

// ============================================================================
// Client/Agent Info
// ============================================================================

/// Information about the client (IDE/host)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Client name
    pub name: String,

    /// Client version
    pub version: String,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl Default for ClientInfo {
    fn default() -> Self {
        Self {
            name: "vtcode".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            metadata: HashMap::new(),
        }
    }
}

/// Information about the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent name
    pub name: String,

    /// Agent version
    pub version: String,

    /// Agent description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl Default for AgentInfo {
    fn default() -> Self {
        Self {
            name: "vtcode-agent".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: Some("VT Code AI coding agent".to_string()),
            metadata: HashMap::new(),
        }
    }
}

// ============================================================================
// Authentication
// ============================================================================

/// Authentication requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthRequirements {
    /// Whether authentication is required
    pub required: bool,

    /// Supported authentication methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<AuthMethod>,
}

/// Supported authentication methods
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// API key authentication
    ApiKey,
    /// OAuth 2.0
    OAuth2,
    /// Bearer token
    Bearer,
    /// Custom authentication
    Custom(String),
}

/// Parameters for authenticate method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    /// Authentication method being used
    pub method: AuthMethod,

    /// Authentication credentials
    pub credentials: AuthCredentials,
}

/// Authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthCredentials {
    /// API key
    ApiKey { key: String },

    /// Bearer token
    Bearer { token: String },

    /// OAuth2 token
    OAuth2 {
        access_token: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
    },
}

/// Result of authenticate method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateResult {
    /// Whether authentication succeeded
    pub authenticated: bool,

    /// Session token (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,

    /// Token expiration (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_params_default() {
        let params = InitializeParams::default();
        assert!(!params.protocol_versions.is_empty());
        assert!(
            params
                .protocol_versions
                .contains(&PROTOCOL_VERSION.to_string())
        );
    }

    #[test]
    fn test_client_info_default() {
        let info = ClientInfo::default();
        assert_eq!(info.name, "vtcode");
        assert!(!info.version.is_empty());
    }

    #[test]
    fn test_capabilities_serialization() {
        let caps = ClientCapabilities {
            filesystem: FilesystemCapabilities {
                read: true,
                write: true,
                list: true,
                search: true,
                watch: false,
            },
            terminal: TerminalCapabilities {
                create: true,
                input: true,
                output: true,
                pty: true,
            },
            ..Default::default()
        };

        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["filesystem"]["read"], true);
        assert_eq!(json["terminal"]["pty"], true);
    }

    #[test]
    fn test_auth_credentials() {
        let creds = AuthCredentials::ApiKey {
            key: "sk-test123".to_string(),
        };
        let json = serde_json::to_value(&creds).unwrap();
        assert_eq!(json["type"], "api_key");
        assert_eq!(json["key"], "sk-test123");
    }
}
