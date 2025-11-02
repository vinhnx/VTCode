//! Enhanced MCP Configuration
//!
//! This module provides enhanced configuration options for MCP with
//! improved validation, security features, and better error handling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Enhanced security configuration for MCP
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnhancedMcpSecurityConfig {
    /// Enable authentication for MCP server
    #[serde(default = "default_auth_enabled")]
    pub auth_enabled: bool,

    /// API key environment variable name
    #[serde(default)]
    pub api_key_env: Option<String>,

    /// Rate limiting configuration
    #[serde(default)]
    pub rate_limit: McpRateLimitConfig,

    /// Tool call validation configuration
    #[serde(default)]
    pub validation: McpValidationConfig,
}

impl Default for EnhancedMcpSecurityConfig {
    fn default() -> Self {
        Self {
            auth_enabled: default_auth_enabled(),
            api_key_env: None,
            rate_limit: McpRateLimitConfig::default(),
            validation: McpValidationConfig::default(),
        }
    }
}

/// Rate limiting configuration for MCP
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpRateLimitConfig {
    /// Maximum requests per minute per client
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,

    /// Maximum concurrent requests per client
    #[serde(default = "default_concurrent_requests")]
    pub concurrent_requests: u32,
}

impl Default for McpRateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: default_requests_per_minute(),
            concurrent_requests: default_concurrent_requests(),
        }
    }
}

/// Validation configuration for MCP
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct McpValidationConfig {
    /// Enable JSON schema validation for tool arguments
    #[serde(default = "default_schema_validation_enabled")]
    pub schema_validation_enabled: bool,

    /// Enable path traversal protection
    #[serde(default = "default_path_traversal_protection_enabled")]
    pub path_traversal_protection: bool,

    /// Maximum argument size in bytes
    #[serde(default = "default_max_argument_size")]
    pub max_argument_size: u32,
}

impl Default for McpValidationConfig {
    fn default() -> Self {
        Self {
            schema_validation_enabled: default_schema_validation_enabled(),
            path_traversal_protection: default_path_traversal_protection_enabled(),
            max_argument_size: default_max_argument_size(),
        }
    }
}

// Default functions
fn default_auth_enabled() -> bool {
    false
}

fn default_requests_per_minute() -> u32 {
    100
}

fn default_concurrent_requests() -> u32 {
    10
}

fn default_schema_validation_enabled() -> bool {
    true
}

fn default_path_traversal_protection_enabled() -> bool {
    true
}

fn default_max_argument_size() -> u32 {
    1024 * 1024 // 1MB
}

/// Enhanced MCP client configuration with validation
#[derive(Debug, Clone)]
pub struct ValidatedMcpClientConfig {
    /// Original configuration
    pub original: crate::config::mcp::McpClientConfig,
    /// Enhanced security configuration
    pub security: EnhancedMcpSecurityConfig,
}

impl ValidatedMcpClientConfig {
    /// Create a new validated configuration from the original
    pub fn new(original: crate::config::mcp::McpClientConfig) -> Self {
        let security = EnhancedMcpSecurityConfig::default();
        Self { original, security }
    }

    /// Validate the configuration and return any issues found
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        // Validate server configuration if enabled
        if self.original.server.enabled {
            // Validate port range
            if self.original.server.port == 0 || self.original.server.port > 65535 {
                errors.push(ValidationError::InvalidPort(self.original.server.port));
            }

            // Validate bind address
            if self.original.server.bind_address.is_empty() {
                errors.push(ValidationError::EmptyBindAddress);
            }

            // Validate security settings if auth is enabled
            if self.security.auth_enabled && self.security.api_key_env.is_none() {
                errors.push(ValidationError::MissingApiKeyEnv);
            }
        }

        // Validate timeouts
        if let Some(startup_timeout) = self.original.startup_timeout_seconds {
            if startup_timeout > 300 {
                // Max 5 minutes
                errors.push(ValidationError::InvalidStartupTimeout(startup_timeout));
            }
        }

        if let Some(tool_timeout) = self.original.tool_timeout_seconds {
            if tool_timeout > 3600 {
                // Max 1 hour
                errors.push(ValidationError::InvalidToolTimeout(tool_timeout));
            }
        }

        // Validate provider configurations
        for provider in &self.original.providers {
            if provider.name.is_empty() {
                errors.push(ValidationError::EmptyProviderName);
            }

            // Validate max_concurrent_requests
            if provider.max_concurrent_requests == 0 {
                errors.push(ValidationError::InvalidMaxConcurrentRequests(
                    provider.name.clone(),
                    provider.max_concurrent_requests,
                ));
            }
        }

