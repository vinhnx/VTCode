use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::InlineListSelection;

use crate::agent::runloop::slash_commands::McpCommandAction;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::mcp_support::{
    diagnose_mcp, display_mcp_config_summary, display_mcp_providers, display_mcp_status, display_mcp_tools,
    refresh_mcp_tools, render_mcp_config_edit_guidance, render_mcp_login_guidance, repair_mcp_runtime,
};
use crate::agent::runloop::unified::session_setup::{active_deferred_tool_policy, refresh_tool_snapshot};

use super::{SlashCommandContext, SlashCommandControl};

const MCP_ACTION_PREFIX: &str = "mcp.action.";
const MCP_ACTION_BACK: &str = "mcp.action.back";

pub(crate) async fn handle_manage_mcp(
    mut ctx: SlashCommandContext<'_>,
    action: McpCommandAction,
) -> Result<SlashCommandControl> {
    if matches!(action, McpCommandAction::Interactive) {
        run_interactive_mcp_manager(&mut ctx).await?;
        ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
        return Ok(SlashCommandControl::Continue);
    }

    execute_mcp_action(&mut ctx, action).await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

async fn execute_mcp_action(ctx: &mut SlashCommandContext<'_>, action: McpCommandAction) -> Result<()> {
    let requires_live_tools =
        matches!(action, McpCommandAction::ListTools | McpCommandAction::RefreshTools | McpCommandAction::Repair);

    if !matches!(action, McpCommandAction::EditConfig | McpCommandAction::Login(_) | McpCommandAction::Logout(_)) {
        super::activation::ensure_mcp_activated(ctx).await?;
        if !super::activation::try_attach_ready_mcp(ctx).await? && requires_live_tools {
            ctx.renderer
                .line(MessageStyle::Info, "MCP is initializing asynchronously. Run the command again in a moment.")?;
            return Ok(());
        }
    }

    let manager = ctx.async_mcp_manager.map(|m| m.as_ref());
    match action {
        McpCommandAction::Interactive => {}
        McpCommandAction::Overview => {
            display_mcp_status(ctx.renderer, ctx.session_bootstrap, ctx.tool_registry, manager, ctx.mcp_panel_state)
                .await?;
        }
        McpCommandAction::ListProviders => {
            display_mcp_providers(ctx.renderer, ctx.session_bootstrap, manager).await?;
        }
        McpCommandAction::ListTools => {
            display_mcp_tools(ctx.renderer, ctx.tool_registry).await?;
        }
        McpCommandAction::RefreshTools => {
            if refresh_mcp_tools(ctx.renderer, ctx.tool_registry).await? {
                apply_manual_mcp_refresh(ctx, "mcp_manual_refresh").await;
            }
            sync_mcp_context_files_if_ready(ctx).await?;
        }
        McpCommandAction::ShowConfig => {
            display_mcp_config_summary(ctx.renderer, ctx.vt_cfg.as_ref(), ctx.session_bootstrap, manager).await?;
        }
        McpCommandAction::EditConfig => {
            render_mcp_config_edit_guidance(ctx.renderer, ctx.config.workspace.as_path()).await?;
        }
        McpCommandAction::Repair => {
            if repair_mcp_runtime(ctx.renderer, manager, ctx.tool_registry, ctx.vt_cfg.as_ref()).await? {
                apply_manual_mcp_refresh(ctx, "mcp_repair_refresh").await;
            }
            sync_mcp_context_files_if_ready(ctx).await?;
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
    Ok(())
}

pub(super) async fn apply_manual_mcp_refresh(ctx: &mut SlashCommandContext<'_>, reason: &'static str) {
    let tool_documentation_mode = ctx
        .vt_cfg
        .as_ref()
        .as_ref()
        .map(|cfg| cfg.agent.tool_documentation_mode)
        .unwrap_or_default();
    let deferred_tool_policy = active_deferred_tool_policy(ctx.config, ctx.vt_cfg.as_ref(), &**ctx.provider_client);
    refresh_tool_snapshot(
        ctx.tool_registry,
        ctx.tools,
        ctx.tool_catalog,
        ctx.config,
        ctx.vt_cfg.as_ref(),
        tool_documentation_mode,
        &deferred_tool_policy,
    )
    .await;
    ctx.tool_catalog.note_explicit_refresh(reason);
}

async fn run_interactive_mcp_manager(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    if !ctx.renderer.supports_inline_ui() {
        execute_mcp_action(ctx, McpCommandAction::Overview).await?;
        return Ok(());
    }

    loop {
        show_mcp_actions_modal(ctx);
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == MCP_ACTION_BACK {
            return Ok(());
        }

        let Some(action_key) = action.strip_prefix(MCP_ACTION_PREFIX) else {
            continue;
        };
        let mapped = match action_key {
            "status" => McpCommandAction::Overview,
            "providers" => McpCommandAction::ListProviders,
            "tools" => McpCommandAction::ListTools,
            "refresh" => McpCommandAction::RefreshTools,
            "config" => McpCommandAction::ShowConfig,
            "edit" => McpCommandAction::EditConfig,
            "repair" => McpCommandAction::Repair,
            "diagnose" => McpCommandAction::Diagnose,
            _ => continue,
        };
        execute_mcp_action(ctx, mapped).await?;
    }
}

fn show_mcp_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let items = vec![
        ui_list::action(
            "Status overview",
            "Show MCP runtime status and health".to_string(),
            Some("Recommended".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}status"))),
        )
        .with_search_value("status overview health".to_string()),
        ui_list::action(
            "List providers",
            "Show configured MCP providers".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}providers"))),
        )
        .with_search_value("providers list".to_string()),
        ui_list::action(
            "List tools",
            "Show tools exposed by active providers".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}tools"))),
        )
        .with_search_value("tools list".to_string()),
        ui_list::action(
            "Refresh tools",
            "Reload tool metadata from providers".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}refresh"))),
        )
        .with_search_value("refresh reload".to_string()),
        ui_list::action(
            "Show config",
            "Display effective MCP configuration".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}config"))),
        )
        .with_search_value("config show".to_string()),
        ui_list::action(
            "Edit config guidance",
            "Show how to edit MCP config files".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}edit"))),
        )
        .with_search_value("edit config".to_string()),
        ui_list::action(
            "Repair runtime",
            "Restart providers and repair MCP runtime".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}repair"))),
        )
        .with_search_value("repair fix runtime".to_string()),
        ui_list::action(
            "Diagnose",
            "Run deeper diagnostics for MCP issues".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}diagnose"))),
        )
        .with_search_value("diagnose diagnostics".to_string()),
        ui_list::action(
            "Back",
            "Close interactive MCP manager".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(MCP_ACTION_BACK.to_string())),
        )
        .with_search_value("back close".to_string()),
    ];

    ctx.renderer.show_list_modal(
        "MCP",
        vec!["Manage MCP providers and tools interactively.".to_string()],
        items,
        Some(InlineListSelection::ConfigAction(format!("{MCP_ACTION_PREFIX}status"))),
        None,
    );
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
