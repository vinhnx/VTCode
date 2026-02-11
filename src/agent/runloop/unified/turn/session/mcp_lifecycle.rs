use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;

use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_mcp_updates(
    mcp_manager: &AsyncMcpManager,
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    tools: &Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    tool_catalog: &ToolCatalogState,
    renderer: &mut AnsiRenderer,
    mcp_catalog_initialized: &mut bool,
    last_mcp_refresh: &mut Instant,
    last_known_mcp_tools: &mut Vec<String>,
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
                                    mcp_tools,
                                    last_known_mcp_tools,
                                )
                                .await?;
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

        if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
            let current_tool_keys: Vec<String> = known_tools
                .iter()
                .map(|t| format!("{}-{}", t.provider, t.name))
                .collect();

            if current_tool_keys != *last_known_mcp_tools {
                match tool_registry.refresh_mcp_tools().await {
                    Ok(()) => match tool_registry.list_mcp_tools().await {
                        Ok(_) => {
                            McpToolManager::enumerate_mcp_tools_after_refresh(
                                tool_registry,
                                tools,
                                tool_catalog,
                                last_known_mcp_tools,
                            )
                            .await?;
                        }
                        Err(err) => {
                            tracing::warn!("Failed to enumerate MCP tools after refresh: {err}");
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to refresh MCP tools during dynamic update: {err}");
                    }
                }
            }
        }
    }

    Ok(())
}