        errors
    }

    /// Check if the configuration is valid
    pub fn is_valid(&self) -> bool {
        self.validate().is_empty()
    }

    /// Log any validation warnings
    pub fn log_warnings(&self) {
        let errors = self.validate();
        if !errors.is_empty() {
            warn!("MCP configuration validation issues found:");
            for error in errors {
                warn!("  - {}", error);
            }
        } else {
            debug!("MCP configuration validation passed");
        }
    }
}

/// Validation error types
#[derive(Debug, Clone)]
pub enum ValidationError {
    InvalidPort(u64),
    EmptyBindAddress,
    MissingApiKeyEnv,
    InvalidStartupTimeout(u64),
    InvalidToolTimeout(u64),
    EmptyProviderName,
    InvalidMaxConcurrentRequests(String, usize),
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidPort(port) => {
                write!(f, "Invalid server port: {}", port)
            }
            ValidationError::EmptyBindAddress => {
                write!(f, "Server bind address cannot be empty")
            }
            ValidationError::MissingApiKeyEnv => {
                write!(f, "API key environment variable must be set when auth is enabled")
            }
            ValidationError::InvalidStartupTimeout(timeout) => {
                write!(f, "Startup timeout cannot exceed 300 seconds: {}", timeout)
            }
            ValidationError::InvalidToolTimeout(timeout) => {
                write!(f, "Tool timeout cannot exceed 3600 seconds: {}", timeout)
            }
            ValidationError::EmptyProviderName => {
                write!(f, "MCP provider name cannot be empty")
            }
            ValidationError::InvalidMaxConcurrentRequests(name, count) => {
                write!(
                    f,
                    "Max concurrent requests must be greater than 0 for provider '{}': {}",
                    name, count
                )
            }
        }
    }
}

/// Enhanced tool configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EnhancedMcpToolConfig {
    /// Name of the tool to expose
    pub name: String,
    /// Whether the tool is enabled
    #[serde(default = "default_tool_enabled")]
    pub enabled: bool,
    /// Optional description override
    pub description: Option<String>,
    /// Rate limiting for this specific tool
    #[serde(default)]
    pub rate_limit: Option<McpRateLimitConfig>,
    /// Validation rules specific to this tool
    #[serde(default)]
    pub validation: Option<McpValidationConfig>,
}

fn default_tool_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::mcp::{
        McpClientConfig, McpProviderConfig, McpServerConfig, McpStdioServerConfig,
        McpTransportConfig,
    };

    fn create_test_config() -> McpClientConfig {
        McpClientConfig {
            enabled: true,
            ui: Default::default(),
            providers: vec![McpProviderConfig {
                name: "test_provider".to_string(),
                transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                    command: "test_command".to_string(),
                    args: vec![],
                    working_directory: None,
                }),
                env: HashMap::new(),
                enabled: true,
                max_concurrent_requests: 5,
                startup_timeout_ms: None,
            }],
            server: McpServerConfig {
                enabled: true,
                bind_address: "127.0.0.1".to_string(),
                port: 3000,
                transport: crate::config::mcp::McpServerTransport::Sse,
                name: "test_server".to_string(),
                version: "1.0.0".to_string(),
                exposed_tools: vec![],
            },
            allowlist: Default::default(),
            max_concurrent_connections: 10,
            request_timeout_seconds: 30,
            retry_attempts: 3,
            startup_timeout_seconds: Some(60),
            tool_timeout_seconds: Some(300),
            experimental_use_rmcp_client: false,
        }
    }

    #[test]
    fn test_validated_config_creation() {
        let original = create_test_config();
        let validated = ValidatedMcpClientConfig::new(original);
        assert!(validated.is_valid());
    }

    #[test]
    fn test_invalid_port_validation() {
        let mut original = create_test_config();
        original.server.port = 70000; // Invalid port
        let validated = ValidatedMcpClientConfig::new(original);
        assert!(!validated.is_valid());
    }

    #[test]
    fn test_empty_bind_address_validation() {
        let mut original = create_test_config();
        original.server.bind_address = String::new(); // Empty bind address
        let validated = ValidatedMcpClientConfig::new(original);
        assert!(!validated.is_valid());
    }

    #[test]
    fn test_timeout_validation() {
        let mut original = create_test_config();
        original.startup_timeout_seconds = Some(400); // Too long
        let validated = ValidatedMcpClientConfig::new(original);
        assert!(!validated.is_valid());
    }

    #[test]
    fn test_zero_concurrent_requests_validation() {
        let mut original = create_test_config();
        original.providers[0].max_concurrent_requests = 0; // Invalid
        let validated = ValidatedMcpClientConfig::new(original);
        assert!(!validated.is_valid());
    }
}