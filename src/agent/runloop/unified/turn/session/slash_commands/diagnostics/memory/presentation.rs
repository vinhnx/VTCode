use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use vtcode_core::llm::{
    LightweightFeature, LightweightRouteSource, auto_lightweight_model, lightweight_model_choices,
    resolve_lightweight_route,
};
use vtcode_core::persistent_memory::PersistentMemoryStatus;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use super::{MEMORY_ACTION_BACK, MEMORY_ACTION_PREFIX, MEMORY_LIGHTWEIGHT_MODEL_PREFIX, SlashCommandContext};

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
            if memory_status.enabled { "enabled" } else { "disabled" },
            if memory_status.auto_write { "on" } else { "off" }
        ),
    )?;
    ctx.renderer
        .line(MessageStyle::Info, &format!("Memory directory: {}", memory_status.directory.display()))?;
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
            "Memory files: `{}`, `{}`, or `{}`",
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
    let resolution = resolve_lightweight_route(runtime_config, vt_cfg, LightweightFeature::Memory, None);
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
            Some(fallback) => {
                format!("{} -> fallback {}", resolution.primary.model, fallback.model)
            }
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
    let agent_config = ctx.vt_cfg.as_ref().map(|cfg| cfg.agent.clone()).unwrap_or_default();
    let lightweight_route = memory_lightweight_route_info(ctx.config, ctx.vt_cfg.as_ref());
    let title = if config_mode {
        "Memory Settings"
    } else {
        "Instruction Memory"
    };

    let mut lines = if config_mode {
        vec!["Focused settings for persistent memory and instruction imports.".to_string()]
    } else {
        vec![format!(
            "{} source(s) • {} matched rule(s).",
            agents.len(),
            matched_rules.len()
        )]
    };
    lines.push(format!(
        "Memory {} • auto-write {} • triage {} • pending {} • cleanup {}",
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

    let mut items = vec![];
    items.push(InlineListItem {
        title: toggle_title("Persistent memory", memory_status.enabled),
        subtitle: Some("Toggle per-repo memory summary injection and learned memory files.".to_string()),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{MEMORY_ACTION_PREFIX}toggle_enabled"))),
        search_value: Some("memory enabled disable toggle".to_string()),
        ..Default::default()
    });
    items.push(InlineListItem {
        title: toggle_title("Auto-write", memory_status.auto_write),
        subtitle: Some("Write one rollout summary at session finalization, then consolidate it.".to_string()),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{MEMORY_ACTION_PREFIX}toggle_auto_write"))),
        search_value: Some("memory auto write toggle".to_string()),
        ..Default::default()
    });
    items.push(InlineListItem {
        title: toggle_title("Lightweight Model For Memory", agent_config.small_model.use_for_memory),
        subtitle: Some(
            "Allow VT Code to use the shared lightweight route for memory classification and summary refresh."
                .to_string(),
        ),
        badge: Some("Toggle".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{MEMORY_ACTION_PREFIX}toggle_small_model"))),
        search_value: Some("memory lightweight model toggle".to_string()),
        ..Default::default()
    });
    items.push(InlineListItem {
        title: format!("Memory Triage Model ({})", lightweight_route.configured_label),
        subtitle: Some({
            let mut subtitle = format!("Effective route: {}", lightweight_route.effective_label);
            if let Some(warning) = lightweight_route.warning.as_deref() {
                let warning = warning.trim();
                if !warning.is_empty() {
                    subtitle.push_str(" • ");
                    subtitle.push_str(warning);
                }
            }
            subtitle
        }),
        badge: Some("Pick".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{MEMORY_ACTION_PREFIX}{MEMORY_LIGHTWEIGHT_MODEL_PREFIX}auto"
        ))),
        search_value: Some("memory triage lightweight model pick".to_string()),
        ..Default::default()
    });
    // Only the configured route is `Current`; others keep their advice badges.
    let mut automatic = ui_list::choice(
        "Automatic",
        Some(format!(
            "Use {} and fall back to {}.",
            auto_lightweight_model(&ctx.config.provider, &ctx.config.model),
            ctx.config.model
        )),
        Some(InlineListSelection::ConfigAction(format!(
            "{MEMORY_ACTION_PREFIX}{MEMORY_LIGHTWEIGHT_MODEL_PREFIX}auto"
        ))),
    )
    .with_search_value("memory lightweight model automatic".to_string());
    let auto_is_current = lightweight_route.configured_label == "Automatic";
    automatic = if auto_is_current {
        automatic.with_badge("Current", vtcode_commons::ui_protocol::InlineTone::Current)
    } else {
        automatic.with_badge("Recommended", vtcode_commons::ui_protocol::InlineTone::Accent)
    };
    items.push(automatic);

    let mut main = ui_list::choice(
        "Use main model",
        Some(format!("Keep memory extraction on {}.", lightweight_route.main_model)),
        Some(InlineListSelection::ConfigAction(format!(
            "{MEMORY_ACTION_PREFIX}{MEMORY_LIGHTWEIGHT_MODEL_PREFIX}main"
        ))),
    )
    .with_search_value("memory lightweight model main".to_string());
    main = if lightweight_route.configured_label == "Use main model" {
        main.with_badge("Current", vtcode_commons::ui_protocol::InlineTone::Current)
    } else {
        main.with_badge("Accuracy", vtcode_commons::ui_protocol::InlineTone::Accent)
    };
    items.push(main);

    items.extend(lightweight_route.choices.iter().map(|model| {
        let mut row = ui_list::choice(
            model.clone(),
            Some("Explicit same-provider lightweight model.".to_string()),
            Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}{MEMORY_LIGHTWEIGHT_MODEL_PREFIX}{model}"
            ))),
        )
        .with_search_value(format!("memory lightweight triage {model}"));
        if lightweight_route.configured_label.eq_ignore_ascii_case(model.as_str()) {
            row = row.with_badge("Current", vtcode_commons::ui_protocol::InlineTone::Current);
        }
        row
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
                "{MEMORY_ACTION_PREFIX}set_lines"
            ))),
            search_value: Some("memory startup line limit".to_string()),
    ..Default::default()
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
                "{MEMORY_ACTION_PREFIX}set_bytes"
            ))),
            search_value: Some("memory startup byte limit".to_string()),
    ..Default::default()
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
                "{MEMORY_ACTION_PREFIX}set_import_depth"
            ))),
            search_value: Some("memory instruction import depth".to_string()),
    ..Default::default()
},
        ui_list::action("Set Directory Override",
                match agent_config.persistent_memory.directory_override.as_deref() {
                    Some(value) if !value.trim().is_empty() => format!("Current: {value}"),
                    _ => {
                        "Write a user-level override for the memory storage directory.".to_string()
                    }
                },
                Some("Prompt".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}set_directory_override"
            )))).with_search_value("memory directory override set".to_string()),
        ui_list::action("Clear Directory Override", "Remove the user-level memory directory override.".to_string(), Some("Action".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}clear_directory_override"
            )))).with_search_value("memory directory override clear".to_string()),
        ui_list::action("Add Instruction Exclude", format!(
                "Current excludes: {}",
                agent_config.instruction_excludes.len()
            ), Some("Prompt".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}add_instruction_exclude"
            )))).with_search_value("memory instruction excludes add".to_string()),
        ui_list::action("Remove Instruction Exclude", "Remove one exclude entry by exact match.".to_string(), Some("Prompt".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}remove_instruction_exclude"
            )))).with_search_value("memory instruction excludes remove".to_string()),
        ui_list::action(if memory_status.cleanup_status.needed {
                "Run Legacy Memory Cleanup".to_string()
            } else {
                "Run Memory Cleanup".to_string()
            }, format!(
                "Rewrite durable memory through the LLM-assisted path and clear consumed rollout summaries (facts: {}, summary lines: {}).",
                memory_status.cleanup_status.suspicious_facts,
                memory_status.cleanup_status.suspicious_summary_lines,
            ), Some("Action".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}cleanup"
            )))).with_search_value("memory cleanup legacy normalize".to_string()),
        ui_list::action("Scaffold Missing Memory Files", 
                "Create `memory_summary.md`, `MEMORY.md`, topic files, and the rollout directory."
                    .to_string(),
                Some("Action".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}scaffold"
            )))).with_search_value("memory scaffold files".to_string()),
        ui_list::action("Rebuild Memory Summary Now", 
                "Recompute `memory_summary.md` and `MEMORY.md` from current memory state."
                    .to_string(),
                Some("Action".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}rebuild"
            )))).with_search_value("memory rebuild summary".to_string()),
        ui_list::action("Batch Extract Memory From Past Sessions", 
                "Read grounded facts from recent sessions and consolidate them into memory."
                    .to_string(),
                Some("Action".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}rebuild_batch"
            )))).with_search_value("memory batch extract sessions".to_string()),
        ui_list::action("Open Raw Settings Section", 
                "Jump to `/config agent.persistent_memory` for the raw settings palette."
                    .to_string(),
                Some("Nav".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}open_settings_section"
            )))).with_search_value("memory open config section".to_string()),
        ui_list::action("Open Memory Summary", memory_status.summary_file.display().to_string(), Some("Edit".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}open_summary"
            )))).with_search_value("memory open summary file".to_string()),
        ui_list::action("Open Memory Directory", memory_status.directory.display().to_string(), Some("Edit".to_string()), Tone::Accent, Some(InlineListSelection::ConfigAction(format!(
                "{MEMORY_ACTION_PREFIX}open_directory"
            )))).with_search_value("memory open directory".to_string()),
        ui_list::action("Back", "Close memory controls.".to_string(), None, Tone::Neutral, Some(InlineListSelection::ConfigAction(
                MEMORY_ACTION_BACK.to_string(),
            ))).with_search_value("back close cancel".to_string()),
    ]);

    ctx.renderer.show_list_modal(
        title,
        lines,
        items,
        Some(InlineListSelection::ConfigAction(format!("{MEMORY_ACTION_PREFIX}toggle_enabled"))),
        None,
    );
}

fn toggle_title(label: &str, enabled: bool) -> String {
    format!("{label}: {}", if enabled { "On" } else { "Off" })
}
