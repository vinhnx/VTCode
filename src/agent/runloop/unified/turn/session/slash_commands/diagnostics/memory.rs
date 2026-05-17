//! Agent Legibility:
//! - Entrypoint: `run_memory_modal` owns the diagnostics memory workflow for settings, cleanup, and lightweight-model routing.
//! - Common changes:
//!   - Persistent-memory modal actions stay in this root, while config persistence and prompt helpers live in `memory/` support modules.
//!   - Shared slash-command UI helpers remain in the surrounding diagnostics and UI modules.
//! - Constraints: This file is still part of active TD-005 debt; keep new modal branches factored into named helpers rather than extending the root flow.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode slash_commands`

#[path = "memory/config_persistence.rs"]
mod config_persistence;
#[path = "memory/navigation.rs"]
mod navigation;
#[path = "memory/presentation.rs"]
mod presentation;
#[path = "memory/prompts.rs"]
mod prompts;

use anyhow::{Context, Result, bail};
use vtcode_core::persistent_memory::{
    PersistentMemoryStatus, cleanup_persistent_memory, persistent_memory_status,
    rebuild_persistent_memory_summary, scaffold_persistent_memory,
};
use vtcode_core::project_doc::load_instruction_appendix;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::InlineListSelection;

use crate::agent::runloop::unified::ui_interaction::{
    instruction_memory_map, start_loading_status,
};

use self::config_persistence::{
    parse_positive_usize, persist_user_directory_override, persist_workspace_config_change,
    set_workspace_instruction_excludes, set_workspace_instruction_import_depth,
    set_workspace_memory_auto_write, set_workspace_memory_byte_limit, set_workspace_memory_enabled,
    set_workspace_memory_line_limit, set_workspace_small_model_for_memory,
    set_workspace_small_model_model,
};
use self::navigation::handle_memory_navigation_action;
use self::presentation::{
    format_path_list, memory_lightweight_route_info, render_common_memory_status,
    show_memory_actions_modal,
};
use self::prompts::{prompt_optional_text, prompt_required_text};
use super::super::ui::wait_for_list_modal_selection;
use super::{SlashCommandContext, SlashCommandControl};

const MEMORY_ACTION_PREFIX: &str = "memory.action.";
const MEMORY_ACTION_BACK: &str = "memory.action.back";
const MEMORY_PROMPT_QUESTION_ID: &str = "memory.input";
const MEMORY_LIGHTWEIGHT_MODEL_PREFIX: &str = "lightweight_model:";

pub(super) async fn run_memory_modal(
    ctx: &mut SlashCommandContext<'_>,
    config_mode: bool,
) -> Result<SlashCommandControl> {
    loop {
        let agent_config = ctx
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.clone())
            .unwrap_or_default();
        let active_dir = ctx
            .context_manager
            .active_instruction_directory_snapshot()
            .unwrap_or_else(|| ctx.config.workspace.clone());
        let match_paths = ctx.context_manager.instruction_context_paths_snapshot();
        let appendix = load_instruction_appendix(&agent_config, &active_dir, &match_paths).await;
        let memory_status =
            persistent_memory_status(&agent_config.persistent_memory, &ctx.config.workspace)?;
        let (agents, matched_rules) = instruction_memory_map(appendix.as_ref());

        show_memory_actions_modal(ctx, config_mode, &memory_status, &agents, &matched_rules);
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(SlashCommandControl::Continue);
        };
        let InlineListSelection::ConfigAction(action) = selection else {
            return Ok(SlashCommandControl::Continue);
        };
        if action == MEMORY_ACTION_BACK {
            return Ok(SlashCommandControl::Continue);
        }

        let Some(action_key) = action.strip_prefix(MEMORY_ACTION_PREFIX) else {
            return Ok(SlashCommandControl::Continue);
        };
        if let Some(control) =
            handle_memory_action(ctx, action_key, &memory_status, config_mode).await?
        {
            return Ok(control);
        }
    }
}

