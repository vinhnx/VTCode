use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

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

    /// Allow list configuration for MCP access control
    #[serde(default)]
    pub allowlist: McpAllowListConfig,

    /// Maximum number of concurrent MCP connections
    #[serde(default = "default_max_concurrent_connections")]
    pub max_concurrent_connections: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout_seconds")]
    pub request_timeout_seconds: u64,

    /// Connection retry attempts
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,

    /// Optional timeout (seconds) when starting providers
    #[serde(default)]
    pub startup_timeout_seconds: Option<u64>,

    /// Optional timeout (seconds) for tool execution
    #[serde(default)]
    pub tool_timeout_seconds: Option<u64>,

    /// Toggle experimental RMCP client features
    #[serde(default = "default_experimental_use_rmcp_client")]
    pub experimental_use_rmcp_client: bool,
}

impl Default for McpClientConfig {
    fn default() -> Self {
        Self {
            enabled: default_mcp_enabled(),
            ui: McpUiConfig::default(),
            providers: Vec::new(),
            server: McpServerConfig::default(),
            allowlist: McpAllowListConfig::default(),
            max_concurrent_connections: default_max_concurrent_connections(),
            request_timeout_seconds: default_request_timeout_seconds(),
            retry_attempts: default_retry_attempts(),
            startup_timeout_seconds: None,
            tool_timeout_seconds: None,
            experimental_use_rmcp_client: default_experimental_use_rmcp_client(),
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

    /// Custom renderer profiles for provider-specific output formatting
    #[serde(default)]
    pub renderers: HashMap<String, McpRendererProfile>,
}

impl Default for McpUiConfig {
    fn default() -> Self {
        Self {
            mode: default_mcp_ui_mode(),
            max_events: default_max_mcp_events(),
            show_provider_names: default_show_provider_names(),
            renderers: HashMap::new(),
        }
    }
}

impl McpUiConfig {
    /// Resolve renderer profile for a provider or tool identifier
    pub fn renderer_for_identifier(&self, identifier: &str) -> Option<McpRendererProfile> {
        let normalized_identifier = normalize_mcp_identifier(identifier);
        if normalized_identifier.is_empty() {
            return None;
        }

        self.renderers.iter().find_map(|(key, profile)| {
            let normalized_key = normalize_mcp_identifier(key);
            if normalized_identifier.starts_with(&normalized_key) {
                Some(*profile)
            } else {
                None
            }
        })
    }

    /// Resolve renderer profile for a fully qualified tool name
    pub fn renderer_for_tool(&self, tool_name: &str) -> Option<McpRendererProfile> {
        let identifier = tool_name.strip_prefix("mcp_").unwrap_or(tool_name);
        self.renderer_for_identifier(identifier)
    }
}

/// UI mode for MCP event display
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum McpUiMode {
    /// Compact mode - shows only event titles
    #[default]
    Compact,
    /// Full mode - shows detailed event logs
    Full,
}

impl std::fmt::Display for McpUiMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpUiMode::Compact => write!(f, "compact"),
            McpUiMode::Full => write!(f, "full"),
        }
    }
}

/// Named renderer profiles for MCP tool output formatting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum McpRendererProfile {
    /// Context7 knowledge base renderer
    Context7,
    /// Sequential thinking trace renderer
    SequentialThinking,
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

/// Allow list configuration for MCP providers
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpAllowListConfig {
    /// Whether to enforce allow list checks
    #[serde(default = "default_allowlist_enforced")]
    pub enforce: bool,

    /// Default rules applied when provider-specific rules are absent
    #[serde(default)]
    pub default: McpAllowListRules,

    /// Provider-specific allow list rules
    #[serde(default)]
    pub providers: BTreeMap<String, McpAllowListRules>,
}

impl Default for McpAllowListConfig {
    fn default() -> Self {
        Self {
            enforce: default_allowlist_enforced(),
            default: McpAllowListRules::default(),
            providers: BTreeMap::new(),
        }
    }
}

impl McpAllowListConfig {
    /// Determine whether a tool is permitted for the given provider
    pub fn is_tool_allowed(&self, provider: &str, tool_name: &str) -> bool {
        if !self.enforce {
            return true;
        }

        self.resolve_match(provider, tool_name, |rules| &rules.tools)
    }

    /// Determine whether a resource is permitted for the given provider
    pub fn is_resource_allowed(&self, provider: &str, resource: &str) -> bool {
        if !self.enforce {
            return true;
        }

        self.resolve_match(provider, resource, |rules| &rules.resources)
    }

    /// Determine whether a prompt is permitted for the given provider
    pub fn is_prompt_allowed(&self, provider: &str, prompt: &str) -> bool {
        if !self.enforce {
            return true;
        }

        self.resolve_match(provider, prompt, |rules| &rules.prompts)
    }

