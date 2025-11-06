//! Basic MCP Integration Tests
//!
//! These tests verify that MCP configuration and basic functionality work correctly.

use std::collections::HashMap;
use vtcode_core::config::mcp::{
    McpClientConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig, McpUiConfig,
    McpUiMode,
};
use vtcode_core::mcp_client::McpClient;

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
        assert!(config.startup_timeout_seconds.is_none());
        assert!(config.tool_timeout_seconds.is_none());
        assert!(config.experimental_use_rmcp_client);
        assert!(config.providers.is_empty());
    }

    #[test]
    fn test_mcp_ui_config_defaults() {
        let ui_config = McpUiConfig::default();
        assert_eq!(ui_config.mode, McpUiMode::Compact);
        assert_eq!(ui_config.max_events, 50);
        assert!(ui_config.show_provider_names);
    }

    #[test]
    fn test_mcp_provider_config_defaults() {
        let provider_config = McpProviderConfig::default();
        assert!(provider_config.enabled);
        assert_eq!(provider_config.max_concurrent_requests, 3);
        assert!(provider_config.env.is_empty());
        assert!(provider_config.name.is_empty());
    }

    #[test]
    fn test_stdio_server_config_defaults() {
        let stdio_config = McpStdioServerConfig::default();
        assert!(stdio_config.command.is_empty());
        assert!(stdio_config.args.is_empty());
        assert!(stdio_config.working_directory.is_none());
    }

    #[test]
    fn test_http_server_config_defaults() {
        let http_config = vtcode_core::config::mcp::McpHttpServerConfig::default();
        assert!(http_config.endpoint.is_empty());
        assert!(http_config.api_key_env.is_none());
        assert_eq!(http_config.protocol_version, "2024-11-05");
        assert!(http_config.http_headers.is_empty());
        assert!(http_config.env_http_headers.is_empty());
    }

    #[test]
    fn test_mcp_client_creation() {
        let config = McpClientConfig::default();
        let client = McpClient::new(config);

        let status = client.get_status();
        assert!(!status.enabled);
        assert_eq!(status.provider_count, 0);
    }

    #[test]
    fn test_mcp_ui_modes() {
        let compact_ui = McpUiConfig {
            mode: McpUiMode::Compact,
            max_events: 25,
            show_provider_names: false,
            renderers: HashMap::new(),
        };

        let full_ui = McpUiConfig {
            mode: McpUiMode::Full,
            max_events: 100,
            show_provider_names: true,
            renderers: HashMap::new(),
        };

        assert_eq!(compact_ui.mode, McpUiMode::Compact);
        assert_eq!(full_ui.mode, McpUiMode::Full);
        assert!(!compact_ui.show_provider_names);
        assert!(full_ui.show_provider_names);
        assert_eq!(compact_ui.max_events, 25);
        assert_eq!(full_ui.max_events, 100);
    }

    #[test]
    fn test_provider_environment_variables() {
        let mut env_vars = HashMap::new();
        env_vars.insert("API_KEY".to_string(), "secret_key".to_string());
        env_vars.insert("DEBUG".to_string(), "true".to_string());

        let provider_config = McpProviderConfig {
            name: "test_provider".to_string(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "test_command".to_string(),
                args: vec![],
                working_directory: None,
            }),
            env: env_vars,
            enabled: true,
            max_concurrent_requests: 1,
            startup_timeout_ms: None,
        };

        assert_eq!(provider_config.env.len(), 2);
        assert_eq!(
            provider_config.env.get("API_KEY"),
            Some(&"secret_key".to_string())
        );
        assert_eq!(provider_config.env.get("DEBUG"), Some(&"true".to_string()));
    }

    #[tokio::test]
    async fn test_mcp_client_initialization() {
        let config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let mut client = McpClient::new(config);

        // This should not fail even if no providers are configured
        let result = client.initialize().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_tool_info_creation() {
        let tool_info = vtcode_core::mcp_client::McpToolInfo {
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
