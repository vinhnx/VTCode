use tracing::warn;
use vtcode_core::llm::provider as uni;
use vtcode_core::mcp::McpToolInfo;

use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;

pub struct McpToolManager;

impl McpToolManager {
    /// Enumerate MCP tools after refresh, identify newly added ones, and update the tool registry
    pub async fn enumerate_mcp_tools_after_refresh(
        tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &tokio::sync::RwLock<Vec<uni::ToolDefinition>>,
        tool_catalog: &ToolCatalogState,
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
    ) -> anyhow::Result<()> {
        match tool_registry.list_mcp_tools().await {
            Ok(new_mcp_tools) => {
                let new_definitions =
                    super::session_setup::build_mcp_tool_definitions(&new_mcp_tools);
                {
                    let mut guard = tools.write().await;
                    guard.retain(|tool| {
                        tool.function
                            .as_ref()
                            .is_none_or(|f| !f.name.starts_with("mcp_"))
                    });
                    guard.extend(new_definitions);
                };
                tool_catalog.bump_version();

                // Calculate which tools are newly added by comparing with last known tools
                let current_tool_keys: Vec<String> = new_mcp_tools
                    .iter()
                    .map(|t| format!("{}-{}", t.provider, t.name))
                    .collect();

                // Update the last known tools silently (don't print discovery messages)
                *last_known_mcp_tools = current_tool_keys;

                Ok(())
            }
            Err(err) => {
                warn!("Failed to enumerate MCP tools after refresh: {err}");
                Err(err)
            }
        }
    }

    /// Enumerate MCP tools after initial setup, identify newly added ones, and update the tool registry
    pub async fn enumerate_mcp_tools_after_initial_setup(
        _tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &tokio::sync::RwLock<Vec<uni::ToolDefinition>>,
        tool_catalog: &ToolCatalogState,
        mcp_tools: Vec<McpToolInfo>, // Passed in from initial setup
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
    ) -> anyhow::Result<()> {
        let new_definitions = super::session_setup::build_mcp_tool_definitions(&mcp_tools);
        {
            let mut guard = tools.write().await;
            guard.retain(|tool| {
                tool.function
                    .as_ref()
                    .is_none_or(|f| !f.name.starts_with("mcp_"))
            });
            guard.extend(new_definitions);
        };
        tool_catalog.bump_version();

        // Calculate which tools are newly added by comparing with last known tools
        let initial_tool_keys: Vec<String> = mcp_tools
            .iter()
            .map(|t| format!("{}-{}", t.provider, t.name))
            .collect();

        // Store the initial tool names to track changes later (silently)
        *last_known_mcp_tools = initial_tool_keys;

        Ok(())
    }
}
