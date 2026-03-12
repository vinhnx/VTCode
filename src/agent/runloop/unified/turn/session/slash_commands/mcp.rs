use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::{InlineListItem, InlineListSelection};

use crate::agent::runloop::slash_commands::McpCommandAction;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::mcp_support::{
    diagnose_mcp, display_mcp_config_summary, display_mcp_providers, display_mcp_status,
    display_mcp_tools, refresh_mcp_tools, render_mcp_config_edit_guidance,
    render_mcp_login_guidance, repair_mcp_runtime,
};
use crate::agent::runloop::unified::session_setup::refresh_tool_snapshot;

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

async fn execute_mcp_action(
    ctx: &mut SlashCommandContext<'_>,
    action: McpCommandAction,
) -> Result<()> {
    let requires_live_tools = matches!(
        action,
        McpCommandAction::ListTools | McpCommandAction::RefreshTools | McpCommandAction::Repair
    );

    if !matches!(
        action,
        McpCommandAction::EditConfig | McpCommandAction::Login(_) | McpCommandAction::Logout(_)
    ) {
        super::activation::ensure_mcp_activated(ctx).await?;
        if !super::activation::try_attach_ready_mcp(ctx).await? && requires_live_tools {
            ctx.renderer.line(
                MessageStyle::Info,
                "MCP is initializing asynchronously. Run the command again in a moment.",
            )?;
            return Ok(());
        }
    }

    let manager = ctx.async_mcp_manager.map(|m| m.as_ref());
    match action {
        McpCommandAction::Interactive => {}
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
            if refresh_mcp_tools(ctx.renderer, ctx.tool_registry).await? {
                apply_manual_mcp_refresh(ctx, "mcp_manual_refresh").await;
            }
            sync_mcp_context_files_if_ready(ctx).await?;
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
            if repair_mcp_runtime(
                ctx.renderer,
                manager,
                ctx.tool_registry,
                ctx.vt_cfg.as_ref(),
            )
            .await?
            {
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

async fn apply_manual_mcp_refresh(ctx: &mut SlashCommandContext<'_>, reason: &'static str) {
    let tool_documentation_mode = ctx
        .vt_cfg
        .as_ref()
        .as_ref()
        .map(|cfg| cfg.agent.tool_documentation_mode)
        .unwrap_or_default();
    refresh_tool_snapshot(
        ctx.tool_registry,
        ctx.tools,
        ctx.tool_catalog,
        ctx.config,
        tool_documentation_mode,
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
        InlineListItem {
            title: "Status overview".to_string(),
            subtitle: Some("Show MCP runtime status and health".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}status",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("status overview health".to_string()),
        },
        InlineListItem {
            title: "List providers".to_string(),
            subtitle: Some("Show configured MCP providers".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}providers",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("providers list".to_string()),
        },
        InlineListItem {
            title: "List tools".to_string(),
            subtitle: Some("Show tools exposed by active providers".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}tools",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("tools list".to_string()),
        },
        InlineListItem {
            title: "Refresh tools".to_string(),
            subtitle: Some("Reload tool metadata from providers".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}refresh",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("refresh reload".to_string()),
        },
        InlineListItem {
            title: "Show config".to_string(),
            subtitle: Some("Display effective MCP configuration".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}config",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("config show".to_string()),
        },
        InlineListItem {
            title: "Edit config guidance".to_string(),
            subtitle: Some("Show how to edit MCP config files".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}edit",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("edit config".to_string()),
        },
        InlineListItem {
            title: "Repair runtime".to_string(),
            subtitle: Some("Restart providers and repair MCP runtime".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}repair",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("repair fix runtime".to_string()),
        },
        InlineListItem {
            title: "Diagnose".to_string(),
            subtitle: Some("Run deeper diagnostics for MCP issues".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}diagnose",
                MCP_ACTION_PREFIX
            ))),
            search_value: Some("diagnose diagnostics".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close interactive MCP manager".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                MCP_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close".to_string()),
        },
    ];

    ctx.renderer.show_list_modal(
        "MCP",
        vec![
            "Manage MCP providers and tools interactively.".to_string(),
            "Use Enter to run an action, Esc to close.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}status",
            MCP_ACTION_PREFIX
        ))),
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
