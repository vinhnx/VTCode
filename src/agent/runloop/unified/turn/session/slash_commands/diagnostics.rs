use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use toml::Value as TomlValue;
use vtcode_core::config::current_config_defaults;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::loader::layers::ConfigLayerSource;
use vtcode_core::instructions::{InstructionSourceKind, format_instruction_path};
use vtcode_core::persistent_memory::{
    PersistentMemoryStatus, cleanup_persistent_memory, persistent_memory_status,
    rebuild_persistent_memory_summary, scaffold_persistent_memory,
};
use vtcode_core::project_doc::load_instruction_appendix;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::unified::diagnostics::{DoctorOptions, run_doctor_diagnostics};
use crate::agent::runloop::unified::palettes::refresh_runtime_config_from_manager;
use crate::agent::runloop::unified::ui_interaction::display_session_status;
use crate::agent::runloop::unified::ui_interaction::start_loading_status;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

use super::{SlashCommandContext, SlashCommandControl};

const DOCTOR_ACTION_PREFIX: &str = "doctor.action.";
const DOCTOR_ACTION_BACK: &str = "doctor.action.back";
const MEMORY_ACTION_PREFIX: &str = "memory.action.";
const MEMORY_ACTION_BACK: &str = "memory.action.back";
const MEMORY_PROMPT_QUESTION_ID: &str = "memory.input";

pub(crate) async fn handle_show_status(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let tool_count = ctx.tools.read().await.len();
    display_session_status(
        ctx.renderer,
        crate::agent::runloop::unified::ui_interaction::SessionStatusContext {
            config: ctx.config,
            vt_cfg: ctx.vt_cfg.as_ref(),
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
        render_memory_status_lines(&mut ctx, false).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Next actions: `/memory` in inline UI, `/config memory`, or `/edit <target>`.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory controls")? {
        return Ok(SlashCommandControl::Continue);
    }

    run_memory_modal(&mut ctx, false).await
}

pub(crate) async fn handle_show_memory_config(
    mut ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        render_memory_config_lines(&mut ctx).await?;
        ctx.renderer.line(
            MessageStyle::Info,
            "Use `/memory` in inline UI for quick actions or `/config agent.persistent_memory` for the raw section.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    if !super::ui::ensure_selection_ui_available(&mut ctx, "opening memory settings")? {
        return Ok(SlashCommandControl::Continue);
    }

    run_memory_modal(&mut ctx, true).await
}

async fn run_memory_modal(
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
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
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

async fn render_memory_status_lines(
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
    if include_config_hint {
        ctx.renderer.line(
            MessageStyle::Info,
            "Focused controls: `/config memory` or `/config agent.persistent_memory`.",
        )?;
    }

    Ok(())
}

async fn render_memory_config_lines(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    let agent_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.clone())
        .unwrap_or_default();
    let memory_status =
        persistent_memory_status(&agent_config.persistent_memory, &ctx.config.workspace)?;

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
            "Small model for memory: {}",
            if agent_config.small_model.use_for_memory {
                "enabled"
            } else {
                "disabled"
            }
        ),
    )?;

    Ok(())
}

fn render_common_memory_status(
    ctx: &mut SlashCommandContext<'_>,
    memory_status: &PersistentMemoryStatus,
) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Persistent memory: {} (auto-write: {})",
            if memory_status.enabled {
                "enabled"
            } else {
                "disabled"
            },
            if memory_status.auto_write {
                "on"
            } else {
                "off"
            }
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Memory directory: {}", memory_status.directory.display()),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Summary: {} ({})",
            memory_status.summary_file.display(),
            if memory_status.summary_exists {
                "present"
            } else {
                "missing"
            }
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Registry: {} ({})",
            memory_status.memory_file.display(),
            if memory_status.registry_exists {
                "present"
            } else {
                "missing"
            }
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Rollouts: {} (pending: {})",
            memory_status.rollout_summaries_dir.display(),
            memory_status.pending_rollout_summaries
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Cleanup required: {} (facts: {}, summary lines: {})",
            if memory_status.cleanup_status.needed {
                "yes"
            } else {
                "no"
            },
            memory_status.cleanup_status.suspicious_facts,
            memory_status.cleanup_status.suspicious_summary_lines,
        ),
    )?;
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Open/edit targets: `/edit {}`, `/edit {}`, or `/edit {}`",
            memory_status.summary_file.display(),
            memory_status.memory_file.display(),
            memory_status.directory.display()
        ),
    )?;
    Ok(())
}

