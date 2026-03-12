use tracing::warn;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::mcp::McpToolInfo;

use crate::agent::runloop::unified::session_setup::refresh_tool_snapshot;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;

pub(crate) struct McpToolManager;

impl McpToolManager {
    /// Enumerate MCP tools after refresh, identify newly added ones, and update the tool registry
    pub(crate) async fn enumerate_mcp_tools_after_refresh(
        tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &std::sync::Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
        tool_catalog: &ToolCatalogState,
        config: &CoreAgentConfig,
        tool_documentation_mode: ToolDocumentationMode,
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
    ) -> anyhow::Result<()> {
        match tool_registry.list_mcp_tools().await {
            Ok(new_mcp_tools) => {
                refresh_tool_snapshot(
                    tool_registry,
                    tools,
                    tool_catalog,
                    config,
                    tool_documentation_mode,
                )
                .await;
                tool_catalog.mark_pending_refresh("mcp_background_refresh");

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
    pub(crate) async fn enumerate_mcp_tools_after_initial_setup(
        tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &std::sync::Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
        tool_catalog: &ToolCatalogState,
        config: &CoreAgentConfig,
        tool_documentation_mode: ToolDocumentationMode,
        mcp_tools: Vec<McpToolInfo>, // Passed in from initial setup
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
    ) -> anyhow::Result<()> {
        refresh_tool_snapshot(
            tool_registry,
            tools,
            tool_catalog,
            config,
            tool_documentation_mode,
        )
        .await;
        tool_catalog.mark_pending_refresh("mcp_background_refresh");

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
