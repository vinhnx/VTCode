use std::collections::HashMap;
use vtcode_config::mcp::{McpProviderConfig, McpStdioServerConfig, McpTransportConfig};

#[test]
fn test_provider_with_custom_startup_timeout() {
    let provider = McpProviderConfig {
        name: "test-provider".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec!["mcp-server-time".to_string()],
            working_directory: None,
        }),
        env: HashMap::new(),
        enabled: true,
        max_concurrent_requests: 3,
        startup_timeout_ms: Some(30000), // 30 seconds
    };

    assert_eq!(provider.startup_timeout_ms, Some(30000));
}

#[test]
fn test_provider_with_default_timeout() {
    let provider = McpProviderConfig {
        name: "default-provider".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec!["mcp-server-time".to_string()],
            working_directory: None,
        }),
        env: HashMap::new(),
        enabled: true,
        max_concurrent_requests: 3,
        startup_timeout_ms: None, // Will use global default
    };

    assert_eq!(provider.startup_timeout_ms, None);
}

#[test]
fn test_provider_with_no_timeout() {
    let provider = McpProviderConfig {
        name: "no-timeout-provider".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec!["mcp-server-time".to_string()],
            working_directory: None,
        }),
        env: HashMap::new(),
        enabled: true,
        max_concurrent_requests: 3,
        startup_timeout_ms: Some(0), // No timeout (infinite)
    };

    assert_eq!(provider.startup_timeout_ms, Some(0));
}

#[test]
fn test_provider_default_struct() {
    let provider = McpProviderConfig::default();
    assert_eq!(provider.startup_timeout_ms, None);
}