fn instruction_memory_map(
    appendix: Option<&vtcode_core::project_doc::InstructionAppendixBundle>,
) -> (Vec<String>, Vec<String>) {
    let Some(bundle) = appendix else {
        return (Vec::new(), Vec::new());
    };
    let Some(project_doc) = bundle.project_doc.as_ref() else {
        return (Vec::new(), Vec::new());
    };

    let agents = project_doc
        .segments
        .iter()
        .filter(|segment| matches!(segment.source.kind, InstructionSourceKind::Agents))
        .map(|segment| {
            format_instruction_path(
                &segment.source.path,
                bundle.project_root.as_path(),
                bundle.home_dir.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    let matched_rules = project_doc
        .segments
        .iter()
        .filter(|segment| {
            matches!(segment.source.kind, InstructionSourceKind::Rule) && segment.source.matched
        })
        .map(|segment| {
            format_instruction_path(
                &segment.source.path,
                bundle.project_root.as_path(),
                bundle.home_dir.as_deref(),
            )
        })
        .collect::<Vec<_>>();

    (agents, matched_rules)
}

fn show_memory_actions_modal(
    ctx: &mut SlashCommandContext<'_>,
    config_mode: bool,
    memory_status: &PersistentMemoryStatus,
    agents: &[String],
    matched_rules: &[String],
) {
    let agent_config = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.agent.clone())
        .unwrap_or_default();
    let title = if config_mode {
        "Memory Settings"
    } else {
        "Instruction Memory"
    };

    let mut lines = if config_mode {
        vec![
            "Focused settings for persistent memory and instruction imports.".to_string(),
            format!(
                "Startup budgets: {} lines, {} bytes | import depth: {}",
                agent_config.persistent_memory.startup_line_limit,
                agent_config.persistent_memory.startup_byte_limit,
                agent_config.instruction_import_max_depth,
            ),
        ]
    } else {
        vec![
            format!("Loaded AGENTS.md sources: {}", format_path_list(agents)),
            format!("Matched rules: {}", format_path_list(matched_rules)),
        ]
    };
    lines.push(format!(
        "Memory {} • auto-write {} • small-model {} • pending rollouts {} • cleanup {}",
        if memory_status.enabled { "on" } else { "off" },
        if memory_status.auto_write {
            "on"
        } else {
            "off"
        },
        if agent_config.small_model.use_for_memory {
            "on"
        } else {
            "off"
        },
        memory_status.pending_rollout_summaries,
        if memory_status.cleanup_status.needed {
            "needed"
        } else {
            "clean"
        },
    ));

    let items = vec![
        InlineListItem {
            title: toggle_title("Persistent memory", memory_status.enabled),
            subtitle: Some(
                "Toggle per-repo memory summary injection and learned memory files.".to_string(),
            ),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}toggle_enabled",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory enabled disable toggle".to_string()),
        },
        InlineListItem {
            title: toggle_title("Auto-write", memory_status.auto_write),
            subtitle: Some(
                "Write one rollout summary at session finalization, then consolidate it."
                    .to_string(),
            ),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}toggle_auto_write",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory auto write toggle".to_string()),
        },
        InlineListItem {
            title: toggle_title(
                "Small Model For Memory",
                agent_config.small_model.use_for_memory,
            ),
            subtitle: Some(
                "Use the small-model tier for memory classification and summary refresh."
                    .to_string(),
            ),
            badge: Some("Toggle".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}toggle_small_model",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory small model toggle".to_string()),
        },
        InlineListItem {
            title: format!(
                "Startup Line Limit ({})",
                agent_config.persistent_memory.startup_line_limit
            ),
            subtitle: Some("Set the number of summary lines injected at startup.".to_string()),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}set_lines",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory startup line limit".to_string()),
        },
        InlineListItem {
            title: format!(
                "Startup Byte Limit ({})",
                agent_config.persistent_memory.startup_byte_limit
            ),
            subtitle: Some("Set the startup byte budget for `memory_summary.md`.".to_string()),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}set_bytes",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory startup byte limit".to_string()),
        },
        InlineListItem {
            title: format!(
                "Instruction Import Depth ({})",
                agent_config.instruction_import_max_depth
            ),
            subtitle: Some(
                "Set recursive `@path` import depth for AGENTS.md and rules.".to_string(),
            ),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}set_import_depth",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory instruction import depth".to_string()),
        },
        InlineListItem {
            title: "Set Directory Override".to_string(),
            subtitle: Some(
                match agent_config.persistent_memory.directory_override.as_deref() {
                    Some(value) if !value.trim().is_empty() => format!("Current: {}", value),
                    _ => {
                        "Write a user-level override for the memory storage directory.".to_string()
                    }
                },
            ),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}set_directory_override",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory directory override set".to_string()),
        },
        InlineListItem {
            title: "Clear Directory Override".to_string(),
            subtitle: Some("Remove the user-level memory directory override.".to_string()),
            badge: Some("Action".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}clear_directory_override",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory directory override clear".to_string()),
        },
        InlineListItem {
            title: "Add Instruction Exclude".to_string(),
            subtitle: Some(format!(
                "Current excludes: {}",
                agent_config.instruction_excludes.len()
            )),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}add_instruction_exclude",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory instruction excludes add".to_string()),
        },
        InlineListItem {
            title: "Remove Instruction Exclude".to_string(),
            subtitle: Some("Remove one exclude entry by exact match.".to_string()),
            badge: Some("Prompt".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}remove_instruction_exclude",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory instruction excludes remove".to_string()),
        },
        InlineListItem {
            title: if memory_status.cleanup_status.needed {
                "Run Legacy Memory Cleanup".to_string()
            } else {
                "Run Memory Cleanup".to_string()
            },
            subtitle: Some(format!(
                "Rewrite durable memory through the LLM-assisted path and clear consumed rollout summaries (facts: {}, summary lines: {}).",
                memory_status.cleanup_status.suspicious_facts,
                memory_status.cleanup_status.suspicious_summary_lines,
            )),
            badge: Some("Action".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}cleanup",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory cleanup legacy normalize".to_string()),
        },
        InlineListItem {
            title: "Scaffold Missing Memory Files".to_string(),
            subtitle: Some(
                "Create `memory_summary.md`, `MEMORY.md`, topic files, and the rollout directory."
                    .to_string(),
            ),
            badge: Some("Action".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}scaffold",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory scaffold files".to_string()),
        },
        InlineListItem {
            title: "Rebuild Memory Summary Now".to_string(),
            subtitle: Some(
                "Recompute `memory_summary.md` and `MEMORY.md` from current memory state."
                    .to_string(),
            ),
            badge: Some("Action".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}rebuild",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory rebuild summary".to_string()),
        },
        InlineListItem {
            title: "Open Raw Settings Section".to_string(),
            subtitle: Some(
                "Jump to `/config agent.persistent_memory` for the raw settings palette."
                    .to_string(),
            ),
            badge: Some("Nav".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}open_settings_section",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory open config section".to_string()),
        },
        InlineListItem {
            title: "Open Memory Summary".to_string(),
            subtitle: Some(memory_status.summary_file.display().to_string()),
            badge: Some("Edit".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}open_summary",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory open summary file".to_string()),
        },
        InlineListItem {
            title: "Open Memory Directory".to_string(),
            subtitle: Some(memory_status.directory.display().to_string()),
            badge: Some("Edit".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}open_directory",
                MEMORY_ACTION_PREFIX
            ))),
            search_value: Some("memory open directory".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close memory controls.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                MEMORY_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close cancel".to_string()),
        },
    ];

    ctx.renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}toggle_enabled",
            MEMORY_ACTION_PREFIX
        ))),
        None,
    );
}