    /// Determine whether a logging channel is permitted
    pub fn is_logging_channel_allowed(&self, provider: Option<&str>, channel: &str) -> bool {
        if !self.enforce {
            return true;
        }

        if let Some(name) = provider
            && let Some(rules) = self.providers.get(name)
            && let Some(patterns) = &rules.logging
        {
            return pattern_matches(patterns, channel);
        }

        if let Some(patterns) = &self.default.logging
            && pattern_matches(patterns, channel)
        {
            return true;
        }

        false
    }

    /// Determine whether a configuration key can be modified
    pub fn is_configuration_allowed(
        &self,
        provider: Option<&str>,
        category: &str,
        key: &str,
    ) -> bool {
        if !self.enforce {
            return true;
        }

        if let Some(name) = provider
            && let Some(rules) = self.providers.get(name)
            && let Some(result) = configuration_allowed(rules, category, key)
        {
            return result;
        }

        if let Some(result) = configuration_allowed(&self.default, category, key) {
            return result;
        }

        false
    }

    fn resolve_match<'a, F>(&'a self, provider: &str, candidate: &str, accessor: F) -> bool
    where
        F: Fn(&'a McpAllowListRules) -> &'a Option<Vec<String>>,
    {
        if let Some(rules) = self.providers.get(provider)
            && let Some(patterns) = accessor(rules)
        {
            return pattern_matches(patterns, candidate);
        }

        if let Some(patterns) = accessor(&self.default)
            && pattern_matches(patterns, candidate)
        {
            return true;
        }

        false
    }
}

fn configuration_allowed(rules: &McpAllowListRules, category: &str, key: &str) -> Option<bool> {
    rules.configuration.as_ref().and_then(|entries| {
        entries
            .get(category)
            .map(|patterns| pattern_matches(patterns, key))
    })
}

fn pattern_matches(patterns: &[String], candidate: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| wildcard_match(pattern, candidate))
}

fn wildcard_match(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let mut regex_pattern = String::from("^");
    let mut literal_buffer = String::new();

    for ch in pattern.chars() {
        match ch {
            '*' => {
                if !literal_buffer.is_empty() {
                    regex_pattern.push_str(&regex::escape(&literal_buffer));
                    literal_buffer.clear();
                }
                regex_pattern.push_str(".*");
            }
            '?' => {
                if !literal_buffer.is_empty() {
                    regex_pattern.push_str(&regex::escape(&literal_buffer));
                    literal_buffer.clear();
                }
                regex_pattern.push('.');
            }
            _ => literal_buffer.push(ch),
        }
    }

    if !literal_buffer.is_empty() {
        regex_pattern.push_str(&regex::escape(&literal_buffer));
    }

    regex_pattern.push('$');

    Regex::new(&regex_pattern)
        .map(|regex| regex.is_match(candidate))
        .unwrap_or(false)
}