pub(super) async fn render_memory_status_lines(
    ctx: &mut SlashCommandContext<'_>,
    include_config_hint: bool,
) -> Result<()> {
    let agent_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.clone())
        .unwrap_or_default();
    let active_dir = ctx
        .context_manager
        .active_instruction_directory_snapshot()
        .unwrap_or_else(|| ctx.config.workspace.clone());
    let match_paths = ctx.context_manager.instruction_context_paths_snapshot();
    let appendix = load_instruction_appendix(&agent_config, &active_dir, &match_paths).await;
    let memory_status =
        persistent_memory_status(&agent_config.persistent_memory, &ctx.config.workspace)?;
    let (agents, matched_rules) = instruction_memory_map(appendix.as_ref());
    let lightweight_route = memory_lightweight_route_info(ctx.config, ctx.vt_cfg.as_ref());

    ctx.renderer
        .line(MessageStyle::Info, "Instruction Memory")?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Loaded AGENTS.md sources: {}", format_path_list(&agents)),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Matched rules: {}", format_path_list(&matched_rules)),
    )?;
    render_common_memory_status(ctx, &memory_status)?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Memory triage model: {} ({})",
            lightweight_route.configured_label, lightweight_route.effective_label
        ),
    )?;
    if let Some(warning) = lightweight_route.warning {
        ctx.renderer.line(
            MessageStyle::Warning,
            &format!("Route warning: {}", warning),
        )?;
    }
    if include_config_hint {
        ctx.renderer.line(
            MessageStyle::Info,
            "Focused controls: `/config memory` or `/config agent.persistent_memory`.",
        )?;
    }

    Ok(())
}

