/// Unified error handling for MCP operations
///
/// VT Code uses `anyhow::Result<T>` for all MCP errors to maintain consistency
/// with the Rust SDK patterns and provide rich error context.
///
/// Phase 3: Error codes follow the pattern MCP_E{code}
/// - MCP_E001-E010: Tool-related errors
/// - MCP_E011-E020: Provider-related errors
/// - MCP_E021-E030: Schema-related errors
/// - MCP_E031-E040: Configuration-related errors
use anyhow::anyhow;
use std::fmt;

pub type McpResult<T> = anyhow::Result<T>;

/// MCP Error codes for better error identification and debugging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// MCP_E001: Tool not found
    ToolNotFound = 1,
    /// MCP_E002: Tool invocation failed
    ToolInvocationFailed = 2,
    /// MCP_E011: Provider not found
    ProviderNotFound = 11,
    /// MCP_E012: Provider unavailable
    ProviderUnavailable = 12,
    /// MCP_E021: Schema validation failed
    SchemaInvalid = 21,
    /// MCP_E031: Configuration error
    ConfigurationError = 31,
    /// MCP_E032: Initialization timeout
    InitializationTimeout = 32,
}

impl ErrorCode {
    /// Get error code string (e.g., "MCP_E001")
    pub fn code(&self) -> String {
        format!("MCP_E{:03}", *self as u32)
    }

    /// Get human-readable error name
    pub fn name(&self) -> &'static str {
        match self {
            Self::ToolNotFound => "ToolNotFound",
            Self::ToolInvocationFailed => "ToolInvocationFailed",
            Self::ProviderNotFound => "ProviderNotFound",
            Self::ProviderUnavailable => "ProviderUnavailable",
            Self::SchemaInvalid => "SchemaInvalid",
            Self::ConfigurationError => "ConfigurationError",
            Self::InitializationTimeout => "InitializationTimeout",
        }
    }

    /// Returns a short, actionable guidance message suitable for display in the TUI.
    pub fn user_guidance(&self) -> &'static str {
        match self {
            Self::ToolNotFound => {
                "Check that the tool name is correct and the MCP provider is running."
            }
            Self::ToolInvocationFailed => {
                "The MCP tool returned an error. Check the tool's arguments and provider logs."
            }
            Self::ProviderNotFound => {
                "Verify the provider name in vtcode.toml or .mcp.json matches a configured MCP server."
            }
            Self::ProviderUnavailable => {
                "The MCP server may be down. Check that the command/endpoint is reachable."
            }
            Self::SchemaInvalid => {
                "The tool's input schema does not match expected format. Check the MCP server implementation."
            }
            Self::ConfigurationError => {
                "Review the MCP section of vtcode.toml or .mcp.json for syntax errors."
            }
            Self::InitializationTimeout => {
                "The MCP server took too long to start. Increase startup_timeout_ms or check the server process."
            }
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

/// Helper to create a "tool not found" error
///
/// Error Code: MCP_E001
pub fn tool_not_found(name: &str) -> anyhow::Error {
    anyhow!(
        "[{}] MCP tool '{}' not found",
        ErrorCode::ToolNotFound.code(),
        name
    )
}

/// Helper to create a "provider not found" error
///
/// Error Code: MCP_E011
pub fn provider_not_found(name: &str) -> anyhow::Error {
    anyhow!(
        "[{}] MCP provider '{}' not found",
        ErrorCode::ProviderNotFound.code(),
        name
    )
}

/// Helper to create a "provider unavailable" error
///
/// Error Code: MCP_E012
pub fn provider_unavailable(name: &str) -> anyhow::Error {
    anyhow!(
        "[{}] MCP provider '{}' is unavailable or failed to initialize",
        ErrorCode::ProviderUnavailable.code(),
        name
    )
}

/// Helper to create a "schema invalid" error
///
/// Error Code: MCP_E021
pub fn schema_invalid(reason: &str) -> anyhow::Error {
    anyhow!(
        "[{}] MCP tool schema is invalid: {}",
        ErrorCode::SchemaInvalid.code(),
        reason
    )
}

/// Helper to create a "tool invocation failed" error
///
/// Error Code: MCP_E002
pub fn tool_invocation_failed(provider: &str, tool: &str, reason: &str) -> anyhow::Error {
    anyhow!(
        "[{}] Failed to invoke tool '{}' on provider '{}': {}",
        ErrorCode::ToolInvocationFailed.code(),
        tool,
        provider,
        reason
    )
}

/// Helper to create an "initialization timeout" error
///
/// Error Code: MCP_E032
pub fn initialization_timeout(timeout_secs: u64) -> anyhow::Error {
    anyhow!(
        "[{}] MCP initialization timeout after {} seconds",
        ErrorCode::InitializationTimeout.code(),
        timeout_secs
    )
}

/// Helper to create a "configuration error"
///
/// Error Code: MCP_E031
pub fn configuration_error(reason: &str) -> anyhow::Error {
    anyhow!(
        "[{}] MCP configuration error: {}",
        ErrorCode::ConfigurationError.code(),
        reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes_format() {
        assert_eq!(ErrorCode::ToolNotFound.code(), "MCP_E001");
        assert_eq!(ErrorCode::ToolInvocationFailed.code(), "MCP_E002");
        assert_eq!(ErrorCode::ProviderNotFound.code(), "MCP_E011");
        assert_eq!(ErrorCode::ProviderUnavailable.code(), "MCP_E012");
        assert_eq!(ErrorCode::SchemaInvalid.code(), "MCP_E021");
        assert_eq!(ErrorCode::ConfigurationError.code(), "MCP_E031");
        assert_eq!(ErrorCode::InitializationTimeout.code(), "MCP_E032");
    }

    #[test]
    fn test_error_names() {
        assert_eq!(ErrorCode::ToolNotFound.name(), "ToolNotFound");
        assert_eq!(ErrorCode::ProviderNotFound.name(), "ProviderNotFound");
        assert_eq!(
            ErrorCode::InitializationTimeout.name(),
            "InitializationTimeout"
        );
    }

    #[test]
    fn test_error_messages_with_codes() {
        let err = tool_not_found("missing_tool");
        let msg = err.to_string();
        assert!(msg.contains("[MCP_E001]"));
        assert!(msg.contains("missing_tool"));
        assert!(msg.contains("not found"));

        let err = provider_not_found("missing_provider");
        let msg = err.to_string();
        assert!(msg.contains("[MCP_E011]"));
        assert!(msg.contains("missing_provider"));

        let err = initialization_timeout(15);
        let msg = err.to_string();
        assert!(msg.contains("[MCP_E032]"));
        assert!(msg.contains("15 seconds"));

        let err = tool_invocation_failed("claude", "list_files", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("[MCP_E002]"));
        assert!(msg.contains("list_files"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(ErrorCode::ToolNotFound.to_string(), "MCP_E001");
        assert_eq!(ErrorCode::ProviderUnavailable.to_string(), "MCP_E012");
    }
}
