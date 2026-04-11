//! Tool availability and schema accessors for ToolRegistry.

use crate::config::ToolDocumentationMode;
use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use crate::tools::names::canonical_tool_name;
use serde_json::Value;

use super::ToolRegistry;
use crate::tools::mcp::legacy_mcp_tool_name;

impl ToolRegistry {
    fn resolve_fallback_seed_tool(&self, failed_tool: &str) -> String {
        if let Ok(resolved) = self.resolve_public_tool_name_sync(failed_tool) {
            return resolved;
        }

        let lower = failed_tool.trim().to_ascii_lowercase();
        match lower.as_str() {
            "exec_code" => tools::UNIFIED_EXEC.to_string(),
            "list_dir" | "list_directory" => tools::UNIFIED_SEARCH.to_string(),
            _ => {
                if let Some((_, suffix)) = lower.rsplit_once('.')
                    && let Ok(resolved) = self.resolve_public_tool_name_sync(suffix)
                {
                    return resolved;
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
            tools::UNIFIED_SEARCH => &[tools::UNIFIED_FILE],
            tools::UNIFIED_EXEC => &[tools::UNIFIED_SEARCH],
            tools::UNIFIED_FILE | tools::APPLY_PATCH => &[tools::UNIFIED_SEARCH],
            // Task trackers require action-specific arguments; generic fallback names
            // create low-signal retries.
            tools::TASK_TRACKER | tools::PLAN_TASK_TRACKER => &[],
            // Unknown tools: prefer no fallback over noisy generic suggestions.
            _ => &[],
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

        let tools = self
            .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
            .await;

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

        if let Some(entry) = self
            .schema_for_public_name(
                tool_name,
                SessionToolsConfig::full_public(
                    SessionSurface::Interactive,
                    CapabilityLevel::CodeSearch,
                    ToolDocumentationMode::Full,
                    ToolModelCapabilities::default(),
                ),
            )
            .await
        {
            return Some(wrap_schema(
                entry.name.as_str(),
                entry.description.as_str(),
                &entry.parameters,
            ));
        }

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
        if self.resolve_public_tool_name_sync(name).is_ok() {
            return true;
        }

        // First check the main tool registry
        if self.inventory.has_tool(name) {
            return true;
        }

        // If not found, check if it's an MCP tool
        if let Some(tool_name) = legacy_mcp_tool_name(name) {
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
