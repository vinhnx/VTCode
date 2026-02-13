//! Tool availability and schema accessors for ToolRegistry.

use serde_json::Value;

use crate::mcp::McpToolExecutor;
use crate::tools::names::canonical_tool_name;

use super::ToolRegistry;

impl ToolRegistry {
    /// Suggest a fallback tool for a failed invocation using lightweight heuristics.
    pub async fn suggest_fallback_tool(&self, failed_tool: &str) -> Option<String> {
        let failed = canonical_tool_name(failed_tool);
        let failed_name: &str = failed.as_ref();
        let candidates: &[&str] = match failed.as_ref() {
            "read_file" => &["list_files", "grep_file", "unified_search"],
            "list_files" => &["read_file", "grep_file", "unified_search"],
            "grep_file" => &["read_file", "unified_search", "code_intelligence"],
            "code_intelligence" => &["grep_file", "read_file", "unified_search"],
            "run_pty_cmd" | "shell" | "unified_exec" => &["unified_search", "grep_file"],
            "write_file" | "edit_file" | "apply_patch" | "unified_file" => {
                &["read_file", "grep_file", "unified_search"]
            }
            _ => &["unified_search", "grep_file", "read_file"],
        };

        let available = self.available_tools().await;
        for candidate in candidates {
            if *candidate != failed_name && available.iter().any(|tool| tool == candidate) {
                return Some((*candidate).to_string());
            }
        }
        None
    }

    /// Get a list of all available tools, including MCP tools.
    pub async fn available_tools(&self) -> Vec<String> {
        // Use try_read to avoid blocking on contested locks
        if let Ok(cache) = self.cached_available_tools.try_read() {
            if let Some(tools) = cache.as_ref() {
                return tools.clone();
            }
        }

        // HP-7: Inventory tools are already sorted, just convert to Vec
        let mut tools = self.inventory.available_tools().to_vec();
        tools.extend(self.inventory.registered_aliases());

        // Add MCP tools if available - use cache first
        {
            let index = self.mcp_tool_index.read().await;
            if !index.is_empty() {
                for tools_list in index.values() {
                    for tool in tools_list {
                        tools.push(format!("mcp_{}", tool));
                    }
                }
            } else {
                // Background compute - if cache is empty, we might need a refresh
                // But generally refresh_mcp_tools should have been called.
                // Fallback to active client query if needed
                let client_opt = { self.mcp_client.read().unwrap().clone() };
                if let Some(mcp_client) = client_opt {
                    match mcp_client.list_mcp_tools().await {
                        Ok(mcp_tools) => {
                            tools.reserve(mcp_tools.len());
                            for tool in mcp_tools {
                                tools.push(format!("mcp_{}", tool.name));
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Failed to list MCP tools: {}", e);
                        }
                    }
                }
            }
        }

        tools.sort_unstable();

        // Update cache with try_write to avoid blocking
        if let Ok(mut cache) = self.cached_available_tools.try_write() {
            *cache = Some(tools.clone());
        }

        tools
    }

    /// Get the schema for a specific tool.
    pub async fn get_tool_schema(&self, tool_name: &str) -> Option<Value> {
        // First check if it's a regular tool
        if let Some(registration) = self.inventory.get_registration(tool_name) {
            if let Some(schema) = registration.parameter_schema() {
                // Wrap in full declaration if it's just parameters
                if schema.get("properties").is_some() && schema.get("name").is_none() {
                    return Some(serde_json::json!({
                        "name": tool_name,
                        "description": registration.metadata().description().unwrap_or(""),
                        "parameters": schema
                    }));
                }
                return Some(schema.clone());
            }
        }

        // Check if it's an MCP tool
        let client_opt = { self.mcp_client.read().unwrap().clone() };
        if let Some(client) = client_opt {
            if self.mcp_circuit_breaker.allow_request() {
                if let Ok(tools) = client.list_mcp_tools().await {
                    if let Some(mcp_tool) = tools.into_iter().find(|t| t.name == tool_name) {
                        return Some(serde_json::json!({
                            "name": tool_name,
                            "description": mcp_tool.description,
                            "parameters": mcp_tool.input_schema
                        }));
                    }
                }
            }
        }

        None
    }

    /// Check if a tool with the given name is registered.
    ///
    /// # Arguments
    /// * `name` - The name of the tool to check
    ///
    /// # Returns
    /// `bool` indicating whether the tool exists (including aliases)
    pub async fn has_tool(&self, name: &str) -> bool {
        // First check the main tool registry
        if self.inventory.has_tool(name) {
            return true;
        }

        // If not found, check if it's an MCP tool
        if let Some(tool_name) = name.strip_prefix("mcp_") {
            if self.find_mcp_provider(tool_name).await.is_some() {
                return true;
            }

            let mcp_client_opt = self.mcp_client.read().unwrap().clone();
            if let Some(mcp_client) = mcp_client_opt {
                if let Ok(true) = mcp_client.has_mcp_tool(tool_name).await {
                    return true;
                }
                // Check if it's an alias
                if let Some(resolved_name) = self.resolve_mcp_tool_alias(tool_name).await
                    && resolved_name != tool_name
                {
                    return true;
                }
            }
        }

        false
    }
}
