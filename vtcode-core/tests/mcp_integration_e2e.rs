//! End-to-End MCP Integration Tests
//!
//! These tests verify that MCP integration works correctly with real MCP servers.
//! They test the complete flow from configuration loading to tool execution.

use std::collections::HashMap;
use tempfile::TempDir;
use tokio::process::Command;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::mcp::{
    McpClientConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig,
};
use vtcode_core::mcp_client::McpClient;
use vtcode_core::tools::registry::ToolRegistry;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // This test requires the time MCP server to be installed
    async fn test_time_mcp_server_integration() {
        // Skip if time server is not available
        if !is_time_server_available().await {
            eprintln!("Time MCP server not available, skipping test");
            return;
        }

        // Create a temporary workspace
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();

        // Create MCP configuration for time server
        let mut mcp_config = McpClientConfig::default();
        mcp_config.enabled = true;

        let time_provider = McpProviderConfig {
            name: "time".to_string(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "uvx".to_string(),
                args: vec!["mcp-server-time".to_string()],
                working_directory: Some(workspace.to_string_lossy().to_string()),
            }),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 3,
            startup_timeout_ms: None,
        };

        mcp_config.providers = vec![time_provider];

        // Create MCP client
        let mut mcp_client = McpClient::new(mcp_config);

        // Initialize the client
        assert!(mcp_client.initialize().await.is_ok());

        // Check that we can list tools
        let tools = mcp_client.list_tools().await.unwrap();
        assert!(!tools.is_empty(), "Time MCP server should provide tools");

        // Look for the get_current_time tool
        let time_tool = tools.iter().find(|tool| tool.name == "get_current_time");
        assert!(
            time_tool.is_some(),
            "get_current_time tool should be available"
        );

        // Execute the get_current_time tool
        let result = mcp_client
            .execute_tool("get_current_time", serde_json::json!({}))
            .await;
        assert!(
            result.is_ok(),
            "get_current_time tool should execute successfully"
        );

        let result_value = result.unwrap();
        assert!(
            result_value.get("time").is_some(),
            "Result should contain time field"
        );

        println!("MCP time server integration test passed!");
        println!(
            "Current time: {}",
            result_value["time"].as_str().unwrap_or("unknown")
        );
    }

    #[tokio::test]
    async fn test_mcp_configuration_loading() {
        let toml_content = r#"
[mcp]
enabled = true
max_concurrent_connections = 3
request_timeout_seconds = 45
retry_attempts = 2

[mcp.ui]
mode = "compact"
max_events = 25
show_provider_names = false

[[mcp.providers]]
name = "time"
enabled = true
command = "uvx"
args = ["mcp-server-time"]
max_concurrent_requests = 2
        "#;

        let config: VTCodeConfig = toml::from_str(toml_content).unwrap();

        assert!(config.mcp.enabled);
        assert_eq!(config.mcp.ui.mode.to_string(), "compact");
        assert_eq!(config.mcp.ui.max_events, 25);
        assert!(!config.mcp.ui.show_provider_names);
        assert_eq!(config.mcp.max_concurrent_connections, 3);
        assert_eq!(config.mcp.request_timeout_seconds, 45);
        assert_eq!(config.mcp.retry_attempts, 2);
        assert!(config.mcp.startup_timeout_seconds.is_none());
        assert!(config.mcp.tool_timeout_seconds.is_none());
        assert!(config.mcp.experimental_use_rmcp_client);
        assert_eq!(config.mcp.providers.len(), 1);

        let provider = &config.mcp.providers[0];
        assert_eq!(provider.name, "time");
        assert!(provider.enabled);
        assert_eq!(provider.max_concurrent_requests, 2);
    }

    #[tokio::test]
    async fn test_tool_registry_with_mcp_client() {
        let temp_dir = TempDir::new().unwrap();

        // Create tool registry without MCP client
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        // Initially should not have MCP tools
        assert!(registry.mcp_client().is_none());

        // Create a mock MCP client config
        let mcp_config = McpClientConfig {
            enabled: true,
            ..Default::default()
        };

        let mcp_client = McpClient::new(mcp_config);

        // Add MCP client to registry
        registry = registry.with_mcp_client(std::sync::Arc::new(mcp_client));

        // Should now have MCP client
        assert!(registry.mcp_client().is_some());
    }

    #[tokio::test]
    async fn test_mcp_disabled_by_default() {
        // Create MCP client with default config (disabled)
        let config = McpClientConfig::default();
        let mut client = McpClient::new(config);

        // Initialize should succeed but do nothing
        assert!(client.initialize().await.is_ok());

        // Should have no providers
        let status = client.get_status();
        assert_eq!(status.provider_count, 0);

        // List tools should return empty
        let tools = client.list_tools().await.unwrap();
        assert!(tools.is_empty());
    }

    #[tokio::test]
    async fn test_mcp_client_status() {
        let config = McpClientConfig::default();
        let client = McpClient::new(config);

        let status = client.get_status();
        assert!(!status.enabled);
        assert_eq!(status.provider_count, 0);
        assert_eq!(status.active_connections, 0);
        assert!(status.configured_providers.is_empty());
    }

    #[tokio::test]
    async fn test_multiple_providers_config() {
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
name = "disabled_provider"
enabled = false
command = "echo"
args = ["disabled"]
max_concurrent_requests = 1
        "#;

        let config: VTCodeConfig = toml::from_str(toml_content).unwrap();

        assert!(config.mcp.enabled);
        assert_eq!(config.mcp.providers.len(), 3);

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

        // Check third provider (disabled)
        let disabled_provider = &config.mcp.providers[2];
        assert_eq!(disabled_provider.name, "disabled_provider");
        assert!(!disabled_provider.enabled);
    }

    #[tokio::test]
    async fn test_provider_environment_variables() {
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

    #[test]
    fn test_mcp_ui_modes() {
        use vtcode_core::config::mcp::McpUiMode;

        let compact_config = vtcode_core::config::mcp::McpUiConfig {
            mode: McpUiMode::Compact,
            max_events: 25,
            show_provider_names: false,
            renderers: HashMap::new(),
        };

        let full_config = vtcode_core::config::mcp::McpUiConfig {
            mode: McpUiMode::Full,
            max_events: 100,
            show_provider_names: true,
            renderers: HashMap::new(),
        };

        assert_eq!(compact_config.mode, McpUiMode::Compact);
        assert_eq!(full_config.mode, McpUiMode::Full);
        assert!(!compact_config.show_provider_names);
        assert!(full_config.show_provider_names);
        assert_eq!(compact_config.max_events, 25);
        assert_eq!(full_config.max_events, 100);
    }
}

/// Check if the time MCP server is available for testing
async fn is_time_server_available() -> bool {
    match Command::new("uvx").arg("--help").output().await {
        Ok(_) => {
            // Try to check if mcp-server-time is available
            match Command::new("uvx")
                .arg("mcp-server-time")
                .arg("--help")
                .output()
                .await
            {
                Ok(output) => output.status.success(),
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}
