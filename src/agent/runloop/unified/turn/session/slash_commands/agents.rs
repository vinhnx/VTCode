use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep,
};

use super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::{AgentDefinitionScope, AgentManagerAction};
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

const AGENT_ACTION_PREFIX: &str = "agents:";
const AGENT_INSPECT_PREFIX: &str = "agents:inspect:";
const THREAD_INSPECT_PREFIX: &str = "agents:thread:";
const PROMPT_QUESTION_ID: &str = "agent-name";

pub(crate) async fn handle_manage_agents(
    ctx: SlashCommandContext<'_>,
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
                if !ensure_selection_ui_available(&mut ctx, "browsing delegated child threads")? {
                    return Ok(SlashCommandControl::Continue);
                }
                show_threads_modal(ctx).await
            } else {
                let mut ctx = ctx;
                handle_list_threads_text(&mut ctx).await
            }
        }
        AgentManagerAction::Create { scope, name } => {
            let mut ctx = ctx;
            handle_create_agent(&mut ctx, scope, &name).await
        }
        AgentManagerAction::Edit { name } => handle_edit_agent(ctx, &name).await,
        AgentManagerAction::Delete { name } => {
            let mut ctx = ctx;
            handle_delete_agent(&mut ctx, &name).await
        }
    }
}

async fn show_agents_manager(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.handle.show_list_modal(
        "Subagents".to_string(),
        vec![
            "Manage effective subagents and delegated child threads.".to_string(),
            "Use Enter to inspect, create, edit, or delete definitions.".to_string(),
        ],
        vec![
            action_item(
                "Browse agents",
                "List effective and shadowed definitions with source badges",
                Some("Recommended"),
                "browse effective shadowed agents",
                "browse",
            ),
            action_item(
                "Browse child threads",
                "Inspect delegated runs and open archived transcripts",
                None,
                "threads delegated transcript",
                "threads",
            ),
            action_item(
                "Create project agent",
                "Scaffold `.vtcode/agents/<name>.md` in this workspace",
                Some("Project"),
                "create project agent scaffold",
                "create-project",
            ),
            action_item(
                "Create user agent",
                "Scaffold `~/.vtcode/agents/<name>.md` for all workspaces",
                Some("User"),
                "create user agent scaffold",
                "create-user",
            ),
            action_item(
                "Edit custom agent",
                "Pick a project or user agent file and open it in your editor",
                None,
                "edit custom agent file",
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
            let name = prompt_agent_name(&mut ctx, "Create project agent", "Agent name").await?;
            if let Some(name) = name {
                handle_create_agent(&mut ctx, AgentDefinitionScope::Project, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        value if value == format!("{AGENT_ACTION_PREFIX}create-user") => {
            let name = prompt_agent_name(&mut ctx, "Create user agent", "Agent name").await?;
            if let Some(name) = name {
                handle_create_agent(&mut ctx, AgentDefinitionScope::User, &name).await
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        value if value == format!("{AGENT_ACTION_PREFIX}edit") => {
            let Some(name) = select_custom_agent_name(&mut ctx, "Edit custom agent").await? else {
                return Ok(SlashCommandControl::Continue);
            };
            handle_edit_agent(ctx, &name).await
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
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
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
        "Loaded subagents".to_string(),
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
            label: "Search subagents".to_string(),
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

async fn show_threads_modal(mut ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let threads = controller.status_entries().await;
    if threads.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "No delegated child threads in main thread {}.",
                ctx.thread_id
            ),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    let items = threads
        .iter()
        .map(|entry| InlineListItem {
            title: format!("{} {}", entry.agent_name, status_label(entry.status)),
            subtitle: Some(format!(
                "{} | {}",
                entry.source,
                entry
                    .summary
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("No summary yet")
            )),
            badge: Some(if entry.background {
                "Background".to_string()
            } else {
                "Foreground".to_string()
            }),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{THREAD_INSPECT_PREFIX}{}",
                entry.id
            ))),
            search_value: Some(format!(
                "{} {} {} {}",
                entry.id, entry.agent_name, entry.source, entry.description
            )),
        })
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        "Delegated child threads".to_string(),
        vec![
            format!("Current main thread: {}.", ctx.thread_id),
            "Select a thread to inspect. Completed threads with archives open in the editor."
                .to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search child threads".to_string(),
            placeholder: Some("id, agent, source, status".to_string()),
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(&mut ctx).await else {
        return Ok(SlashCommandControl::Continue);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(SlashCommandControl::Continue);
    };
    let Some(id) = action.strip_prefix(THREAD_INSPECT_PREFIX) else {
        return Ok(SlashCommandControl::Continue);
    };
    let entry = threads
        .into_iter()
        .find(|entry| entry.id == id)
        .ok_or_else(|| anyhow!("Unknown delegated thread {}", id))?;

    if let Some(path) = entry.transcript_path.clone() {
        return super::apps::handle_launch_editor(ctx, Some(path.display().to_string())).await;
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!(
            "{} {} {}",
            entry.id,
            entry.agent_name,
            status_label(entry.status)
        ),
    )?;
    ctx.renderer
        .line(MessageStyle::Output, &format!("Source: {}", entry.source))?;
    ctx.renderer.line(
        MessageStyle::Output,
        &format!("Description: {}", entry.description),
    )?;
    if let Some(summary) = entry
        .summary
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        ctx.renderer.line(
            MessageStyle::Output,
            &format!("Summary: {}", summary.trim()),
        )?;
    }
    if let Some(error) = entry
        .error
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        ctx.renderer
            .line(MessageStyle::Error, &format!("Error: {}", error.trim()))?;
    }
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
            "Loaded {} effective subagents ({} shadowed definitions).",
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
            .line(MessageStyle::Info, "No delegated child threads yet.")?;
    } else {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("{} delegated child thread(s):", threads.len()),
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

async fn handle_list_threads_text(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    let Some(controller) = ctx.tool_registry.subagent_controller() else {
        ctx.renderer.line(
            MessageStyle::Info,
            "Subagent controller is not active in this session.",
        )?;
        return Ok(SlashCommandControl::Continue);
    };

    let threads = controller.status_entries().await;
    if threads.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            &format!(
                "No delegated child threads in main thread {}.",
                ctx.thread_id
            ),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Delegated child threads for main thread {}:", ctx.thread_id),
    )?;
    for entry in threads {
        let summary = entry.summary.unwrap_or_default();
        let summary = summary.trim();
        let suffix = if summary.is_empty() {
            String::new()
        } else {
            format!(" - {}", summary)
        };
        ctx.renderer.line(
            MessageStyle::Output,
            &format!(
                "{} {} {}{}",
                entry.id,
                entry.agent_name,
                status_label(entry.status),
                suffix
            ),
        )?;
    }
    Ok(SlashCommandControl::Continue)
}

async fn handle_create_agent(
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

    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow!("Invalid agent destination {}", path.display()))?,
    )?;
    std::fs::write(&path, scaffold_agent_markdown(name))?;

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
    }

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Created agent scaffold at {}.", path.display()),
    )?;
    Ok(SlashCommandControl::Continue)
}

