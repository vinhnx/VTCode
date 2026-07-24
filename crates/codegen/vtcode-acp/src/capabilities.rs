//! ACP capabilities and initialization types
//!
//! This module implements the capability negotiation as defined by ACP:
//! - Protocol version negotiation
//! - Feature capability exchange
//! - Agent information structures
//!
//! Reference: <https://agentclientprotocol.com/llms.txt>

use hashbrown::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Current ACP protocol version supported by this implementation
pub(crate) const PROTOCOL_VERSION: &str = "2025-01-01";

/// Supported protocol versions (newest first)
pub(crate) const SUPPORTED_VERSIONS: &[&str] = &["2025-01-01", "2024-11-01"];

// ============================================================================
// Initialize Request/Response
// ============================================================================

/// Parameters for the initialize method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// Protocol versions the client supports (newest first)
    pub(crate) protocol_versions: Vec<String>,

    /// Client capabilities
    pub(crate) capabilities: ClientCapabilities,

    /// Client information
    pub(crate) client_info: ClientInfo,
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
    pub(crate) protocol_version: String,

    /// Agent capabilities
    pub(crate) capabilities: AgentCapabilities,

    /// Agent information
    pub(crate) agent_info: AgentInfo,

    /// Authentication requirements (if any)
    #[serde(skip_serializing_if = "Option::is_none")]
    auth_requirements: Option<AuthRequirements>,
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
    filesystem: FilesystemCapabilities,

    /// Terminal/shell capabilities
    #[serde(default)]
    terminal: TerminalCapabilities,

    /// UI/notification capabilities
    #[serde(default)]
    ui: UiCapabilities,

    /// MCP server connections the client can provide
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mcp_servers: Vec<McpServerCapability>,

    /// Extension points for custom capabilities
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    extensions: HashMap<String, Value>,
}

/// File system operation capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemCapabilities {
    /// Can read files
    #[serde(default)]
    read: bool,

    /// Can write files
    #[serde(default)]
    write: bool,

    /// Can list directories
    #[serde(default)]
    list: bool,

    /// Can search files (grep/find)
    #[serde(default)]
    search: bool,

    /// Can watch for file changes
    #[serde(default)]
    watch: bool,
}

/// Terminal operation capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCapabilities {
    /// Can create terminal sessions
    #[serde(default)]
    create: bool,

    /// Can send input to terminals
    #[serde(default)]
    input: bool,

    /// Can read terminal output
    #[serde(default)]
    output: bool,

    /// Supports PTY (pseudo-terminal)
    #[serde(default)]
    pty: bool,
}

/// UI/notification capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiCapabilities {
    /// Can show notifications
    #[serde(default)]
    notifications: bool,

    /// Can show progress indicators
    #[serde(default)]
    progress: bool,

    /// Can prompt for user input
    #[serde(default)]
    input_prompt: bool,

    /// Can show file diffs
    #[serde(default)]
    diff_view: bool,
}

/// MCP server connection capability
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerCapability {
    /// Server name/identifier
    name: String,

    /// Server transport type (stdio, http, sse)
    transport: String,

    /// Tools this server provides
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tools: Vec<String>,
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
    tools: Vec<ToolCapability>,

    /// Supported features
    #[serde(default)]
    features: AgentFeatures,

    /// Model information
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<ModelInfo>,

    /// Extension points
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    extensions: HashMap<String, Value>,
}

/// A tool the agent can execute
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCapability {
    /// Tool name
    name: String,

    /// Tool description
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    /// Input schema (JSON Schema)
    #[serde(skip_serializing_if = "Option::is_none")]
    input_schema: Option<Value>,

    /// Whether tool requires user confirmation
    #[serde(default)]
    requires_confirmation: bool,
}

/// Agent feature flags
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentFeatures {
    /// Supports streaming responses
    #[serde(default)]
    streaming: bool,

    /// Supports multi-turn conversations
    #[serde(default)]
    multi_turn: bool,

    /// Supports session persistence
    #[serde(default)]
    session_persistence: bool,

    /// Supports image/vision input
    #[serde(default)]
    vision: bool,

    /// Supports code execution
    #[serde(default)]
    code_execution: bool,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    /// Model identifier
    id: String,

    /// Model name
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// Provider name
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<String>,

    /// Context window size
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<u32>,
}

// ============================================================================
// Client/Agent Info
// ============================================================================

/// Information about the client (IDE/host)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    /// Client name
    name: String,

    /// Client version
    version: String,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,
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
    pub(crate) name: String,

    /// Agent version
    version: String,

    /// Agent description
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,

    /// Additional metadata
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, Value>,
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
    required: bool,

    /// Supported authentication methods
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    methods: Vec<AuthMethod>,
}