pub(super) async fn render_memory_config_lines(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let agent_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.clone())
        .unwrap_or_default();
    let memory_status =
        persistent_memory_status(&agent_config.persistent_memory, &ctx.config.workspace)?;
    let lightweight_route = memory_lightweight_route_info(ctx.config, ctx.vt_cfg.as_ref());

    ctx.renderer.line(MessageStyle::Info, "Memory Settings")?;
    render_common_memory_status(ctx, &memory_status)?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Startup budgets: {} lines, {} bytes",
            agent_config.persistent_memory.startup_line_limit,
            agent_config.persistent_memory.startup_byte_limit
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Import depth: {} | instruction excludes: {}",
            agent_config.instruction_import_max_depth,
            agent_config.instruction_excludes.len()
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Memory triage model: {}",
            lightweight_route.configured_label
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Effective route: {}", lightweight_route.effective_label),
    )?;
    if let Some(warning) = lightweight_route.warning {
        ctx.renderer.line(
            MessageStyle::Warning,
            &format!("Route warning: {}", warning),
        )?;
    }

    Ok(())
}
async fn handle_memory_action(
    ctx: &mut SlashCommandContext<'_>,
    action_key: &str,
    memory_status: &PersistentMemoryStatus,
    _config_mode: bool,
) -> Result<Option<SlashCommandControl>> {
    if let Some(control) = handle_memory_navigation_action(ctx, action_key, memory_status).await? {
        return Ok(Some(control));
    }

    if let Some(selection) = action_key.strip_prefix(MEMORY_LIGHTWEIGHT_MODEL_PREFIX) {
        let model = match selection {
            "auto" => String::new(),
            "main" => ctx.config.model.clone(),
            explicit => explicit.to_string(),
        };
        persist_workspace_config_change(ctx, move |root| {
            set_workspace_small_model_model(root, model);
            Ok(())
        })
        .await?;
        ctx.renderer
            .line(MessageStyle::Info, "Updated the memory triage model.")?;
        return Ok(None);
    }

    match action_key {
        "toggle_enabled" => {
            let enabled = ctx
                .vt_cfg
                .as_ref()
                .map(vtcode_core::config::loader::VTCodeConfig::persistent_memory_enabled)
                .unwrap_or(true);
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_memory_enabled(root, !enabled);
                Ok(())
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Toggled persistent memory.")?;
        }
        "toggle_auto_write" => {
            let auto_write = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.auto_write)
                .unwrap_or(true);
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_memory_auto_write(root, !auto_write);
                Ok(())
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Toggled auto-write.")?;
        }
        "toggle_small_model" => {
            let enabled = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.small_model.use_for_memory)
                .unwrap_or(true);
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_small_model_for_memory(root, !enabled);
                Ok(())
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Toggled lightweight memory routing.")?;
        }
        "set_lines" => {
            let current = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.startup_line_limit)
                .unwrap_or(200);
            let Some(value) = prompt_required_text(
                ctx,
                "Startup Line Limit",
                "Enter the number of `memory_summary.md` lines to inject at startup.",
                "Lines",
                &current.to_string(),
                Some(current.to_string()),
            )
            .await?
            else {
                return Ok(None);
            };
            let parsed = parse_positive_usize(&value, "startup line limit")?;
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_memory_line_limit(root, parsed)
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Updated startup line limit.")?;
        }
        "set_bytes" => {
            let current = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.startup_byte_limit)
                .unwrap_or(25_600);
            let Some(value) = prompt_required_text(
                ctx,
                "Startup Byte Limit",
                "Enter the byte budget loaded from `memory_summary.md` at startup.",
                "Bytes",
                &current.to_string(),
                Some(current.to_string()),
            )
            .await?
            else {
                return Ok(None);
            };
            let parsed = parse_positive_usize(&value, "startup byte limit")?;
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_memory_byte_limit(root, parsed)
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Updated startup byte limit.")?;
        }
        "set_import_depth" => {
            let current = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.instruction_import_max_depth)
                .unwrap_or(5);
            let Some(value) = prompt_required_text(
                ctx,
                "Instruction Import Depth",
                "Enter the maximum recursive `@path` import depth for AGENTS.md and rules.",
                "Depth",
                &current.to_string(),
                Some(current.to_string()),
            )
            .await?
            else {
                return Ok(None);
            };
            let parsed = parse_positive_usize(&value, "instruction import depth")?;
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_instruction_import_depth(root, parsed)
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Updated instruction import depth.")?;
        }
        "set_directory_override" => {
            let placeholder = memory_status.directory.display().to_string();
            let Some(value) = prompt_optional_text(
                ctx,
                "Directory Override",
                "Enter a user-level persistent memory directory override.",
                "Directory",
                &placeholder,
                ctx.vt_cfg
                    .as_ref()
                    .and_then(|cfg| cfg.agent.persistent_memory.directory_override.clone()),
            )
            .await?
            else {
                return Ok(None);
            };
            persist_user_directory_override(ctx, Some(value.trim().to_string())).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Updated the user memory directory override.",
            )?;
        }
        "clear_directory_override" => {
            persist_user_directory_override(ctx, None).await?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Cleared the user memory directory override.",
            )?;
        }
        "add_instruction_exclude" => {
            let Some(value) = prompt_required_text(
                ctx,
                "Instruction Exclude",
                "Add an exclude glob for AGENTS.md or `.vtcode/rules/` discovery.",
                "Pattern",
                "**/other-team/.vtcode/rules/**",
                None,
            )
            .await?
            else {
                return Ok(None);
            };
            let value = value.trim().to_string();
            let mut excludes = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.instruction_excludes.clone())
                .unwrap_or_default();
            if !excludes.iter().any(|entry| entry == &value) {
                excludes.push(value);
            }
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_instruction_excludes(root, excludes);
                Ok(())
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Added instruction exclude.")?;
        }
        "remove_instruction_exclude" => {
            let Some(value) = prompt_required_text(
                ctx,
                "Remove Instruction Exclude",
                "Enter the exact exclude pattern to remove.",
                "Pattern",
                "**/other-team/.vtcode/rules/**",
                None,
            )
            .await?
            else {
                return Ok(None);
            };
            let value = value.trim().to_string();
            let mut excludes = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.instruction_excludes.clone())
                .unwrap_or_default();
            excludes.retain(|entry| entry != &value);
            persist_workspace_config_change(ctx, move |root| {
                set_workspace_instruction_excludes(root, excludes);
                Ok(())
            })
            .await?;
            ctx.renderer
                .line(MessageStyle::Info, "Removed matching instruction excludes.")?;
        }
        "scaffold" => {
            let persistent_memory_config = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.clone())
                .unwrap_or_default();
            ctx.renderer
                .line(MessageStyle::Info, "Scaffolding persistent memory files...")?;
            let spinner = start_loading_status(
                ctx.handle,
                ctx.input_status_state,
                "Scaffolding memory files...",
            );
            let status =
                scaffold_persistent_memory(&persistent_memory_config, &ctx.config.workspace)
                    .await?
                    .context("Persistent memory is disabled.")?;
            drop(spinner);
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Scaffolded memory files under {}.",
                    status.directory.display()
                ),
            )?;
        }
        "cleanup" => {
            ctx.renderer
                .line(MessageStyle::Info, "Cleaning persistent memory...")?;
            let spinner = start_loading_status(
                ctx.handle,
                ctx.input_status_state,
                "Cleaning persistent memory...",
            );
            let report = cleanup_persistent_memory(ctx.config, ctx.vt_cfg.as_ref(), true)
                .await?
                .context("Persistent memory is disabled.")?;
            drop(spinner);
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Cleaned persistent memory under {}. Rewritten facts: {}. Removed rollout files: {}.",
                    report.directory.display(),
                    report.rewritten_facts,
                    report.removed_rollout_files
                ),
            )?;
        }
        "rebuild" => {
            ctx.renderer.line(
                MessageStyle::Info,
                "Rebuilding persistent memory summary...",
            )?;
            let spinner = start_loading_status(
                ctx.handle,
                ctx.input_status_state,
                "Rebuilding memory summary...",
            );
            rebuild_persistent_memory_summary(ctx.config, ctx.vt_cfg.as_ref())
                .await?
                .context("Persistent memory is disabled.")?;
            drop(spinner);
            ctx.renderer
                .line(MessageStyle::Info, "Rebuilt memory summary and registry.")?;
        }
        _ => bail!("Unknown memory action: {}", action_key),
    }

    Ok(None)
}