fn toggle_title(label: &str, enabled: bool) -> String {
    format!("{label}: {}", if enabled { "On" } else { "Off" })
}

async fn handle_memory_action(
    ctx: &mut SlashCommandContext<'_>,
    action_key: &str,
    memory_status: &PersistentMemoryStatus,
    _config_mode: bool,
) -> Result<Option<SlashCommandControl>> {
    match action_key {
        "toggle_enabled" => {
            let enabled = ctx
                .vt_cfg
                .as_ref()
                .map(|cfg| cfg.agent.persistent_memory.enabled)
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
                .line(MessageStyle::Info, "Toggled small-model memory routing.")?;
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
        "open_settings_section" => {
            return super::show_settings_at_path_from_context(ctx, Some("agent.persistent_memory"))
                .await
                .map(Some);
        }
        "open_summary" => {
            return super::apps::launch_editor_from_context(
                ctx,
                Some(memory_status.summary_file.display().to_string()),
            )
            .await
            .map(Some);
        }
        "open_directory" => {
            return super::apps::launch_editor_from_context(
                ctx,
                Some(memory_status.directory.display().to_string()),
            )
            .await
            .map(Some);
        }
        _ => bail!("Unknown memory action: {}", action_key),
    }

    Ok(None)
}

async fn persist_workspace_config_change<F>(
    ctx: &mut SlashCommandContext<'_>,
    update: F,
) -> Result<()>
where
    F: FnOnce(&mut toml::map::Map<String, TomlValue>) -> Result<()>,
{
    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let workspace_config_path = preferred_workspace_config_path(&manager, &ctx.config.workspace);
    let mut root = load_toml_value(&workspace_config_path)?;
    let root_table = root
        .as_table_mut()
        .context("Workspace config root is not a TOML table")?;
    update(root_table)?;
    save_toml_value(&workspace_config_path, &root)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await
}

async fn persist_user_directory_override(
    ctx: &mut SlashCommandContext<'_>,
    value: Option<String>,
) -> Result<()> {
    let manager = ConfigManager::load_from_workspace(&ctx.config.workspace)
        .context("Failed to load VT Code configuration")?;
    let user_config_path =
        preferred_user_config_path(&manager).context("Could not resolve user config path")?;
    write_user_directory_override(&user_config_path, value)?;
    refresh_runtime_config_from_manager(
        ctx.renderer,
        ctx.handle,
        ctx.config,
        ctx.vt_cfg,
        ctx.provider_client.as_ref(),
        ctx.session_bootstrap,
        ctx.full_auto,
    )
    .await
}

fn write_user_directory_override(path: &Path, value: Option<String>) -> Result<()> {
    let mut root = load_toml_value(path)?;

    let root_table = root
        .as_table_mut()
        .context("User config root is not a TOML table")?;
    match value {
        Some(value) if !value.trim().is_empty() => {
            let agent_table = ensure_child_table(root_table, "agent");
            let memory_table = ensure_child_table(agent_table, "persistent_memory");
            memory_table.insert("directory_override".to_string(), TomlValue::String(value));
        }
        _ => {
            let remove_memory_table = {
                let agent_table = ensure_child_table(root_table, "agent");
                let memory_table = ensure_child_table(agent_table, "persistent_memory");
                memory_table.remove("directory_override");
                memory_table.is_empty()
            };
            if remove_memory_table {
                let remove_agent_table = {
                    let agent_table = ensure_child_table(root_table, "agent");
                    agent_table.remove("persistent_memory");
                    agent_table.is_empty()
                };
                if remove_agent_table {
                    root_table.remove("agent");
                }
            }
        }
    }

    save_toml_value(path, &root)
}

fn ensure_child_table<'a>(
    table: &'a mut toml::map::Map<String, TomlValue>,
    key: &str,
) -> &'a mut toml::map::Map<String, TomlValue> {
    let entry = table
        .entry(key.to_string())
        .or_insert_with(|| TomlValue::Table(Default::default()));
    if !entry.is_table() {
        *entry = TomlValue::Table(Default::default());
    }
    entry
        .as_table_mut()
        .expect("table entry should be a table after initialization")
}

