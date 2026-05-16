use vtcode_core::llm::{
    LightweightFeature, LightweightRouteSource, auto_lightweight_model,
    lightweight_model_choices, resolve_lightweight_route,
};
use vtcode_core::persistent_memory::PersistentMemoryStatus;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{InlineListItem, InlineListSelection};

use super::{
    MEMORY_ACTION_BACK, MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX,
    SlashCommandContext,
};

pub(super) struct MemoryLightweightRouteInfo {
    pub(super) configured_label: String,
    pub(super) effective_label: String,
    pub(super) warning: Option<String>,
    pub(super) choices: Vec<String>,
    pub(super) main_model: String,
}

pub(super) fn render_common_memory_status(
    ctx: &mut SlashCommandContext<'_>,
    memory_status: &PersistentMemoryStatus,
) -> anyhow::Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Persistent memory: {} (auto-write: {})",
            if memory_status.enabled {
                "enabled"
            } else {
                "disabled"
            },
            if memory_status.auto_write { "on" } else { "off" }
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

pub(super) fn memory_lightweight_route_info(
    runtime_config: &vtcode_core::config::types::AgentConfig,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> MemoryLightweightRouteInfo {
    let resolution =
        resolve_lightweight_route(runtime_config, vt_cfg, LightweightFeature::Memory, None);
    let configured_label = vt_cfg
        .map(|cfg| {
            if !cfg.agent.small_model.enabled || !cfg.agent.small_model.use_for_memory {
                "Use main model".to_string()
            } else {
                let configured = cfg.agent.small_model.model.trim();
                if configured.is_empty() {
                    "Automatic".to_string()
                } else if configured.eq_ignore_ascii_case(runtime_config.model.as_str()) {
                    "Use main model".to_string()
                } else {
                    configured.to_string()
                }
            }
        })
        .unwrap_or_else(|| "Use main model".to_string());
    let effective_label = match resolution.source {
        LightweightRouteSource::MainModel => runtime_config.model.clone(),
        _ => match resolution.fallback_to_main_model() {
            Some(fallback) => format!(
                "{} -> fallback {}",
                resolution.primary.model, fallback.model
            ),
            None => resolution.primary.model.clone(),
        },
    };

    let mut choices = lightweight_model_choices(&runtime_config.provider, &runtime_config.model);
    choices.retain(|model| !model.eq_ignore_ascii_case(runtime_config.model.as_str()));

    MemoryLightweightRouteInfo {
        configured_label,
        effective_label,
        warning: resolution.warning,
        choices,
        main_model: runtime_config.model.clone(),
    }
}

pub(super) fn show_memory_actions_modal(
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
    let lightweight_route = memory_lightweight_route_info(ctx.config, ctx.vt_cfg.as_ref());
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
        "Memory {} • auto-write {} • triage {} • pending rollouts {} • cleanup {}",
        if memory_status.enabled { "on" } else { "off" },
        if memory_status.auto_write { "on" } else { "off" },
        lightweight_route.configured_label,
        memory_status.pending_rollout_summaries,
        if memory_status.cleanup_status.needed {
            "needed"
        } else {
            "clean"
        },
    ));
    lines.push(format!(
        "Effective memory route: {}",
        lightweight_route.effective_label
    ));
    if let Some(warning) = &lightweight_route.warning {
        lines.push(format!("Route warning: {}", warning));
    }

    let mut items = vec![];
    items.push(InlineListItem {
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
    });
    items.push(InlineListItem {
        title: toggle_title("Auto-write", memory_status.auto_write),
        subtitle: Some(
            "Write one rollout summary at session finalization, then consolidate it.".to_string(),
        ),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}toggle_auto_write",
            MEMORY_ACTION_PREFIX
        ))),
        search_value: Some("memory auto write toggle".to_string()),
    });
    items.push(InlineListItem {
        title: toggle_title(
            "Lightweight Model For Memory",
            agent_config.small_model.use_for_memory,
        ),
        subtitle: Some(
            "Allow VT Code to use the shared lightweight route for memory classification and summary refresh."
                .to_string(),
        ),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}toggle_small_model",
            MEMORY_ACTION_PREFIX
        ))),
        search_value: Some("memory lightweight model toggle".to_string()),
    });
    items.push(InlineListItem {
        title: format!("Memory Triage Model ({})", lightweight_route.configured_label),
        subtitle: Some(format!(
            "Effective route: {}",
            lightweight_route.effective_label
        )),
        badge: Some("Pick".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}auto",
            MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX
        ))),
        search_value: Some("memory triage lightweight model pick".to_string()),
    });
    items.push(InlineListItem {
        title: "Automatic".to_string(),
        subtitle: Some(format!(
            "Use {} and fall back to {}.",
            auto_lightweight_model(&ctx.config.provider, &ctx.config.model),
            ctx.config.model
        )),
        badge: Some(if lightweight_route.configured_label == "Automatic" {
            "Current".to_string()
        } else {
            "Recommended".to_string()
        }),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}auto",
            MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX
        ))),
        search_value: Some("memory lightweight model automatic".to_string()),
    });
    items.push(InlineListItem {
        title: "Use main model".to_string(),
        subtitle: Some(format!(
            "Keep memory extraction on {}.",
            lightweight_route.main_model
        )),
        badge: Some(if lightweight_route.configured_label == "Use main model" {
            "Current".to_string()
        } else {
            "Accuracy".to_string()
        }),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}main",
            MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX
        ))),
        search_value: Some("memory lightweight model main".to_string()),
    });
    items.extend(lightweight_route.choices.iter().map(|model| InlineListItem {
        title: model.clone(),
        subtitle: Some("Explicit same-provider lightweight model.".to_string()),
        badge: Some(
            if lightweight_route
                .configured_label
                .eq_ignore_ascii_case(model.as_str())
            {
                "Current".to_string()
            } else {
                "Model".to_string()
            },
        ),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}{model}",
            MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX
        ))),
        search_value: Some(format!("memory lightweight triage {}", model)),
    }));
    items.extend([
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
    ]);

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

pub(super) fn format_path_list(paths: &[String]) -> String {
    if paths.is_empty() {
        "none".to_string()
    } else {
        paths.join(", ")
    }
}