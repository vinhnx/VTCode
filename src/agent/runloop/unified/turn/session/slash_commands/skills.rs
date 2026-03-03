use std::sync::Arc;

use anyhow::Result;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::llm::provider as uni;
use vtcode_core::skills::executor::SkillToolAdapter;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::tools::ToolRegistration;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::{InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection};

use crate::agent::runloop::SkillCommandOutcome;
use crate::agent::runloop::handle_skill_command;
use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};

use super::{SlashCommandContext, SlashCommandControl};

const SKILL_OPEN_PREFIX: &str = "skills.open.";
const SKILL_ENABLE_PREFIX: &str = "skills.enable.";
const SKILL_DISABLE_PREFIX: &str = "skills.disable.";
const SKILL_INFO_PREFIX: &str = "skills.info.";
const SKILL_BACK_ACTION: &str = "skills.back";

#[derive(Clone)]
struct InteractiveSkillEntry {
    name: String,
    description: String,
    loaded: bool,
}

pub async fn handle_manage_skills(
    mut ctx: SlashCommandContext<'_>,
    action: crate::agent::runloop::SkillCommandAction,
) -> Result<SlashCommandControl> {
    super::activation::ensure_skills_context_activated(&ctx).await?;

    if matches!(
        action,
        crate::agent::runloop::SkillCommandAction::Interactive
    ) {
        return run_interactive_skills_manager(&mut ctx).await;
    }

    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;
    apply_skill_command_outcome(&mut ctx, outcome).await
}

