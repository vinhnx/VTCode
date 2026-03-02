use anyhow::Result;
use std::sync::Arc;
use tracing::warn;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;

use super::SlashCommandContext;

pub(super) async fn ensure_skills_context_activated(ctx: &SlashCommandContext<'_>) -> Result<()> {
    let Some(vt_cfg) = ctx.vt_cfg.as_ref() else {
        return Ok(());
    };

    vtcode_core::context::ensure_skills_dynamic_context(&ctx.config.workspace, &vt_cfg.context.dynamic)
        .await
}

pub(super) async fn ensure_mcp_activated(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let Some(manager) = ctx.async_mcp_manager else {
        return Ok(());
    };

    if let Err(err) = manager.start_initialization() {
        warn!("Failed to start MCP initialization: {}", err);
        ctx.renderer.line(
            MessageStyle::Error,
            &format!("Failed to start MCP runtime: {}", err),
        )?;
        return Ok(());
    }
    // Non-blocking: start initialization and return. Tool attachment happens in
    // async lifecycle updates, or immediately if already ready.
    let _ = try_attach_ready_mcp(ctx).await?;
    Ok(())
}

pub(super) async fn try_attach_ready_mcp(ctx: &mut SlashCommandContext<'_>) -> Result<bool> {
    let Some(manager) = ctx.async_mcp_manager else {
        return Ok(false);
    };

    match manager.get_status().await {
        McpInitStatus::Ready { client } => {
            if ctx.tool_registry.mcp_client().is_none() {
                ctx.tool_registry.set_mcp_client(Arc::clone(&client)).await;
                if let Err(err) = ctx.tool_registry.refresh_mcp_tools().await {
                    warn!("Failed to refresh MCP tools after activation: {}", err);
                }
                sync_mcp_context_files(ctx, &client).await?;
            }
            Ok(true)
        }
        McpInitStatus::Error { message } => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("MCP activation failed: {}", message))?;
            Ok(false)
        }
        McpInitStatus::Disabled | McpInitStatus::Initializing { .. } => Ok(false),
    }
}

pub(super) async fn sync_mcp_context_files(
    ctx: &SlashCommandContext<'_>,
    client: &Arc<vtcode_core::mcp::McpClient>,
) -> Result<()> {
    let Some(vt_cfg) = ctx.vt_cfg.as_ref() else {
        return Ok(());
    };

    let dynamic_cfg = &vt_cfg.context.dynamic;
    if !dynamic_cfg.enabled || !dynamic_cfg.sync_mcp_tools {
        return Ok(());
    }

    vtcode_core::context::ensure_mcp_dynamic_context(&ctx.config.workspace, dynamic_cfg).await?;
    if let Err(err) = client.sync_tools_to_files(&ctx.config.workspace).await {
        warn!("Failed to sync MCP tools to dynamic context files: {}", err);
    }

    Ok(())
}
