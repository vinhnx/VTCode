use std::collections::HashMap;

use vtcode_core::config::mcp::{
    McpClientConfig, McpProviderConfig, McpStdioServerConfig, McpTransportConfig,
};
use vtcode_core::mcp_client::McpClient;

#[tokio::test]
#[ignore]
async fn context7_list_tools_smoke() {
    let provider = McpProviderConfig {
        name: "context7".to_string(),
        transport: McpTransportConfig::Stdio(McpStdioServerConfig {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@upstash/context7-mcp@latest".to_string()],
            working_directory: None,
        }),
        env: HashMap::new(),
        enabled: true,
        max_concurrent_requests: 1,
        startup_timeout_ms: None,
    };

    let mut config = McpClientConfig::default();
    config.enabled = true;
    config.providers = vec![provider];

    let mut client = McpClient::new(config);
    client.initialize().await.unwrap();

    let tools = client.list_tools().await.unwrap();
    assert!(
        !tools.is_empty(),
        "context7 should expose at least one tool"
    );
}