/// Supported authentication methods
///
/// Follows ACP authentication specification:
/// <https://agentclientprotocol.com/protocol/auth>
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Agent handles authentication itself (default/backward-compatible)
    #[serde(rename = "agent")]
    Agent {
        /// Unique identifier for this auth method
        id: String,
        /// Human-readable name
        name: String,
        /// Description of the auth method
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },

    /// Environment variable-based authentication
    /// User provides a key/credential that client passes as environment variable
    #[serde(rename = "env_var")]
    EnvVar {
        /// Unique identifier for this auth method
        id: String,
        /// Human-readable name
        name: String,
        /// Description of the auth method
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Environment variable name to set
        var_name: String,
        /// Optional link to page where user can get their key
        #[serde(skip_serializing_if = "Option::is_none")]
        link: Option<String>,
    },

    /// Terminal/TUI-based interactive authentication
    /// Client launches interactive terminal for user to login
    #[serde(rename = "terminal")]
    Terminal {
        /// Unique identifier for this auth method
        id: String,
        /// Human-readable name
        name: String,
        /// Description of the auth method
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// Additional arguments to pass to agent command
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
        /// Additional environment variables to set
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        env: HashMap<String, String>,
    },

    /// Legacy: API key authentication (deprecated, use EnvVar instead)
    #[serde(rename = "api_key")]
    ApiKey,

    /// Legacy: OAuth 2.0 (use Terminal for interactive flows)
    #[serde(rename = "oauth2")]
    OAuth2,

    /// Legacy: Bearer token authentication
    #[serde(rename = "bearer")]
    Bearer,

    /// Custom authentication (agent-specific)
    #[serde(rename = "custom")]
    Custom(String),
}

/// Parameters for authenticate method
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    /// Authentication method being used
    method: AuthMethod,

    /// Authentication credentials
    credentials: AuthCredentials,
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
    pub(crate) authenticated: bool,

    /// Session token (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) session_token: Option<String>,

    /// Token expiration (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_params_default() {
        let params = InitializeParams::default();
        assert!(!params.protocol_versions.is_empty());
        assert!(params.protocol_versions.contains(&PROTOCOL_VERSION.to_string()));
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
            terminal: TerminalCapabilities { create: true, input: true, output: true, pty: true },
            ..Default::default()
        };

        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["filesystem"]["read"], true);
        assert_eq!(json["terminal"]["pty"], true);
    }

    #[test]
    fn test_auth_credentials() {
        let creds = AuthCredentials::ApiKey { key: "sk-test123".to_string() };
        let json = serde_json::to_value(&creds).unwrap();
        assert_eq!(json["type"], "api_key");
        assert_eq!(json["key"], "sk-test123");
    }

    #[test]
    fn test_auth_method_agent() {
        let method = AuthMethod::Agent {
            id: "agent_auth".to_string(),
            name: "Agent Authentication".to_string(),
            description: Some("Let agent handle authentication".to_string()),
        };
        let json = serde_json::to_value(&method).unwrap();
        assert_eq!(json["type"], "agent");
        assert_eq!(json["id"], "agent_auth");
        assert_eq!(json["name"], "Agent Authentication");
    }

    #[test]
    fn test_auth_method_env_var() {
        let method = AuthMethod::EnvVar {
            id: "openai_key".to_string(),
            name: "OpenAI API Key".to_string(),
            description: Some("Provide your OpenAI API key".to_string()),
            var_name: "OPENAI_API_KEY".to_string(),
            link: Some("https://platform.openai.com/api-keys".to_string()),
        };
        let json = serde_json::to_value(&method).unwrap();
        assert_eq!(json["type"], "env_var");
        assert_eq!(json["id"], "openai_key");
        assert_eq!(json["name"], "OpenAI API Key");
        assert_eq!(json["var_name"], "OPENAI_API_KEY");
        assert_eq!(json["link"], "https://platform.openai.com/api-keys");
    }

    #[test]
    fn test_auth_method_terminal() {
        let mut env = HashMap::new();
        env.insert("VAR1".to_string(), "value1".to_string());

        let method = AuthMethod::Terminal {
            id: "terminal_login".to_string(),
            name: "Terminal Login".to_string(),
            description: Some("Login via interactive terminal".to_string()),
            args: vec!["--login".to_string(), "--interactive".to_string()],
            env,
        };
        let json = serde_json::to_value(&method).unwrap();
        assert_eq!(json["type"], "terminal");
        assert_eq!(json["args"][0], "--login");
        assert_eq!(json["env"]["VAR1"], "value1");
    }

    #[test]
    fn test_auth_method_serialization_roundtrip() {
        let method = AuthMethod::EnvVar {
            id: "test_id".to_string(),
            name: "Test".to_string(),
            description: None,
            var_name: "TEST_VAR".to_string(),
            link: None,
        };

        let json = serde_json::to_value(&method).unwrap();
        let deserialized: AuthMethod = serde_json::from_value(json).unwrap();

        match deserialized {
            AuthMethod::EnvVar { id, name, var_name, .. } => {
                assert_eq!(id, "test_id");
                assert_eq!(name, "Test");
                assert_eq!(var_name, "TEST_VAR");
            }
            _ => panic!("Unexpected auth method variant"),
        }
    }

    #[test]
    fn test_legacy_auth_methods() {
        // Ensure backward compatibility
        let json = serde_json::json!({"type": "api_key"});
        let method: AuthMethod = serde_json::from_value(json).unwrap();
        matches!(method, AuthMethod::ApiKey);

        let json = serde_json::json!({"type": "oauth2"});
        let method: AuthMethod = serde_json::from_value(json).unwrap();
        matches!(method, AuthMethod::OAuth2);

        let json = serde_json::json!({"type": "bearer"});
        let method: AuthMethod = serde_json::from_value(json).unwrap();
        matches!(method, AuthMethod::Bearer);
    }
}
