use anyhow::{Context, Result};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::diagnostics::{DoctorOptions, run_doctor_diagnostics};
use crate::agent::runloop::unified::ui_interaction::display_session_status;

use super::{SlashCommandContext, SlashCommandControl};

#[path = "diagnostics/memory.rs"]
mod memory;

const DOCTOR_ACTION_PREFIX: &str = "doctor.action.";
const DOCTOR_ACTION_BACK: &str = "doctor.action.back";

pub(crate) async fn handle_show_status(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let tool_count = ctx.tools.read().await.len();
    let active_instruction_directory = ctx
        .context_manager
        .active_instruction_directory_snapshot()
        .unwrap_or_else(|| ctx.config.workspace.clone());
    let instruction_context_paths = ctx.context_manager.instruction_context_paths_snapshot();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            vt_cfg: ctx.vt_cfg.as_ref(),
            active_instruction_directory: &active_instruction_directory,
            instruction_context_paths: &instruction_context_paths,
            message_count: ctx.conversation_history.len(),
            stats: ctx.session_stats,
            available_tools: tool_count,
            async_mcp_manager: ctx.async_mcp_manager.map(|manager| manager.as_ref()),
            loaded_skills: ctx.loaded_skills,
        },
    )
    .await?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_show_memory(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        memory::render_memory_status_lines(&mut ctx, false).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Next actions: `/memory` in inline UI, `/config memory`, or `/edit <target>`.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory controls")? {
        return Ok(SlashCommandControl::Continue);
    }

    memory::run_memory_modal(&mut ctx, false).await
}

pub(crate) async fn handle_show_memory_config(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        memory::render_memory_config_lines(&mut ctx).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Use `/memory` in inline UI for quick actions or `/config agent.persistent_memory` for the raw section.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    memory::run_memory_modal(&mut ctx, true).await
}

pub(crate) async fn handle_run_doctor(
    mut ctx: SlashCommandContext<'_>,
    quick: bool,
) -> Result<SlashCommandControl> {
    run_doctor(&mut ctx, quick).await?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_start_doctor_interactive(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        run_doctor(&mut ctx, false).await?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening doctor checks")? {
        return Ok(SlashCommandControl::Continue);
    }

    show_doctor_actions_modal(&mut ctx);
    let Some(selection) = super::ui::wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };

    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };

    if action == DOCTOR_ACTION_BACK {
        return Ok(SlashCommandControl::Continue);
    }

    let Some(action_key) = action.strip_prefix(DOCTOR_ACTION_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    match action_key {
        "quick" => run_doctor(&mut ctx, true).await?,
        "full" => run_doctor(&mut ctx, false).await?,
        _ => {}
    }

    Ok(SlashCommandControl::Continue)
}

async fn run_doctor(ctx: &mut SlashCommandContext<'_>, quick: bool) -> Result<()> {
    let provider_runtime = ctx.provider_client.name().to_string();
    run_doctor_diagnostics(
        ctx.renderer,
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &provider_runtime,
        ctx.async_mcp_manager.map(|m| m.as_ref()),
        ctx.linked_directories,
        Some(ctx.loaded_skills),
        DoctorOptions { quick },
    )
    .await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(())
}

pub(crate) async fn handle_start_terminal_setup(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let vt_cfg = ctx
        .vt_cfg
        .as_ref()
        .context("VT Code configuration not available")?;
    vtcode_core::terminal_setup::run_terminal_setup_wizard(ctx.renderer, vt_cfg).await?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

fn show_doctor_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let items = vec![
        InlineListItem {
            title: "Run full diagnostics".to_string(),
            subtitle: Some(
                "Run all checks: config, provider key, dependencies, MCP, links, and skills"
                    .to_string(),
            ),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}full",
                DOCTOR_ACTION_PREFIX
            ))),
            search_value: Some("doctor full all checks mcp dependencies".to_string()),
        },
        InlineListItem {
            title: "Run quick diagnostics".to_string(),
            subtitle: Some(
                "Run core checks only (skips dependencies, MCP, links, and skills)".to_string(),
            ),
            badge: Some("Fast".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}quick",
                DOCTOR_ACTION_PREFIX
            ))),
            search_value: Some("doctor quick fast checks".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close without running diagnostics".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                DOCTOR_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close cancel".to_string()),
        },
    ];

    ctx.renderer.show_list_modal(
        "Doctor",
        vec![
            "Choose how to run VT Code diagnostics.".to_string(),
            "Use Enter to run an action, Esc to close.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}full",
            DOCTOR_ACTION_PREFIX
        ))),
        None,
    );
}
