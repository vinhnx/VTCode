//! MCP Integration Tests
//!
//! Tests for MCP (Model Context Protocol) functionality including
//! configuration loading, provider setup, and tool execution.

use std::collections::HashMap;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::{
    McpAllowListConfig, McpAllowListRules, McpClientConfig, McpProviderConfig,
    McpStdioServerConfig, McpTransportConfig, McpUiConfig, McpUiMode,
};
use vtcode_core::mcp_client::{McpClient, McpToolExecutor};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_config_loading() {
        // Test that MCP configuration can be loaded from TOML
        let toml_content = r#"
enabled = true

[ui]
mode = "compact"
max_events = 100
show_provider_names = true

max_concurrent_connections = 3
request_timeout_seconds = 30
retry_attempts = 2
startup_timeout_seconds = 120
tool_timeout_seconds = 45
experimental_use_rmcp_client = false

[[providers]]
name = "time"
enabled = true
command = "uvx"
args = ["mcp-server-time"]
max_concurrent_requests = 1
        "#;

        let mcp_config: McpClientConfig = toml::from_str(toml_content).unwrap();

        println!(
            "Parsed config: enabled={}, providers={}",
            mcp_config.enabled,
            mcp_config.providers.len()
        );

        assert!(mcp_config.enabled);
        assert_eq!(mcp_config.ui.mode, McpUiMode::Compact);
        assert_eq!(mcp_config.ui.max_events, 100);
        assert!(mcp_config.ui.show_provider_names);
        assert_eq!(mcp_config.max_concurrent_connections, 5); // Default value
        assert_eq!(mcp_config.request_timeout_seconds, 30);
        assert_eq!(mcp_config.startup_timeout_seconds, Some(120));
        assert_eq!(mcp_config.tool_timeout_seconds, Some(45));
        assert!(!mcp_config.experimental_use_rmcp_client);
        // retry_attempts uses default value of 3, which is fine

        assert_eq!(
            mcp_config.providers.len(),
            1,
            "Should have exactly 1 provider"
        );

        let provider = &mcp_config.providers[0];
        assert_eq!(provider.name, "time");
        assert!(provider.enabled);
        assert_eq!(provider.max_concurrent_requests, 1);

        match &provider.transport {
            McpTransportConfig::Stdio(stdio_config) => {
                assert_eq!(stdio_config.command, "uvx");
                assert_eq!(stdio_config.args, vec!["mcp-server-time"]);
            }
            McpTransportConfig::Http(_) => panic!("Expected stdio transport"),
        }
    }

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
    fn test_provider_config_creation() {
        let stdio_config = McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@upstash/context7-mcp@latest".to_string()],
            working_directory: Some("/tmp".to_string()),
        };

        let provider_config = McpProviderConfig {
            name: "context7".to_string(),
            transport: McpTransportConfig::Stdio(stdio_config),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 2,
            startup_timeout_ms: None,
        };

        assert_eq!(provider_config.name, "context7");
        assert!(provider_config.enabled);
        assert_eq!(provider_config.max_concurrent_requests, 2);

        match provider_config.transport {
            McpTransportConfig::Stdio(ref config) => {
                assert_eq!(config.command, "npx");
                assert_eq!(config.args, vec!["-y", "@upstash/context7-mcp@latest"]);
                assert_eq!(config.working_directory, Some("/tmp".to_string()));
            }
            McpTransportConfig::Http(_) => panic!("Expected stdio transport"),
        }
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
    fn test_ui_config_modes() {
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
    fn test_multiple_providers_config() {
        let toml_content = r#"
[mcp]
enabled = true

[[mcp.providers]]
name = "time"
enabled = true
command = "uvx"
args = ["mcp-server-time"]
max_concurrent_requests = 1

[[mcp.providers]]
name = "context7"
enabled = true
command = "npx"
args = ["-y", "@upstash/context7-mcp@latest"]
max_concurrent_requests = 2

[[mcp.providers]]
name = "fetch"
enabled = true
command = "uvx"
args = ["mcp-server-fetch"]
max_concurrent_requests = 1
        "#;

        let config: VTCodeConfig = toml::from_str(toml_content).unwrap();

        assert!(config.mcp.enabled);
        assert_eq!(config.mcp.providers.len(), 3);
        assert!(config.mcp.startup_timeout_seconds.is_none());
        assert!(config.mcp.tool_timeout_seconds.is_none());
        assert!(config.mcp.experimental_use_rmcp_client);

        // Check first provider (time)
        let time_provider = &config.mcp.providers[0];
        assert_eq!(time_provider.name, "time");
        assert!(time_provider.enabled);
        assert_eq!(time_provider.max_concurrent_requests, 1);

        // Check second provider (context7)
        let context7_provider = &config.mcp.providers[1];
        assert_eq!(context7_provider.name, "context7");
        assert!(context7_provider.enabled);
        assert_eq!(context7_provider.max_concurrent_requests, 2);

        // Check third provider (fetch)
        let fetch_provider = &config.mcp.providers[2];
        assert_eq!(fetch_provider.name, "fetch");
        assert!(fetch_provider.enabled);
        assert_eq!(fetch_provider.max_concurrent_requests, 1);
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

    #[tokio::test]
    async fn test_mcp_client_tool_execution() {
        let config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let client = McpClient::new(config);

        // Test tool execution without providers (should fail gracefully)
        let result = client
            .execute_tool("test_tool", serde_json::json!({}))
            .await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No MCP providers configured")
        );
    }

    #[tokio::test]
    async fn test_mcp_client_tool_listing() {
        let config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let client = McpClient::new(config);

        // Test tool listing without providers
        let result = client.list_tools().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mcp_client_disabled() {
        let config = McpClientConfig {
            enabled: false,
            ..Default::default()
        };

        let client = McpClient::new(config);

        // All operations should return empty results when disabled
        let tools_result = client.list_tools().await;
        assert!(tools_result.is_ok());
        assert_eq!(tools_result.unwrap().len(), 0);

        let execute_result = client.execute_tool("test", serde_json::json!({})).await;
        assert!(execute_result.is_err());
        assert!(execute_result.unwrap_err().to_string().contains("disabled"));

        let has_tool_result = client.has_mcp_tool("test").await;
        assert!(has_tool_result.is_ok());
        assert!(!has_tool_result.unwrap());
    }

    #[tokio::test]
    async fn test_mcp_client_status() {
        let config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let client = McpClient::new(config);
        let status = client.get_status();

        assert!(status.enabled);
        assert_eq!(status.provider_count, 0);
        assert_eq!(status.active_connections, 0);
        assert_eq!(status.configured_providers.len(), 0);
    }

    #[tokio::test]
    async fn test_provider_tool_availability() {
        let config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let client = McpClient::new(config);

        // Test tool availability without providers
        let result = client.has_mcp_tool("test_tool").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_mcp_allowlist_configuration() {
        let mut config = McpAllowListConfig::default();
        config.enforce = true;
        config.default.tools = Some(vec!["get_*".to_string(), "list_*".to_string()]);

        let mut provider_rules = McpAllowListRules::default();
        provider_rules.tools = Some(vec!["convert_*".to_string()]);
        config.providers.insert("time".to_string(), provider_rules);

        // Test default rules
        assert!(config.is_tool_allowed("other", "get_current_time"));
        assert!(config.is_tool_allowed("other", "list_documents"));
        assert!(!config.is_tool_allowed("other", "delete_current_time"));

        // Test provider-specific rules
        assert!(config.is_tool_allowed("time", "convert_timezone"));
        assert!(!config.is_tool_allowed("time", "get_current_time"));
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
}
