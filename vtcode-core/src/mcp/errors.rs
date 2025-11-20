/// Unified error handling for MCP operations
///
/// VTCode uses `anyhow::Result<T>` for all MCP errors to maintain consistency
/// with the Rust SDK patterns and provide rich error context.

use anyhow::anyhow;

pub type McpResult<T> = anyhow::Result<T>;

/// Helper to create a "tool not found" error
pub fn tool_not_found(name: &str) -> anyhow::Error {
    anyhow!("MCP tool '{}' not found", name)
}

/// Helper to create a "provider not found" error
pub fn provider_not_found(name: &str) -> anyhow::Error {
    anyhow!("MCP provider '{}' not found", name)
}

/// Helper to create a "provider unavailable" error
pub fn provider_unavailable(name: &str) -> anyhow::Error {
    anyhow!("MCP provider '{}' is unavailable or failed to initialize", name)
}

/// Helper to create a "schema invalid" error
pub fn schema_invalid(reason: &str) -> anyhow::Error {
    anyhow!("MCP tool schema is invalid: {}", reason)
}

/// Helper to create a "tool invocation failed" error
pub fn tool_invocation_failed(provider: &str, tool: &str, reason: &str) -> anyhow::Error {
    anyhow!("Failed to invoke tool '{}' on provider '{}': {}", tool, provider, reason)
}

/// Helper to create an "initialization timeout" error
pub fn initialization_timeout(timeout_secs: u64) -> anyhow::Error {
    anyhow!("MCP initialization timeout after {} seconds", timeout_secs)
}

/// Helper to create a "configuration error"
pub fn configuration_error(reason: &str) -> anyhow::Error {
    anyhow!("MCP configuration error: {}", reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_messages() {
        let err = tool_not_found("missing_tool");
        assert!(err.to_string().contains("missing_tool"));
        assert!(err.to_string().contains("not found"));

        let err = provider_not_found("missing_provider");
        assert!(err.to_string().contains("missing_provider"));

        let err = initialization_timeout(15);
        assert!(err.to_string().contains("15 seconds"));
    }
}
