use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;

use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshDecision {
    None,
    MarkPending,
    ApplyPending,
}

fn decide_refresh_action(pending_refresh: bool, tool_list_changed: bool) -> RefreshDecision {
    if pending_refresh {
        RefreshDecision::ApplyPending
    } else if tool_list_changed {
        RefreshDecision::MarkPending
    } else {
        RefreshDecision::None
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_mcp_updates(
    mcp_manager: &AsyncMcpManager,
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tools: &Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    tool_catalog: &ToolCatalogState,
    config: &CoreAgentConfig,
    tool_documentation_mode: ToolDocumentationMode,
    renderer: &mut AnsiRenderer,
    mcp_catalog_initialized: &mut bool,
    last_mcp_refresh: &mut Instant,
    last_known_mcp_tools: &mut Vec<String>,
    pending_mcp_refresh: &mut bool,
    refresh_interval: std::time::Duration,
) -> Result<()> {
    if !*mcp_catalog_initialized {
        match mcp_manager.get_status().await {
            McpInitStatus::Ready { client } => {
                tool_registry.set_mcp_client(Arc::clone(&client)).await;
                match tool_registry.refresh_mcp_tools().await {
                    Ok(()) => {
                        let mut registered_tools = 0usize;
                        match tool_registry.list_mcp_tools().await {
                            Ok(mcp_tools) => {
                                registered_tools = mcp_tools.len();
                                McpToolManager::enumerate_mcp_tools_after_initial_setup(
                                    tool_registry,
                                    tools,
                                    tool_catalog,
                                    config,
                                    tool_documentation_mode,
                                    mcp_tools,
                                    last_known_mcp_tools,
                                )
                                .await?;
                                *pending_mcp_refresh = false;
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "Failed to enumerate MCP tools after refresh: {err}"
                                );
                            }
                        }

                        renderer.line(
                            MessageStyle::Info,
                            &format!(
                                "MCP tools ready ({} registered). Use /mcp tools to inspect the catalog.",
                                registered_tools
                            ),
                        )?;
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                    }
                    Err(err) => {
                        tracing::warn!("Failed to refresh MCP tools after initialization: {err}");
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to index MCP tools: {}", err),
                        )?;
                        renderer.line_if_not_empty(MessageStyle::Output)?;
                    }
                }
                *mcp_catalog_initialized = true;
            }
            McpInitStatus::Error { message } => {
                renderer.line(MessageStyle::Error, &format!("MCP Error: {}", message))?;
                renderer.line_if_not_empty(MessageStyle::Output)?;
                *mcp_catalog_initialized = true;
            }
            McpInitStatus::Initializing { .. } | McpInitStatus::Disabled => {}
        }
    }

    if *mcp_catalog_initialized && last_mcp_refresh.elapsed() >= refresh_interval {
        *last_mcp_refresh = std::time::Instant::now();

        if matches!(
            decide_refresh_action(*pending_mcp_refresh, false),
            RefreshDecision::ApplyPending
        ) {
            match tool_registry.refresh_mcp_tools().await {
                Ok(()) => match tool_registry.list_mcp_tools().await {
                    Ok(_) => {
                        McpToolManager::enumerate_mcp_tools_after_refresh(
                            tool_registry,
                            tools,
                            tool_catalog,
                            config,
                            tool_documentation_mode,
                            last_known_mcp_tools,
                        )
                        .await?;
                        *pending_mcp_refresh = false;
                    }
                    Err(err) => {
                        tracing::warn!(
                            "Failed to enumerate deferred MCP tools after refresh: {err}"
                        );
                    }
                },
                Err(err) => {
                    tracing::warn!("Failed to refresh deferred MCP tools: {err}");
                }
            }
            return Ok(());
        }

        if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
            let current_tool_keys: Vec<String> = known_tools
                .iter()
                .map(|t| format!("{}-{}", t.provider, t.name))
                .collect();

            if matches!(
                decide_refresh_action(false, current_tool_keys != *last_known_mcp_tools),
                RefreshDecision::MarkPending
            ) {
                // Defer refresh to the next boundary so the active turn keeps a stable tool set.
                *pending_mcp_refresh = true;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RefreshDecision, decide_refresh_action};

    #[test]
    fn decide_refresh_action_marks_pending_on_change() {
        assert_eq!(
            decide_refresh_action(false, true),
            RefreshDecision::MarkPending
        );
    }

    #[test]
    fn decide_refresh_action_applies_pending_first() {
        assert_eq!(
            decide_refresh_action(true, true),
            RefreshDecision::ApplyPending
        );
        assert_eq!(
            decide_refresh_action(true, false),
            RefreshDecision::ApplyPending
        );
    }

    #[test]
    fn decide_refresh_action_is_none_when_stable() {
        assert_eq!(decide_refresh_action(false, false), RefreshDecision::None);
    }
}