async fn handle_edit_agent(
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
    std::fs::remove_file(&path)?;
    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
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

async fn prompt_agent_name(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    freeform_label: &str,
) -> Result<Option<String>> {
    let step = WizardStep {
        title: "Name".to_string(),
        question: "Enter a lowercase hyphenated agent name.".to_string(),
        items: vec![InlineListItem {
            title: "Save".to_string(),
            subtitle: Some(
                "Press Tab to type the agent name, then Enter to scaffold it.".to_string(),
            ),
            badge: Some("Required".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: PROMPT_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("save agent name".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some("example-agent".to_string()),
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
    let Some(value) = (match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == PROMPT_QUESTION_ID => {
                        other.or_else(|| selected.first().cloned())
                    }
                    _ => None,
                })
        }
        WizardModalOutcome::Cancelled { .. } => None,
    }) else {
        return Ok(None);
    };

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Ok(None);
    }
    validate_agent_name(&trimmed)?;
    Ok(Some(trimmed))
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
        "---\nname: {name}\ndescription: Describe when VT Code should delegate to this agent\ntools: Read, Grep, Glob\nmodel: inherit\n---\n\nDescribe the agent's focused behavior, constraints, and expected output.\n"
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
    match spec.file_path {
        Some(_) => spec.source.label().to_string(),
        None => "Built-in".to_string(),
    }
}

fn agent_subtitle(spec: &vtcode_config::SubagentSpec, shadowed: bool) -> String {
    let mut parts = vec![spec.source.label().to_string(), spec.description.clone()];
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