fn set_workspace_memory_enabled(root_table: &mut toml::map::Map<String, TomlValue>, value: bool) {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert("enabled".to_string(), TomlValue::Boolean(value));
}

fn set_workspace_memory_auto_write(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: bool,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert("auto_write".to_string(), TomlValue::Boolean(value));
}

fn set_workspace_memory_line_limit(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert(
        "startup_line_limit".to_string(),
        TomlValue::Integer(usize_to_toml_integer(value, "startup_line_limit")?),
    );
    Ok(())
}

fn set_workspace_memory_byte_limit(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    let memory_table = ensure_child_table(agent_table, "persistent_memory");
    memory_table.insert(
        "startup_byte_limit".to_string(),
        TomlValue::Integer(usize_to_toml_integer(value, "startup_byte_limit")?),
    );
    Ok(())
}

fn set_workspace_instruction_import_depth(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: usize,
) -> Result<()> {
    let agent_table = ensure_child_table(root_table, "agent");
    agent_table.insert(
        "instruction_import_max_depth".to_string(),
        TomlValue::Integer(usize_to_toml_integer(
            value,
            "instruction_import_max_depth",
        )?),
    );
    Ok(())
}

fn set_workspace_instruction_excludes(
    root_table: &mut toml::map::Map<String, TomlValue>,
    values: Vec<String>,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    agent_table.insert(
        "instruction_excludes".to_string(),
        TomlValue::Array(values.into_iter().map(TomlValue::String).collect()),
    );
}

