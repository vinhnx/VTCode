use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use vtcode_core::constants::tools;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{
    AgentPaletteItem, InlineListItem, InlineListSearchConfig, InlineListSelection,
};

use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::{
    AgentDefinitionScope, AgentManagerAction, SubprocessManagerAction,
};

#[path = "agents_authoring.rs"]
mod authoring;
#[path = "agents/runtime.rs"]
mod runtime;

#[cfg(test)]
use runtime::{
    active_subagent_entries, background_subprocess_summary, subprocess_action_prompt,
    summarize_thread_event_preview, visible_subagent_entries,
};
use runtime::{
    apply_background_subprocess_action, close_subagent_entry, handle_list_subprocesses_text,
    handle_list_threads_text, render_active_agent_status_text, render_background_setup_guidance,
    render_background_subprocess_status_text, render_subprocess_status,
    show_active_agent_inspector, show_background_subprocess_inspector, show_threads_modal,
};

const AGENT_ACTION_PREFIX: &str = "agents:";
const AGENT_INSPECT_PREFIX: &str = "agents:inspect:";
const THREAD_INSPECT_PREFIX: &str = "agents:thread:";
const THREAD_TRANSCRIPT_PREFIX: &str = "agents:transcript:";
const THREAD_CANCEL_PREFIX: &str = "agents:cancel:";
const SUBPROCESS_TRANSCRIPT_PREFIX: &str = "subprocesses:transcript:";
const SUBPROCESS_ARCHIVE_PREFIX: &str = "subprocesses:archive:";
const SUBPROCESS_STOP_PREFIX: &str = "subprocesses:stop:";
const SUBPROCESS_CANCEL_PREFIX: &str = "subprocesses:cancel:";
const ACTIVE_AGENT_INSPECTOR_REFRESH_MS: u64 = 750;
const DEFAULT_AGENT_DESCRIPTION_TEXT: &str = "Describe when VT Code should delegate to this agent.";
const DEFAULT_AGENT_BODY_TEXT: &str = "\nYou are a focused VT Code subagent.\n\nScope:\n- Describe the tasks this agent should handle.\n- Keep behavior narrow and task-specific.\n\nConstraints:\n- Use VT Code tool ids in frontmatter such as `read_file`, `list_files`, `unified_search`, and `unified_exec`.\n- Prefer the narrowest tool set that fits the job.\n- Return concise, actionable results.\n\nOutput:\n- State what you checked.\n- Summarize findings or changes.\n- Call out verification or remaining risks when relevant.\n";
const DEFAULT_AGENT_TOOL_IDS: [&str; 3] =
    [tools::READ_FILE, tools::LIST_FILES, tools::UNIFIED_SEARCH];
const SUBAGENT_CONTROLLER_INACTIVE_MESSAGE: &str =
    "Subagent controller is not active in this session.";

pub(crate) async fn handle_manage_agents(
    mut ctx: SlashCommandContext<'_>,
    action: AgentManagerAction,
) -> Result<SlashCommandControl> {
    match action {
        AgentManagerAction::List => {
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                if !ensure_selection_ui_available(&mut ctx, "opening subagent manager")? {
                    return Ok(SlashCommandControl::Continue);
                }
                show_agents_manager(ctx).await
            } else {
                let mut ctx = ctx;
                handle_list_agents_text(&mut ctx).await
            }
        }
        AgentManagerAction::Threads => {
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                if !ensure_selection_ui_available(&mut ctx, "browsing subagent threads")? {
                    return Ok(SlashCommandControl::Continue);
                }
                show_threads_modal(ctx).await
            } else {
                let mut ctx = ctx;
                handle_list_threads_text(&mut ctx).await
            }
        }
        AgentManagerAction::Create { scope, name } => {
            authoring::handle_create_agent(ctx, scope, name.as_deref()).await
        }
        AgentManagerAction::Inspect { id } => {
            let Some(controller) = ctx.tool_registry.subagent_controller() else {
                return render_missing_subagent_controller(&mut ctx);
            };
            let entry = controller.status_for(&id).await?;
            if ctx.renderer.supports_inline_ui() {
                let mut ctx = ctx;
                show_active_agent_inspector(&mut ctx, entry).await
            } else {
                let snapshot = controller.snapshot_for_thread(&id).await?;
                render_active_agent_status_text(&mut ctx, &entry, &snapshot)?;
                Ok(SlashCommandControl::Continue)
            }
        }
        AgentManagerAction::Close { id } => {
            let Some(controller) = ctx.tool_registry.subagent_controller() else {
                return render_missing_subagent_controller(&mut ctx);
            };
            let entry = controller.status_for(&id).await?;
            close_subagent_entry(&mut ctx, &controller, &entry.id, &entry.display_label).await
        }
        AgentManagerAction::Edit { name } => {
            authoring::handle_edit_agent(ctx, name.as_deref()).await
        }
        AgentManagerAction::Delete { name } => {
            let mut ctx = ctx;
            handle_delete_agent(&mut ctx, &name).await
        }
    }
}