/// Allow list rules for a provider or default configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpAllowListRules {
    /// Tool name patterns permitted for the provider
    #[serde(default)]
    pub tools: Option<Vec<String>>,

    /// Resource name patterns permitted for the provider
    #[serde(default)]
    pub resources: Option<Vec<String>>,

    /// Prompt name patterns permitted for the provider
    #[serde(default)]
    pub prompts: Option<Vec<String>>,

    /// Logging channels permitted for the provider
    #[serde(default)]
    pub logging: Option<Vec<String>>,

    /// Configuration keys permitted for the provider grouped by category
    #[serde(default)]
    pub configuration: Option<BTreeMap<String, Vec<String>>>,
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
#[derive(Default)]
pub enum McpServerTransport {
    /// Server Sent Events transport
    #[default]
    Sse,
    /// HTTP transport
    Http,
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
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct McpStdioServerConfig {
    /// Command to execute
    pub command: String,

    /// Command arguments
    pub args: Vec<String>,

    /// Working directory for the command
    #[serde(default)]
    pub working_directory: Option<String>,
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

fn default_experimental_use_rmcp_client() -> bool {
    true
}

fn default_provider_enabled() -> bool {
    true
}

fn default_provider_max_concurrent() -> usize {
    3
}

fn default_allowlist_enforced() -> bool {
    false
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

fn normalize_mcp_identifier(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::mcp as mcp_constants;
    use std::collections::BTreeMap;

    #[test]
    fn test_mcp_config_defaults() {
        let config = McpClientConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.ui.mode, McpUiMode::Compact);
        assert_eq!(config.ui.max_events, 50);
        assert!(config.ui.show_provider_names);
        assert!(config.ui.renderers.is_empty());
        assert_eq!(config.max_concurrent_connections, 5);
        assert_eq!(config.request_timeout_seconds, 30);
        assert_eq!(config.retry_attempts, 3);
        assert!(config.providers.is_empty());
        assert!(!config.server.enabled);
        assert!(!config.allowlist.enforce);
        assert!(config.allowlist.default.tools.is_none());
    }

    #[test]
    fn test_allowlist_pattern_matching() {
        let patterns = vec!["get_*".to_string(), "convert_timezone".to_string()];
        assert!(pattern_matches(&patterns, "get_current_time"));
        assert!(pattern_matches(&patterns, "convert_timezone"));
        assert!(!pattern_matches(&patterns, "delete_timezone"));
    }

    #[test]
    fn test_allowlist_provider_override() {
        let mut config = McpAllowListConfig::default();
        config.enforce = true;
        config.default.tools = Some(vec!["get_*".to_string()]);

        let mut provider_rules = McpAllowListRules::default();
        provider_rules.tools = Some(vec!["list_*".to_string()]);
        config
            .providers
            .insert("context7".to_string(), provider_rules);

        assert!(config.is_tool_allowed("context7", "list_documents"));
        assert!(!config.is_tool_allowed("context7", "get_current_time"));
        assert!(config.is_tool_allowed("other", "get_timezone"));
        assert!(!config.is_tool_allowed("other", "list_documents"));
    }

    #[test]
    fn test_allowlist_configuration_rules() {
        let mut config = McpAllowListConfig::default();
        config.enforce = true;

        let mut default_rules = McpAllowListRules::default();
        default_rules.configuration = Some(BTreeMap::from([(
            "ui".to_string(),
            vec!["mode".to_string(), "max_events".to_string()],
        )]));
        config.default = default_rules;

        let mut provider_rules = McpAllowListRules::default();
        provider_rules.configuration = Some(BTreeMap::from([(
            "provider".to_string(),
            vec!["max_concurrent_requests".to_string()],
        )]));
        config.providers.insert("time".to_string(), provider_rules);

        assert!(config.is_configuration_allowed(None, "ui", "mode"));
        assert!(!config.is_configuration_allowed(None, "ui", "show_provider_names"));
        assert!(config.is_configuration_allowed(
            Some("time"),
            "provider",
            "max_concurrent_requests"
        ));
        assert!(!config.is_configuration_allowed(Some("time"), "provider", "retry_attempts"));
    }

    #[test]
    fn test_allowlist_resource_override() {
        let mut config = McpAllowListConfig::default();
        config.enforce = true;
        config.default.resources = Some(vec!["docs/*".to_string()]);

        let mut provider_rules = McpAllowListRules::default();
        provider_rules.resources = Some(vec!["journals/*".to_string()]);
        config
            .providers
            .insert("context7".to_string(), provider_rules);

        assert!(config.is_resource_allowed("context7", "journals/2024"));
        assert!(!config.is_resource_allowed("context7", "docs/manual"));
        assert!(config.is_resource_allowed("other", "docs/reference"));
        assert!(!config.is_resource_allowed("other", "journals/2023"));
    }

    #[test]
    fn test_allowlist_logging_override() {
        let mut config = McpAllowListConfig::default();
        config.enforce = true;
        config.default.logging = Some(vec!["info".to_string(), "debug".to_string()]);

        let mut provider_rules = McpAllowListRules::default();
        provider_rules.logging = Some(vec!["audit".to_string()]);
        config
            .providers
            .insert("sequential".to_string(), provider_rules);

        assert!(config.is_logging_channel_allowed(Some("sequential"), "audit"));
        assert!(!config.is_logging_channel_allowed(Some("sequential"), "info"));
        assert!(config.is_logging_channel_allowed(Some("other"), "info"));
        assert!(!config.is_logging_channel_allowed(Some("other"), "trace"));
    }

    #[test]
    fn test_mcp_ui_renderer_resolution() {
        let mut config = McpUiConfig::default();
        config.renderers.insert(
            mcp_constants::RENDERER_CONTEXT7.to_string(),
            McpRendererProfile::Context7,
        );
        config.renderers.insert(
            mcp_constants::RENDERER_SEQUENTIAL_THINKING.to_string(),
            McpRendererProfile::SequentialThinking,
        );

        assert_eq!(
            config.renderer_for_tool("mcp_context7_lookup"),
            Some(McpRendererProfile::Context7)
        );
        assert_eq!(
            config.renderer_for_tool("mcp_context7lookup"),
            Some(McpRendererProfile::Context7)
        );
        assert_eq!(
            config.renderer_for_tool("mcp_sequentialthinking_run"),
            Some(McpRendererProfile::SequentialThinking)
        );
        assert_eq!(
            config.renderer_for_identifier("sequential-thinking-analyze"),
            Some(McpRendererProfile::SequentialThinking)
        );
        assert_eq!(config.renderer_for_tool("mcp_unknown"), None);
    }
}
