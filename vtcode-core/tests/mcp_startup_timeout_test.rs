use std::collections::HashMap;
use std::path::PathBuf;
use tokio::process::Command;
use vtcode_config::mcp::{McpProviderConfig, McpStdioServerConfig, McpTransportConfig};
use vtcode_core::mcp::{McpClient, McpToolExecutor};

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

#[tokio::test]
async fn test_unreachable_provider_does_not_block_initialization() {
    let config = vtcode_config::mcp::McpClientConfig {
        enabled: true,
        providers: vec![McpProviderConfig {
            name: "missing-binary".to_string(),
            transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                command: "this-command-should-not-exist-vtcode".to_string(),
                args: vec![],
                working_directory: None,
            }),
            env: HashMap::new(),
            enabled: true,
            max_concurrent_requests: 1,
            startup_timeout_ms: Some(500),
        }],
        startup_timeout_seconds: Some(1),
        tool_timeout_seconds: Some(1),
        ..Default::default()
    };

    let mut client = McpClient::new(config);
    assert!(
        client.initialize().await.is_ok(),
        "MCP initialization should continue even when a provider fails to connect"
    );

    let status = client.get_status();
    assert_eq!(status.provider_count, 0);
    assert_eq!(status.active_connections, 0);
    assert!(status.configured_providers.is_empty());

    let has_tool_err = client
        .has_mcp_tool("echo")
        .await
        .expect_err("configured-but-disconnected providers should return connectedness error");
    assert!(
        has_tool_err
            .to_string()
            .contains("No MCP providers are currently connected"),
        "unexpected has_mcp_tool error: {}",
        has_tool_err
    );
}

#[tokio::test]
async fn test_partial_provider_failures_still_keep_healthy_provider() {
    if !is_python_available().await {
        eprintln!("python3 not available, skipping mixed provider startup test");
        return;
    }

    let script_path = mock_mcp_server_path();
    if !script_path.exists() {
        eprintln!(
            "mock MCP server fixture not available at {}, skipping test",
            script_path.display()
        );
        return;
    }

    let config = vtcode_config::mcp::McpClientConfig {
        enabled: true,
        providers: vec![
            McpProviderConfig {
                name: "broken".to_string(),
                transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                    command: "this-command-should-not-exist-vtcode".to_string(),
                    args: vec![],
                    working_directory: None,
                }),
                env: HashMap::new(),
                enabled: true,
                max_concurrent_requests: 1,
                startup_timeout_ms: Some(500),
            },
            McpProviderConfig {
                name: "mock".to_string(),
                transport: McpTransportConfig::Stdio(McpStdioServerConfig {
                    command: "python3".to_string(),
                    args: vec![script_path.to_string_lossy().to_string()],
                    working_directory: None,
                }),
                env: HashMap::new(),
                enabled: true,
                max_concurrent_requests: 1,
                startup_timeout_ms: Some(2_000),
            },
        ],
        startup_timeout_seconds: Some(2),
        tool_timeout_seconds: Some(2),
        ..Default::default()
    };

    let mut client = McpClient::new(config);
    assert!(client.initialize().await.is_ok());

    let status = client.get_status();
    assert_eq!(
        status.provider_count, 1,
        "healthy providers should remain active when another provider fails"
    );
    assert_eq!(status.active_connections, 1);
    assert_eq!(status.configured_providers, vec!["mock".to_string()]);

    let tools = client.list_tools().await.expect("tools should be listed");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert_eq!(
        client.provider_for_tool("echo").as_deref(),
        Some("mock"),
        "tool index should only contain healthy provider tools"
    );
}

async fn is_python_available() -> bool {
    match Command::new("python3").arg("--version").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

fn mock_mcp_server_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("mock_mcp_server.py")
}
