use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::slash_commands::McpCommandAction;
use crate::agent::runloop::unified::mcp_support::{
    diagnose_mcp, display_mcp_config_summary, display_mcp_providers, display_mcp_status,
    display_mcp_tools, refresh_mcp_tools, render_mcp_config_edit_guidance,
    render_mcp_login_guidance, repair_mcp_runtime,
};

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_manage_mcp(
    ctx: SlashCommandContext<'_>,
    action: McpCommandAction,
) -> Result<SlashCommandControl> {
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
