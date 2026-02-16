//! MCP-related helper methods for ToolRegistry.

use super::ToolRegistry;
use crate::utils::path::normalize_ascii_identifier;

impl ToolRegistry {
    pub(super) async fn mcp_policy_keys(&self) -> Vec<String> {
        let index = self.mcp_tool_index.read().await;
        let capacity: usize = index.values().map(|tools| tools.len()).sum();
        let mut keys = Vec::with_capacity(capacity);
        for (provider, tools) in index.iter() {
            for tool in tools {
                keys.push(format!("mcp::{}::{}", provider, tool));
            }
        }
        keys
    }

    pub(super) async fn find_mcp_provider(&self, tool_name: &str) -> Option<String> {
        self.mcp_reverse_index.read().await.get(tool_name).cloned()
    }
}

pub(crate) fn normalize_mcp_tool_identifier(value: &str) -> String {
    normalize_ascii_identifier(value)
}
