//! Tool availability and schema accessors for ToolRegistry.

use serde_json::Value;

use crate::tools::names::canonical_tool_name;

use super::ToolRegistry;

impl ToolRegistry {
    fn resolve_fallback_seed_tool(&self, failed_tool: &str) -> String {
        if let Some(registration) = self.inventory.registration_for(failed_tool) {
            return registration.name().to_string();
        }

        let lower = failed_tool.trim().to_ascii_lowercase();
        match lower.as_str() {
            // Legacy/provider-emitted execution aliases
            "exec_code" | "exec code" | "run code" | "run command" | "run command (pty)"
            | "container.exec" | "bash" => "unified_exec".to_string(),
            // Harmony namespace variants
            "repo_browser.list_files"
            | "list files"
            | "search text"
            | "list tools"
            | "list errors"
            | "show agent info"
            | "fetch" => "unified_search".to_string(),
            "repo_browser.read_file"
            | "repo_browser.write_file"
            | "read file"
            | "write file"
            | "edit file"
            | "apply patch"
            | "delete file"
            | "move file"
            | "copy file"
            | "file operation" => "unified_file".to_string(),
            _ => {
                if let Some((_, suffix)) = lower.rsplit_once('.')
                    && let Some(registration) = self.inventory.registration_for(suffix)
                {
                    return registration.name().to_string();
                }
                lower
            }
        }
    }

    /// Suggest a fallback tool for a failed invocation using lightweight heuristics.
    pub async fn suggest_fallback_tool(&self, failed_tool: &str) -> Option<String> {
        let available = self.available_tools().await;
        let failed = canonical_tool_name(failed_tool);
        let failed_name: &str = failed.as_ref();
        let seed = self.resolve_fallback_seed_tool(failed_name);

        if seed != failed_name && available.iter().any(|tool| tool == &seed) {
            return Some(seed);
        }

        let candidates: &[&str] = match seed.as_str() {
            "read_file" => &["list_files", "grep_file", "unified_search"],
            "list_files" => &["read_file", "grep_file", "unified_search"],
            "grep_file" => &["read_file", "unified_search"],
            "run_pty_cmd" | "shell" | "unified_exec" => {
                &["unified_exec", "unified_search", "grep_file"]
            }
            "write_file" | "edit_file" | "apply_patch" | "unified_file" => {
                &["read_file", "grep_file", "unified_search"]
            }
            _ => &["unified_search", "grep_file", "read_file"],
        };

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
        if let Ok(cache) = self.cached_available_tools.try_read()
            && let Some(tools) = cache.as_ref()
        {
            return tools.clone();
        }

        // HP-7: Inventory tools are already sorted, just convert to Vec
        let mut tools = self.inventory.available_tools().to_vec();
        tools.extend(self.inventory.registered_aliases());

        tools.sort_unstable();
        tools.dedup();

        // Update cache with try_write to avoid blocking
        if let Ok(mut cache) = self.cached_available_tools.try_write() {
            *cache = Some(tools.clone());
        }

        tools
    }

    /// Get the schema for a specific tool.
    pub async fn get_tool_schema(&self, tool_name: &str) -> Option<Value> {
        let wrap_schema = |requested_name: &str, description: &str, schema: &Value| {
            // Wrap in full declaration if it's just parameters
            if schema.get("properties").is_some() && schema.get("name").is_none() {
                serde_json::json!({
                    "name": requested_name,
                    "description": description,
                    "parameters": schema
                })
            } else {
                schema.clone()
            }
        };

        // Resolve tool (handles built-ins, MCP proxies, and aliases)
        if let Some(registration) = self.inventory.get_registration(tool_name)
            && let Some(schema) = registration.parameter_schema()
        {
            return Some(wrap_schema(
                tool_name,
                registration.metadata().description().unwrap_or(""),
                schema,
            ));
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
            if self.has_mcp_tool(tool_name).await {
                return true;
            }

            // Check if it's an alias
            if let Some(resolved_name) = self.resolve_mcp_tool_alias(tool_name).await
                && resolved_name != tool_name
            {
                return true;
            }
        }

        false
    }
}