async fn apply_skill_command_outcome(
    ctx: &mut SlashCommandContext<'_>,
    outcome: SkillCommandOutcome,
) -> Result<SlashCommandControl> {
    match outcome {
        SkillCommandOutcome::Handled { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::LoadSkill { skill, message } => {
            let skill_name = skill.name().to_string();

            let adapter = SkillToolAdapter::new(skill.clone());
            let adapter_arc = Arc::new(adapter);

            let name_static: &'static str = Box::leak(Box::new(skill_name.clone()));
            let registration =
                ToolRegistration::from_tool(name_static, CapabilityLevel::Bash, adapter_arc);

            if let Err(e) = ctx.tool_registry.register_tool(registration).await {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to register skill as tool: {}", e),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            ctx.loaded_skills
                .write()
                .await
                .insert(skill_name.clone(), skill.clone());

            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UnloadSkill { name } => {
            ctx.loaded_skills.write().await.remove(&name);
            ctx.renderer
                .line(MessageStyle::Info, &format!("Unloaded skill: {}", name))?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UseSkill { skill, input } => {
            use vtcode_core::skills::execute_skill_with_sub_llm;

            let skill_name = skill.name().to_string();
            let available_tools = ctx.tools.read().await.clone();
            let model = ctx.config.model.clone();

            match execute_skill_with_sub_llm(
                &skill,
                input,
                ctx.provider_client.as_ref(),
                ctx.tool_registry,
                available_tools,
                model,
            )
            .await
            {
                Ok(result) => {
                    ctx.renderer.line(MessageStyle::Output, &result)?;
                    ctx.conversation_history.push(uni::Message::user(format!(
                        "/skills use {} [executed]",
                        skill_name
                    )));

                    let result_string: String = result;
                    let limited = truncate_message_content(&result_string);
                    ctx.conversation_history
                        .push(uni::Message::assistant(limited));
                    enforce_history_limits(ctx.conversation_history);
                    Ok(SlashCommandControl::Continue)
                }
                Err(e) => {
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to execute skill: {}", e),
                    )?;
                    Ok(SlashCommandControl::Continue)
                }
            }
        }
        SkillCommandOutcome::Error { message } => {
            ctx.renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

async fn run_interactive_skills_manager(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        let fallback = handle_skill_command(
            crate::agent::runloop::SkillCommandAction::Help,
            ctx.config.workspace.clone(),
        )
        .await?;
        return apply_skill_command_outcome(ctx, fallback).await;
    }

    loop {
        let entries = discover_interactive_skills(ctx).await?;
        if entries.is_empty() {
            ctx.renderer.line(
                MessageStyle::Info,
                "No skills found. Use /skills --create <name> to scaffold a new one.",
            )?;
            return Ok(SlashCommandControl::Continue);
        }

        show_skills_list_modal(ctx, &entries);
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        let Some(skill_name) = action.strip_prefix(SKILL_OPEN_PREFIX) else {
            continue;
        };
        let Some(entry) = entries
            .iter()
            .find(|entry| entry.name == skill_name)
            .cloned()
        else {
            continue;
        };

        show_skill_actions_modal(ctx, &entry);
        let Some(skill_action) = wait_for_list_modal_selection(ctx).await else {
            continue;
        };
        let InlineListSelection::ConfigAction(skill_action) = skill_action else {
            continue;
        };

        if skill_action == SKILL_BACK_ACTION {
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_ENABLE_PREFIX) {
            let outcome = handle_skill_command(
                crate::agent::runloop::SkillCommandAction::Load {
                    name: name.to_string(),
                },
                ctx.config.workspace.clone(),
            )
            .await?;
            let _ = apply_skill_command_outcome(ctx, outcome).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_DISABLE_PREFIX) {
            let outcome = handle_skill_command(
                crate::agent::runloop::SkillCommandAction::Unload {
                    name: name.to_string(),
                },
                ctx.config.workspace.clone(),
            )
            .await?;
            let _ = apply_skill_command_outcome(ctx, outcome).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_INFO_PREFIX) {
            let outcome = handle_skill_command(
                crate::agent::runloop::SkillCommandAction::Info {
                    name: name.to_string(),
                },
                ctx.config.workspace.clone(),
            )
            .await?;
            let _ = apply_skill_command_outcome(ctx, outcome).await?;
            continue;
        }
    }
}

async fn discover_interactive_skills(
    ctx: &SlashCommandContext<'_>,
) -> Result<Vec<InteractiveSkillEntry>> {
    let mut loader = EnhancedSkillLoader::new(ctx.config.workspace.clone());
    let discovered = loader.discover_all_skills().await?;
    let loaded = ctx.loaded_skills.read().await;

    let mut entries: Vec<InteractiveSkillEntry> = discovered
        .skills
        .into_iter()
        .map(|skill_ctx| {
            let manifest = skill_ctx.manifest();
            InteractiveSkillEntry {
                name: manifest.name.clone(),
                description: manifest.description.clone(),
                loaded: loaded.contains_key(&manifest.name),
            }
        })
        .collect();

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

fn show_skills_list_modal(ctx: &mut SlashCommandContext<'_>, entries: &[InteractiveSkillEntry]) {
    let items: Vec<InlineListItem> = entries
        .iter()
        .map(|entry| InlineListItem {
            title: entry.name.clone(),
            subtitle: Some(entry.description.clone()),
            badge: Some(if entry.loaded { "Enabled" } else { "Available" }.to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                SKILL_OPEN_PREFIX, entry.name
            ))),
            search_value: Some(format!("{} {}", entry.name, entry.description)),
        })
        .collect();

    let selected = entries.first().map(|entry| {
        InlineListSelection::ConfigAction(format!("{}{}", SKILL_OPEN_PREFIX, entry.name))
    });

    ctx.renderer.show_list_modal(
        "Skills",
        vec![
            "Browse skills and press Enter for actions.".to_string(),
            "Choose a skill to enable/disable or inspect details.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search skills".to_string(),
            placeholder: Some("Type skill name or description".to_string()),
        }),
    );
}

fn show_skill_actions_modal(ctx: &mut SlashCommandContext<'_>, entry: &InteractiveSkillEntry) {
    let mut items = Vec::new();
    if entry.loaded {
        items.push(InlineListItem {
            title: "Disable for this session".to_string(),
            subtitle: Some("Unload this skill from the active session".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                SKILL_DISABLE_PREFIX, entry.name
            ))),
            search_value: Some("disable unload session".to_string()),
        });
    } else {
        items.push(InlineListItem {
            title: "Enable for this session".to_string(),
            subtitle: Some("Load this skill into the active session".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                SKILL_ENABLE_PREFIX, entry.name
            ))),
            search_value: Some("enable load session".to_string()),
        });
    }

    items.push(InlineListItem {
        title: "View details".to_string(),
        subtitle: Some("Show full skill metadata and instructions".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            SKILL_INFO_PREFIX, entry.name
        ))),
        search_value: Some("details info metadata".to_string()),
    });

    items.push(InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Return to skills list".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            SKILL_BACK_ACTION.to_string(),
        )),
        search_value: Some("back return".to_string()),
    });

    let default_selection = items.first().and_then(|item| item.selection.clone());
    let title = format!("Skill: {}", entry.name);
    ctx.renderer.show_list_modal(
        &title,
        vec![entry.description.clone()],
        items,
        default_selection,
        None,
    );
}

async fn wait_for_list_modal_selection(
    ctx: &mut SlashCommandContext<'_>,
) -> Option<InlineListSelection> {
    loop {
        if ctx.ctrl_c_state.is_cancel_requested() {
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            return None;
        }

        let notify = ctx.ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = ctx.session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            ctx.handle.close_modal();
            ctx.handle.force_redraw();
            return None;
        };

        match event {
            InlineEvent::ListModalSubmit(selection) => {
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return Some(selection);
            }
            InlineEvent::ListModalCancel | InlineEvent::Cancel | InlineEvent::Exit => {
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            InlineEvent::Interrupt => {
                ctx.ctrl_c_state.register_signal();
                ctx.ctrl_c_notify.notify_waiters();
                ctx.handle.close_modal();
                ctx.handle.force_redraw();
                return None;
            }
            _ => continue,
        }
    }
}
