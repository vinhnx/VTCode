//! ToolExecutor trait implementation for ToolRegistry.

use anyhow::Result;
use serde_json::Value;

use super::ToolRegistry;
use crate::tools::traits::ToolExecutor;

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
        self.has_tool(name).await
    }
}
