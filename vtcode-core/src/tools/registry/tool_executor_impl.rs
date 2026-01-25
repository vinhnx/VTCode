//! ToolExecutor trait implementation for ToolRegistry.

use anyhow::Result;
use serde_json::Value;

use crate::tools::traits::ToolExecutor;
use super::ToolRegistry;

#[async_trait::async_trait]
impl ToolExecutor for ToolRegistry {
    async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        self.execute_tool(name, args).await
    }

    async fn execute_tool_ref(&self, name: &str, args: &Value) -> Result<Value> {
        self.execute_tool_ref(name, args).await
    }

    async fn available_tools(&self) -> Vec<String> {
        self.available_tools().await
    }

    async fn has_tool(&self, name: &str) -> bool {
        // Optimized check: check inventory first, then cached MCP presence
        if self.inventory.has_tool(name) {
            return true;
        }

        let presence = self.mcp_tool_presence.read().await;
        if let Some(&present) = presence.get(name) {
            return present;
        }

        // Fallback to provider check if not in quick cache
        if self.find_mcp_provider(name).await.is_some() {
            return true;
        }

        false
    }
}
