#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio;

    #[tokio::test]
    async fn test_async_mcp_manager_creation() {
        let config = McpClientConfig::default();
        let event_callback: Arc<dyn Fn(McpEvent) + Send + Sync> = Arc::new(|_event| {});

        let manager = AsyncMcpManager::new(config, event_callback);
        let status = manager.get_status().await;

        // With default config, MCP should be disabled
        assert!(matches!(status, McpInitStatus::Disabled));
    }

    #[tokio::test]
    async fn test_mcp_init_status_display() {
        let disabled_status = McpInitStatus::Disabled;
        assert_eq!(disabled_status.to_string(), "MCP is disabled");

        let initializing_status = McpInitStatus::Initializing {
            progress: "Connecting...".to_string(),
        };
        assert_eq!(
            initializing_status.to_string(),
            "MCP initializing: Connecting..."
        );

        let error_status = McpInitStatus::Error {
            message: "Connection failed".to_string(),
        };
        assert_eq!(error_status.to_string(), "MCP error: Connection failed");
    }

    #[tokio::test]
    async fn test_mcp_init_status_helpers() {
        let disabled_status = McpInitStatus::Disabled;
        assert!(!disabled_status.is_ready());
        assert!(!disabled_status.is_error());
        assert!(disabled_status.get_error_message().is_none());

        let error_status = McpInitStatus::Error {
            message: "Test error".to_string(),
        };
        assert!(!error_status.is_ready());
        assert!(error_status.is_error());
        assert_eq!(error_status.get_error_message(), Some("Test error"));

        let initializing_status = McpInitStatus::Initializing {
            progress: "Init...".to_string(),
        };
        assert!(initializing_status.is_initializing());
        assert!(!initializing_status.is_ready());
    }
}