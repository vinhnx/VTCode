use tracing::warn;
use vtcode_core::llm::provider as uni;
use vtcode_core::mcp::McpToolInfo;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub struct McpToolManager;

impl McpToolManager {
    /// Enumerate MCP tools after refresh, identify newly added ones, and update the tool registry
    pub async fn enumerate_mcp_tools_after_refresh(
        tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &tokio::sync::RwLock<Vec<uni::ToolDefinition>>,
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
        renderer: &mut AnsiRenderer,
    ) -> anyhow::Result<()> {
        match tool_registry.list_mcp_tools().await {
            Ok(new_mcp_tools) => {
                let new_definitions =
                    super::session_setup::build_mcp_tool_definitions(&new_mcp_tools);
                let _updated_snapshot = {
                    let mut guard = tools.write().await;
                    guard.retain(|tool| !tool.function.name.starts_with("mcp_"));
                    guard.extend(new_definitions);
                    guard.clone()
                };

                // Calculate which tools are newly added by comparing with last known tools
                let current_tool_keys: Vec<String> = new_mcp_tools
                    .iter()
                    .map(|t| format!("{}-{}", t.provider, t.name))
                    .collect();

                let mut added_tools = Vec::new();
                for new_key in &current_tool_keys {
                    if !last_known_mcp_tools.contains(new_key) {
                        // Extract provider and tool name from the key for display
                        if let Some(pos) = new_key.find('-') {
                            let provider = &new_key[..pos];
                            let tool_name = &new_key[pos + 1..];
                            added_tools.push(format!("{}:{}", provider, tool_name));
                        } else {
                            // Fallback if there's no '-' in the key
                            added_tools.push(new_key.clone());
                        }
                    }
                }

                let message = if !added_tools.is_empty() {
                    if added_tools.len() == 1 {
                        format!("Discovered new MCP tool: {}", added_tools[0])
                    } else {
                        format!(
                            "Discovered {} new MCP tools: {}",
                            added_tools.len(),
                            added_tools.join(", ")
                        )
                    }
                } else {
                    // Fallback message if we can't determine which tools were added
                    let added_count = new_mcp_tools
                        .len()
                        .saturating_sub(last_known_mcp_tools.len());
                    if added_count > 0 {
                        format!(
                            "Discovered {} new MCP tool{}",
                            added_count,
                            if added_count == 1 { "" } else { "s" }
                        )
                    } else {
                        "MCP tools updated".to_string()
                    }
                };

                renderer.line(MessageStyle::Info, &message)?;
                renderer.line_if_not_empty(MessageStyle::Output)?;

                // Update the last known tools
                *last_known_mcp_tools = current_tool_keys;

                Ok(())
            }
            Err(err) => {
                warn!("Failed to enumerate MCP tools after refresh: {err}");
                return Err(err.into());
            }
        }
    }

    /// Enumerate MCP tools after initial setup, identify newly added ones, and update the tool registry  
    pub async fn enumerate_mcp_tools_after_initial_setup(
        _tool_registry: &mut vtcode_core::tools::ToolRegistry,
        tools: &tokio::sync::RwLock<Vec<uni::ToolDefinition>>,
        mcp_tools: Vec<McpToolInfo>, // Passed in from initial setup
        last_known_mcp_tools: &mut Vec<String>, // This becomes the new current tool list
        renderer: &mut AnsiRenderer,
    ) -> anyhow::Result<()> {
        let new_definitions = super::session_setup::build_mcp_tool_definitions(&mcp_tools);
        let _updated_snapshot = {
            let mut guard = tools.write().await;
            guard.retain(|tool| !tool.function.name.starts_with("mcp_"));
            guard.extend(new_definitions);
            guard.clone()
        };

        // Calculate which tools are newly added by comparing with last known tools
        let initial_tool_keys: Vec<String> = mcp_tools
            .iter()
            .map(|t| format!("{}-{}", t.provider, t.name))
            .collect();

        let mut added_tools = Vec::new();
        for new_key in &initial_tool_keys {
            if !last_known_mcp_tools.contains(new_key) {
                // Extract provider and tool name from the key for display
                if let Some(pos) = new_key.find('-') {
                    let provider = &new_key[..pos];
                    let tool_name = &new_key[pos + 1..];
                    added_tools.push(format!("{}:{}", provider, tool_name));
                } else {
                    // Fallback if there's no '-' in the key
                    added_tools.push(new_key.clone());
                }
            }
        }

        let message = if !added_tools.is_empty() {
            if added_tools.len() == 1 {
                format!("Discovered new MCP tool: {}", added_tools[0])
            } else {
                format!(
                    "Discovered {} new MCP tools: {}",
                    added_tools.len(),
                    added_tools.join(", ")
                )
            }
        } else {
            // Fallback message if we can't determine which tools were added
            let added_count = mcp_tools.len().saturating_sub(last_known_mcp_tools.len());
            if added_count > 0 {
                format!(
                    "Discovered {} new MCP tool{}",
                    added_count,
                    if added_count == 1 { "" } else { "s" }
                )
            } else {
                "MCP tools updated".to_string()
            }
        };

        renderer.line(MessageStyle::Info, &message)?;
        renderer.line_if_not_empty(MessageStyle::Output)?;

        // Store the initial tool names to track changes later
        *last_known_mcp_tools = initial_tool_keys;

        Ok(())
    }
}
