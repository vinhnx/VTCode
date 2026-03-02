use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::McpCommandAction;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::mcp_support::{
    diagnose_mcp, display_mcp_config_summary, display_mcp_providers, display_mcp_status,
    display_mcp_tools, refresh_mcp_tools, render_mcp_config_edit_guidance,
    render_mcp_login_guidance, repair_mcp_runtime,
};

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_manage_mcp(
    mut ctx: SlashCommandContext<'_>,
    action: McpCommandAction,
) -> Result<SlashCommandControl> {
    let requires_live_tools = matches!(
        action,
        McpCommandAction::ListTools | McpCommandAction::RefreshTools | McpCommandAction::Repair
    );

    if !matches!(
        action,
        McpCommandAction::EditConfig | McpCommandAction::Login(_) | McpCommandAction::Logout(_)
    ) {
        super::activation::ensure_mcp_activated(&mut ctx).await?;
        if !super::activation::try_attach_ready_mcp(&mut ctx).await?
            && requires_live_tools
        {
            ctx.renderer.line(
                MessageStyle::Info,
                "MCP is initializing asynchronously. Run the command again in a moment.",
            )?;
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            return Ok(SlashCommandControl::Continue);
        }
    }

    let manager = ctx.async_mcp_manager.map(|m| m.as_ref());
    match action {
        McpCommandAction::Overview => {
            display_mcp_status(
                ctx.renderer,
                ctx.session_bootstrap,
                ctx.tool_registry,
                manager,
                ctx.mcp_panel_state,
            )
            .await?;
        }
        McpCommandAction::ListProviders => {
            display_mcp_providers(ctx.renderer, ctx.session_bootstrap, manager).await?;
        }
        McpCommandAction::ListTools => {
            display_mcp_tools(ctx.renderer, ctx.tool_registry).await?;
        }
        McpCommandAction::RefreshTools => {
            refresh_mcp_tools(ctx.renderer, ctx.tool_registry).await?;
            sync_mcp_context_files_if_ready(&ctx).await?;
        }
        McpCommandAction::ShowConfig => {
            display_mcp_config_summary(
                ctx.renderer,
                ctx.vt_cfg.as_ref(),
                ctx.session_bootstrap,
                manager,
            )
            .await?;
        }
        McpCommandAction::EditConfig => {
            render_mcp_config_edit_guidance(ctx.renderer, ctx.config.workspace.as_path()).await?;
        }
        McpCommandAction::Repair => {
            repair_mcp_runtime(
                ctx.renderer,
                manager,
                ctx.tool_registry,
                ctx.vt_cfg.as_ref(),
            )
            .await?;
            sync_mcp_context_files_if_ready(&ctx).await?;
        }
        McpCommandAction::Diagnose => {
            diagnose_mcp(
                ctx.renderer,
                ctx.vt_cfg.as_ref(),
                ctx.session_bootstrap,
                manager,
                ctx.tool_registry,
                ctx.mcp_panel_state,
            )
            .await?;
        }
        McpCommandAction::Login(name) => {
            render_mcp_login_guidance(ctx.renderer, name, true)?;
        }
        McpCommandAction::Logout(name) => {
            render_mcp_login_guidance(ctx.renderer, name, false)?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

async fn sync_mcp_context_files_if_ready(ctx: &SlashCommandContext<'_>) -> Result<()> {
    let Some(manager) = ctx.async_mcp_manager else {
        return Ok(());
    };
    if let McpInitStatus::Ready { client } = manager.get_status().await {
        super::activation::sync_mcp_context_files(ctx, &client).await?;
    }
    Ok(())
}