pub(crate) async fn handle_manage_subprocesses(
    mut ctx: SlashCommandContext<'_>,
    action: SubprocessManagerAction,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return render_missing_subagent_controller(&mut ctx);
    };

    match action {
        SubprocessManagerAction::ToggleDefault => {
            if !controller.background_subagents_enabled()
                || controller.configured_default_background_agent().is_none()
            {
                if ctx.renderer.supports_inline_ui() {
                    ctx.handle.show_local_agents();
                }
                render_background_setup_guidance(&mut ctx)?;
                return Ok(SlashCommandControl::Continue);
            }
            let entry = controller.toggle_default_background_subagent().await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::Refresh => {
            let entries = controller.refresh_background_processes().await?;
            if entries.is_empty() {
                ctx.renderer
                    .line(MessageStyle::Info, "No managed background subprocesses.")?;
            } else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Refreshed {} background subprocesses.", entries.len()),
                )?;
            }
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::List => {
            if ctx.renderer.supports_inline_ui() {
                ctx.handle.show_local_agents();
                return Ok(SlashCommandControl::Continue);
            }
            handle_list_subprocesses_text(&mut ctx).await
        }
        SubprocessManagerAction::Inspect { id } => {
            let entry = controller.background_snapshot(&id).await?;
            if ctx.renderer.supports_inline_ui() {
                show_background_subprocess_inspector(&mut ctx, entry.entry).await
            } else {
                render_background_subprocess_status_text(&mut ctx, &entry)?;
                Ok(SlashCommandControl::Continue)
            }
        }
        SubprocessManagerAction::Stop { id } => {
            let entry =
                apply_background_subprocess_action(&mut ctx, &controller, &id, false).await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
        SubprocessManagerAction::Cancel { id } => {
            let entry =
                apply_background_subprocess_action(&mut ctx, &controller, &id, true).await?;
            render_subprocess_status(&mut ctx, &entry)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

fn render_missing_subagent_controller(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, SUBAGENT_CONTROLLER_INACTIVE_MESSAGE)?;
    Ok(SlashCommandControl::Continue)
}

async fn show_agents_manager(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.handle.show_list_modal(
        "Agents".to_string(),
        vec![
            "Manage agent definitions, subagent runs, and custom definitions.".to_string(),
            "Use Enter to inspect, create, edit, or delete definitions.".to_string(),
        ],
        vec![
            action_item(
                "Browse agents",
                "List primary and subagent definitions with source badges",
                Some("Recommended"),
                "browse effective shadowed agents",
                "browse",
            ),
            action_item(
                "Browse active agents",
                "Inspect delegated runs without switching the main session",
                None,
                "active agents delegated inspector",
                "threads",
            ),
            action_item(
                "Create project agent",
                "Guided flow for `.vtcode/agents/<name>.md` with VT Code-native frontmatter",
                Some("Project"),
                "create project agent guided authoring",
                "create-project",
            ),
            action_item(
                "Create user agent",
                "Guided flow for `~/.vtcode/agents/<name>.md` with VT Code-native frontmatter",
                Some("User"),
                "create user agent guided authoring",
                "create-user",
            ),
            action_item(
                "Edit custom agent",
                "Guided editor for native `.vtcode` agents; imported files still open in your editor",
                None,
                "edit custom agent guided authoring",
                "edit",
            ),
            action_item(
                "Delete custom agent",
                "Pick a project or user agent file and remove it",
                None,
                "delete custom agent file",
                "delete",
            ),
        ],
        Some(InlineListSelection::ConfigAction(format!(
            "{AGENT_ACTION_PREFIX}browse"
        ))),
        Some(InlineListSearchConfig {
            label: "Search subagent actions".to_string(),
            placeholder: Some("browse, create, edit, thread".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };

    match action.as_str() {
        value if value == format!("{AGENT_ACTION_PREFIX}browse") => show_agent_catalog(ctx).await,
        value if value == format!("{AGENT_ACTION_PREFIX}threads") => show_threads_modal(ctx).await,
        value if value == format!("{AGENT_ACTION_PREFIX}create-project") => {
            authoring::handle_create_agent(ctx, Some(AgentDefinitionScope::Project), None).await
        }
        value if value == format!("{AGENT_ACTION_PREFIX}create-user") => {
            authoring::handle_create_agent(ctx, Some(AgentDefinitionScope::User), None).await
        }
        value if value == format!("{AGENT_ACTION_PREFIX}edit") => {
            authoring::handle_edit_agent(ctx, None).await
        }
        value if value == format!("{AGENT_ACTION_PREFIX}delete") => {
            let Some(name) = select_custom_agent_name(&mut ctx, "Delete custom agent").await?
            else {
                return Ok(SlashCommandControl::Continue);
            };
            if confirm_delete_agent(&mut ctx, &name).await? {
                handle_delete_agent(&mut ctx, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        _ => Ok(SlashCommandControl::Continue),
    }
}

async fn show_agent_catalog(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        return render_missing_subagent_controller(&mut ctx);
    };

    let specs = controller.effective_specs().await;
    let shadowed = controller.shadowed_specs().await;
    if specs.is_empty() && shadowed.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No subagent definitions are currently loaded.",
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let mut items = Vec::new();
    for spec in &specs {
        items.push(InlineListItem {
            title: spec.name.clone(),
            subtitle: Some(agent_subtitle(spec, false)),
            badge: Some(agent_badge(spec)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{AGENT_INSPECT_PREFIX}{}",
                spec.name
            ))),
            search_value: Some(format!(
                "{} {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        });
    }
    for spec in &shadowed {
        items.push(InlineListItem {
            title: format!("{} (shadowed)", spec.name),
            subtitle: Some(agent_subtitle(spec, true)),
            badge: Some("Shadowed".to_string()),
            indent: 0,
            selection: None,
            search_value: Some(format!(
                "{} shadowed {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        });
    }

    let selected = items.iter().find_map(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        "Loaded agents".to_string(),
        vec![
            format!(
                "{} effective definition(s), {} shadowed definition(s).",
                specs.len(),
                shadowed.len()
            ),
            "Select an effective definition to inspect details.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search agents".to_string(),
            placeholder: Some("name, source, description".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };
    let Some(name) = action.strip_prefix(AGENT_INSPECT_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    let spec = specs
        .into_iter()
        .find(|spec| spec.name == name)
        .ok_or_else(|| anyhow!("Unknown agent {}", name))?;
    render_agent_details(
        &mut ctx,
        &spec,
        shadowed.iter().filter(|entry| entry.name == name).count(),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn handle_list_agents_text(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let specs = controller.effective_specs().await;
    let shadowed = controller.shadowed_specs().await;
    let threads = controller.status_entries().await;

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Loaded {} effective agent definitions ({} shadowed definitions).",
            specs.len(),
            shadowed.len()
        ),
    )?;
    for spec in specs {
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("{} {}", spec.name, agent_subtitle(&spec, false)),
        )?;
    }

    if threads.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No subagent threads yet.")?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} subagent thread(s):", threads.len()),
        )?;
        for entry in threads {
            ctx.renderer.line(
                MessageStyle::Output,
                &format!(
                    "{} {} {}",
                    entry.id,
                    entry.agent_name,
                    status_label(entry.status)
                ),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}

async fn legacy_create_agent_scaffold(
    ctx: &mut SlashCommandContext<'_>,
    scope: AgentDefinitionScope,
    name: &str,
) -> Result<SlashCommandControl> {
    validate_agent_name(name)?;
    let path = match scope {
        AgentDefinitionScope::Project => ctx
            .config
            .workspace
            .join(".vtcode/agents")
            .join(format!("{name}.md")),
        AgentDefinitionScope::User => dirs::home_dir()
            .ok_or_else(|| anyhow!("Cannot resolve home directory for user-scope agent"))?
            .join(".vtcode/agents")
            .join(format!("{name}.md")),
    };

    if path.exists() {
        bail!("Agent file already exists at {}", path.display());
    }

    tokio::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow!("Invalid agent destination {}", path.display()))?,
    )
    .await?;
    tokio::fs::write(&path, scaffold_agent_markdown(name)).await?;

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "Created agent scaffold at {} with VT Code-native subagent frontmatter.",
            path.display()
        ),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn legacy_open_agent_editor(
    ctx: SlashCommandContext<'_>,
    name: &str,
) -> Result<SlashCommandControl> {
    let path = resolve_custom_agent_path(&ctx, name).await?;
    super::apps::handle_launch_editor(ctx, Some(path.display().to_string())).await
}

async fn handle_delete_agent(
    ctx: &mut SlashCommandContext<'_>,
    name: &str,
) -> Result<SlashCommandControl> {
    let path = resolve_custom_agent_path(ctx, name).await?;
    tokio::fs::remove_file(&path).await?;
    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Deleted agent definition {}.", path.display()),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn select_custom_agent_name(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
) -> Result<Option<String>> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    let specs = controller
        .effective_specs()
        .await
        .into_iter()
        .filter(|spec| spec.file_path.is_some())
        .collect::<Vec<_>>();
    if specs.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No editable custom agents are currently loaded.",
        )?;
        return Ok(None);
    }

    let items = specs
        .iter()
        .map(|spec| InlineListItem {
            title: spec.name.clone(),
            subtitle: Some(agent_subtitle(spec, false)),
            badge: Some(agent_badge(spec)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{AGENT_INSPECT_PREFIX}{}",
                spec.name
            ))),
            search_value: Some(format!(
                "{} {} {}",
                spec.name,
                spec.description,
                spec.source.label()
            )),
        })
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        title.to_string(),
        vec!["Select a project or user-scope agent definition.".to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search custom agents".to_string(),
            placeholder: Some("name, description, source".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    Ok(action
        .strip_prefix(AGENT_INSPECT_PREFIX)
        .map(ToString::to_string))
}

async fn confirm_delete_agent(ctx: &mut SlashCommandContext<'_>, name: &str) -> Result<bool> {
    ctx.handle.show_list_modal(
        "Delete custom agent".to_string(),
        vec![format!(
            "Delete `{name}` from disk? This cannot be undone automatically."
        )],
        vec![
            InlineListItem {
                title: "Delete agent".to_string(),
                subtitle: Some("Remove the selected definition file".to_string()),
                badge: Some("Confirm".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:confirm-delete".to_string(),
                )),
                search_value: Some("confirm delete".to_string()),
            },
            InlineListItem {
                title: "Cancel".to_string(),
                subtitle: Some("Keep the agent definition".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "agents:cancel-delete".to_string(),
                )),
                search_value: Some("cancel".to_string()),
            },
        ],
        Some(InlineListSelection::ConfigAction(
            "agents:cancel-delete".to_string(),
        )),
        None,
    );
    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(false);
    };
    Ok(matches!(
        selection,
        InlineListSelection::ConfigAction(action) if action == "agents:confirm-delete"
    ))
}

async fn resolve_custom_agent_path(ctx: &SlashCommandContext<'_>, name: &str) -> Result<PathBuf> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    let spec = controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.matches_name(name))
        .ok_or_else(|| anyhow!("Unknown agent {}", name))?;
    let path = spec.file_path.ok_or_else(|| {
        anyhow!(
            "Agent {} is built-in or plugin-provided and cannot be edited here",
            name
        )
    })?;
    Ok(path)
}

fn render_agent_details(
    ctx: &mut SlashCommandContext<'_>,
    spec: &vtcode_config::SubagentSpec,
    shadowed_count: usize,
) -> Result<()> {
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("{} [{}]", spec.name, spec.source.label()),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("Description: {}", spec.description),
    )?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!(
            "Mode: {}",
            if spec.is_read_only() {
                "read-only"
            } else {
                "write-capable"
            }
        ),
    )?;
    if let Some(path) = spec.file_path.as_ref() {
        ctx.renderer
            .line(MessageStyle::Output, &format!("File: {}", path.display()))?;
    }
    if shadowed_count > 0 {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{shadowed_count} lower-priority definition(s) are shadowed."),
        )?;
    }
    for warning in &spec.warnings {
        ctx.renderer
            .line(MessageStyle::Warning, &format!("Warning: {}", warning))?;
    }
    Ok(())
}

fn validate_agent_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        bail!("Agent name cannot be empty");
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        bail!("Agent name must use lowercase letters, digits, or hyphens");
    }
    Ok(())
}

fn scaffold_agent_markdown(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: {description}\ntools:\n  - {read_file}\n  - {list_files}\n  - {unified_search}\nmodel: inherit\ncolor: blue\nreasoning_effort: medium\npermissions:\n  default: deny\n  allow:\n    - {read_file}\n    - {list_files}\n    - {unified_search}\n---\n{body}",
        description = DEFAULT_AGENT_DESCRIPTION_TEXT,
        read_file = DEFAULT_AGENT_TOOL_IDS[0],
        list_files = DEFAULT_AGENT_TOOL_IDS[1],
        unified_search = DEFAULT_AGENT_TOOL_IDS[2],
        body = DEFAULT_AGENT_BODY_TEXT,
    )
}

fn action_item(
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    search_value: &str,
    action: &str,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(ToString::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{AGENT_ACTION_PREFIX}{action}"
        ))),
        search_value: Some(search_value.to_string()),
    }
}