fn set_workspace_small_model_for_memory(
    root_table: &mut toml::map::Map<String, TomlValue>,
    value: bool,
) {
    let agent_table = ensure_child_table(root_table, "agent");
    let small_model_table = ensure_child_table(agent_table, "small_model");
    small_model_table.insert("use_for_memory".to_string(), TomlValue::Boolean(value));
}

fn usize_to_toml_integer(value: usize, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{} is too large to persist", label))
}

fn load_toml_value(path: &Path) -> Result<TomlValue> {
    if !path.exists() {
        return Ok(TomlValue::Table(Default::default()));
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(TomlValue::Table(Default::default()));
    }

    toml::from_str::<TomlValue>(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

fn save_toml_value(path: &Path, root: &TomlValue) -> Result<()> {
    let is_empty = root.as_table().is_some_and(|table| table.is_empty());
    if is_empty {
        if path.exists() {
            std::fs::remove_file(path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    std::fs::write(path, toml::to_string_pretty(root)?)
        .with_context(|| format!("Failed to write {}", path.display()))
}

fn preferred_workspace_config_path(manager: &ConfigManager, workspace: &Path) -> PathBuf {
    manager
        .layer_stack()
        .layers()
        .iter()
        .rev()
        .find_map(|layer| match &layer.source {
            ConfigLayerSource::Workspace { file } if layer.is_enabled() => Some(file.clone()),
            _ => None,
        })
        .unwrap_or_else(|| workspace.join(manager.config_file_name()))
}

fn preferred_user_config_path(manager: &ConfigManager) -> Option<PathBuf> {
    manager
        .layer_stack()
        .layers()
        .iter()
        .rev()
        .find_map(|layer| match &layer.source {
            ConfigLayerSource::User { file } if layer.is_enabled() => Some(file.clone()),
            _ => None,
        })
        .or_else(|| {
            let defaults = current_config_defaults();
            defaults
                .home_config_paths(manager.config_file_name())
                .into_iter()
                .next()
        })
        .or_else(|| dirs::home_dir().map(|home| home.join(manager.config_file_name())))
}

fn parse_positive_usize(value: &str, label: &str) -> Result<usize> {
    let parsed = value
        .trim()
        .parse::<usize>()
        .with_context(|| format!("Failed to parse {}", label))?;
    if parsed == 0 {
        bail!("{} must be greater than 0", label);
    }
    Ok(parsed)
}

async fn prompt_required_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
) -> Result<Option<String>> {
    let Some(value) = prompt_text(ctx, title, question, freeform_label, placeholder, false).await?
    else {
        return Ok(None);
    };
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "Input was empty. Nothing changed.")?;
        return Ok(None);
    }
    Ok(Some(trimmed))
}

async fn prompt_optional_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
) -> Result<Option<String>> {
    prompt_text(ctx, title, question, freeform_label, placeholder, true).await
}

async fn prompt_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
    allow_empty: bool,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Input".to_string(),
        question: question.to_string(),
        items: vec![InlineListItem {
            title: "Submit".to_string(),
            subtitle: Some("Press Tab to type text, then Enter to submit.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: MEMORY_PROMPT_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit memory input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
    };

    let outcome = show_wizard_modal_and_wait(
        ctx.handle,
        ctx.session,
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
    )
    .await?;
    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == MEMORY_PROMPT_QUESTION_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    };

    let Some(value) = value else {
        return Ok(None);
    };
    if allow_empty {
        return Ok(Some(value));
    }

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(Some(trimmed))
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

fn format_path_list(paths: &[String]) -> String {
    if paths.is_empty() {
        "none".to_string()
    } else {
        paths.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_user_directory_override_removes_empty_file() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("config.toml");

        write_user_directory_override(&path, Some("/tmp/memory".to_string())).expect("write");
        assert!(path.exists());

        write_user_directory_override(&path, None).expect("clear");
        assert!(!path.exists());
    }

    #[test]
    fn workspace_memory_settings_preserve_unrelated_keys() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("vtcode.toml");
        std::fs::write(
            &path,
            "[agent]\ntheme = \"ciapre\"\n[agent.small_model]\nmodel = \"gpt-5-mini\"\n",
        )
        .expect("seed config");

        let mut root = load_toml_value(&path).expect("load config");
        let root_table = root.as_table_mut().expect("root table");
        set_workspace_memory_enabled(root_table, false);
        set_workspace_memory_auto_write(root_table, false);
        set_workspace_memory_line_limit(root_table, 111).expect("line limit");
        set_workspace_memory_byte_limit(root_table, 222).expect("byte limit");
        set_workspace_instruction_import_depth(root_table, 7).expect("import depth");
        set_workspace_instruction_excludes(
            root_table,
            vec!["**/other-team/.vtcode/rules/**".to_string()],
        );
        set_workspace_small_model_for_memory(root_table, false);
        save_toml_value(&path, &root).expect("save config");

        let saved = load_toml_value(&path).expect("reload config");
        let agent = saved
            .get("agent")
            .and_then(TomlValue::as_table)
            .expect("agent table");
        assert_eq!(
            agent.get("theme").and_then(TomlValue::as_str),
            Some("ciapre")
        );
        assert!(agent.get("provider").is_none());
        assert_eq!(
            agent
                .get("instruction_import_max_depth")
                .and_then(TomlValue::as_integer),
            Some(7)
        );
        assert_eq!(
            agent
                .get("instruction_excludes")
                .and_then(TomlValue::as_array)
                .map(|entries| entries.len()),
            Some(1)
        );

        let memory = agent
            .get("persistent_memory")
            .and_then(TomlValue::as_table)
            .expect("persistent memory table");
        assert_eq!(
            memory.get("enabled").and_then(TomlValue::as_bool),
            Some(false)
        );
        assert_eq!(
            memory.get("auto_write").and_then(TomlValue::as_bool),
            Some(false)
        );
        assert_eq!(
            memory
                .get("startup_line_limit")
                .and_then(TomlValue::as_integer),
            Some(111)
        );
        assert_eq!(
            memory
                .get("startup_byte_limit")
                .and_then(TomlValue::as_integer),
            Some(222)
        );

        let small_model = agent
            .get("small_model")
            .and_then(TomlValue::as_table)
            .expect("small model table");
        assert_eq!(
            small_model.get("model").and_then(TomlValue::as_str),
            Some("gpt-5-mini")
        );
        assert_eq!(
            small_model
                .get("use_for_memory")
                .and_then(TomlValue::as_bool),
            Some(false)
        );
    }
}
