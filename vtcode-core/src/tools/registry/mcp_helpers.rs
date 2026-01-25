//! MCP-related helper methods for ToolRegistry.

use super::ToolRegistry;

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
        let index = self.mcp_tool_index.read().await;
        for (provider, tools) in index.iter() {
            if tools.iter().any(|candidate| candidate == tool_name) {
                return Some(provider.clone());
            }
        }
        None
    }
}

pub(crate) fn normalize_mcp_tool_identifier(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        }
    }
    normalized
}