fn agent_badge(spec: &vtcode_config::SubagentSpec) -> String {
    match spec.mode {
        vtcode_config::AgentMode::Primary => "Primary".to_string(),
        vtcode_config::AgentMode::Subagent => "Subagent".to_string(),
        vtcode_config::AgentMode::All => "All".to_string(),
    }
}

fn agent_subtitle(spec: &vtcode_config::SubagentSpec, shadowed: bool) -> String {
    let mut parts = vec![spec.source.label().to_string(), spec.description.clone()];
    let kind = match spec.mode {
        vtcode_config::AgentMode::Primary => "primary",
        vtcode_config::AgentMode::Subagent => "subagent",
        vtcode_config::AgentMode::All => "all",
    };
    parts.push(kind.to_string());
    if spec.is_read_only() {
        parts.push("read-only".to_string());
    }
    if shadowed {
        parts.push("shadowed".to_string());
    }
    parts.join(" | ")
}

fn status_label(status: vtcode_core::subagents::SubagentStatus) -> &'static str {
    match status {
        vtcode_core::subagents::SubagentStatus::Queued => "[queued]",
        vtcode_core::subagents::SubagentStatus::Running => "[running]",
        vtcode_core::subagents::SubagentStatus::Waiting => "[waiting]",
        vtcode_core::subagents::SubagentStatus::Completed => "[completed]",
        vtcode_core::subagents::SubagentStatus::Failed => "[failed]",
        vtcode_core::subagents::SubagentStatus::Closed => "[closed]",
    }
}

async fn refresh_agent_palette(
    handle: &vtcode_ui::tui::app::InlineHandle,
    controller: &vtcode_core::subagents::SubagentController,
) {
    let specs = controller.effective_specs().await;
    handle.configure_agent_palette(
        specs
            .into_iter()
            .filter(|spec| spec.is_subagent())
            .map(|spec| AgentPaletteItem {
                name: spec.name,
                description: Some(spec.description),
            })
            .collect(),
    );
}

#[cfg(test)]
#[path = "agents/tests.rs"]
mod tests;
