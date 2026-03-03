use std::sync::Arc;

use anyhow::Result;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::llm::provider as uni;
use vtcode_core::skills::executor::SkillToolAdapter;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::tools::ToolRegistration;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::{
    InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode,
    WizardStep,
};

use crate::agent::runloop::unified::turn::utils::{
    enforce_history_limits, truncate_message_content,
};
use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, wait_for_wizard_modal};
use crate::agent::runloop::{SkillCommandAction, SkillCommandOutcome, handle_skill_command};

use super::{SlashCommandContext, SlashCommandControl};

const SKILL_ACTION_PREFIX: &str = "skills.action.";
const SKILL_ACTION_BACK: &str = "skills.action.back";
const SKILL_OPEN_PREFIX: &str = "skills.open.";
const SKILL_ENABLE_PREFIX: &str = "skills.enable.";
const SKILL_DISABLE_PREFIX: &str = "skills.disable.";
const SKILL_INFO_PREFIX: &str = "skills.info.";
const SKILL_USE_PREFIX: &str = "skills.use.";
const SKILL_VALIDATE_PREFIX: &str = "skills.validate.";
const SKILL_PACKAGE_PREFIX: &str = "skills.package.";
const SKILL_BACK_ACTION: &str = "skills.back";
const SKILL_PICK_PREFIX: &str = "skills.pick.";
const SKILL_PICK_BACK_ACTION: &str = "skills.pick.back";
const SKILL_PROMPT_QUESTION_ID: &str = "skills.input";

#[derive(Clone)]
struct InteractiveSkillEntry {
    name: String,
    description: String,
    loaded: bool,
}

#[derive(Clone, Copy)]
enum SkillFilter {
    Any,
    Loaded,
    Unloaded,
}

impl SkillFilter {
    fn matches(self, entry: &InteractiveSkillEntry) -> bool {
        match self {
            SkillFilter::Any => true,
            SkillFilter::Loaded => entry.loaded,
            SkillFilter::Unloaded => !entry.loaded,
        }
    }
}

pub async fn handle_manage_skills(
    mut ctx: SlashCommandContext<'_>,
    action: SkillCommandAction,
) -> Result<SlashCommandControl> {
    super::activation::ensure_skills_context_activated(&ctx).await?;

    if matches!(action, SkillCommandAction::Interactive) {
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

async fn execute_skill_action(
    ctx: &mut SlashCommandContext<'_>,
    action: SkillCommandAction,
) -> Result<()> {
    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;
    let _ = apply_skill_command_outcome(ctx, outcome).await?;
    Ok(())
}

async fn run_interactive_skills_manager(
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        execute_skill_action(ctx, SkillCommandAction::Help).await?;
        return Ok(SlashCommandControl::Continue);
    }

    loop {
        show_skills_manager_actions_modal(ctx);
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(SlashCommandControl::Continue);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };

        if action == SKILL_ACTION_BACK {
            return Ok(SlashCommandControl::Continue);
        }

        let Some(action_key) = action.strip_prefix(SKILL_ACTION_PREFIX) else {
            continue;
        };

        match action_key {
            "browse" => run_skill_browser(ctx).await?,
            "list" => execute_skill_action(ctx, SkillCommandAction::List { query: None }).await?,
            "search" => {
                if let Some(query) = prompt_required_text(
                    ctx,
                    "Search Skills",
                    "Provide a query to filter by name or description.",
                    "Query:",
                    "Type search text",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::List { query: Some(query) })
                        .await?;
                }
            }
            "create" => {
                if let Some(name) = prompt_required_text(
                    ctx,
                    "Create Skill",
                    "Provide a new skill name (kebab-case recommended).",
                    "Name:",
                    "Type skill name",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Create { name, path: None })
                        .await?;
                }
            }
            "load" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Unloaded,
                    "Enable Skill",
                    "Select a skill to enable for this session.",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Load { name }).await?;
                }
            }
            "unload" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Loaded,
                    "Disable Skill",
                    "Select an enabled skill to unload from this session.",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Unload { name }).await?;
                }
            }
            "info" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Any,
                    "Skill Details",
                    "Select a skill to inspect.",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Info { name }).await?;
                }
            }
            "use" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Any,
                    "Run Skill",
                    "Select a skill to execute with input.",
                )
                .await?
                    && let Some(input) = prompt_optional_text(
                        ctx,
                        "Run Skill",
                        "Provide input for this skill run (optional).",
                        "Input:",
                        "Type skill input (optional)",
                    )
                    .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Use { name, input }).await?;
                }
            }
            "validate" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Any,
                    "Validate Skill",
                    "Select a skill to validate.",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Validate { name }).await?;
                }
            }
            "package" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Any,
                    "Package Skill",
                    "Select a skill to package to .skill.",
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Package { name }).await?;
                }
            }
            "regen" => execute_skill_action(ctx, SkillCommandAction::RegenerateIndex).await?,
            "help" => execute_skill_action(ctx, SkillCommandAction::Help).await?,
            _ => continue,
        }
    }
}

async fn run_skill_browser(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    loop {
        let entries = discover_interactive_skills(ctx).await?;
        if entries.is_empty() {
            ctx.renderer.line(
                MessageStyle::Info,
                "No skills found. Use /skills --create <name> to scaffold a new one.",
            )?;
            return Ok(());
        }

        show_skills_list_modal(ctx, &entries);
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(());
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == SKILL_PICK_BACK_ACTION {
            return Ok(());
        }

        let Some(skill_name) = action.strip_prefix(SKILL_OPEN_PREFIX) else {
            continue;
        };
        let Some(entry) = entries
            .iter()
            .find(|candidate| candidate.name == skill_name)
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
            execute_skill_action(
                ctx,
                SkillCommandAction::Load {
                    name: name.to_string(),
                },
            )
            .await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_DISABLE_PREFIX) {
            execute_skill_action(
                ctx,
                SkillCommandAction::Unload {
                    name: name.to_string(),
                },
            )
            .await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_INFO_PREFIX) {
            execute_skill_action(
                ctx,
                SkillCommandAction::Info {
                    name: name.to_string(),
                },
            )
            .await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_USE_PREFIX) {
            if let Some(input) = prompt_optional_text(
                ctx,
                &format!("Run Skill: {}", name),
                "Provide input for this skill run (optional).",
                "Input:",
                "Type skill input (optional)",
            )
            .await?
            {
                execute_skill_action(
                    ctx,
                    SkillCommandAction::Use {
                        name: name.to_string(),
                        input,
                    },
                )
                .await?;
            }
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_VALIDATE_PREFIX) {
            execute_skill_action(
                ctx,
                SkillCommandAction::Validate {
                    name: name.to_string(),
                },
            )
            .await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_PACKAGE_PREFIX) {
            execute_skill_action(
                ctx,
                SkillCommandAction::Package {
                    name: name.to_string(),
                },
            )
            .await?;
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

async fn pick_skill_name(
    ctx: &mut SlashCommandContext<'_>,
    filter: SkillFilter,
    title: &str,
    description: &str,
) -> Result<Option<String>> {
    let entries = discover_interactive_skills(ctx).await?;
    let filtered: Vec<InteractiveSkillEntry> = entries
        .into_iter()
        .filter(|entry| filter.matches(entry))
        .collect();

    if filtered.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No matching skills available for that action.",
        )?;
        return Ok(None);
    }

    show_skill_picker_modal(ctx, title, description, &filtered);
    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };

    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    if action == SKILL_PICK_BACK_ACTION {
        return Ok(None);
    }

    Ok(action
        .strip_prefix(SKILL_PICK_PREFIX)
        .map(std::string::ToString::to_string))
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
            .line(MessageStyle::Info, "Input was empty. Nothing executed.")?;
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
                question_id: SKILL_PROMPT_QUESTION_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("submit input".to_string()),
        }],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
    };

    ctx.handle.show_wizard_modal_with_mode(
        title.to_string(),
        vec![step],
        0,
        None,
        WizardModalMode::MultiStep,
    );
    ctx.handle.force_redraw();

    let outcome =
        wait_for_wizard_modal(ctx.handle, ctx.session, ctx.ctrl_c_state, ctx.ctrl_c_notify).await?;
    let value = match outcome {
        WizardModalOutcome::Submitted(selections) => {
            selections
                .into_iter()
                .find_map(|selection| match selection {
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other,
                    } if question_id == SKILL_PROMPT_QUESTION_ID => {
                        if let Some(other) = other {
                            Some(other)
                        } else {
                            selected.first().cloned()
                        }
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

fn show_skills_manager_actions_modal(ctx: &mut SlashCommandContext<'_>) {
    let items = vec![
        InlineListItem {
            title: "Browse skills".to_string(),
            subtitle: Some("Open the skills catalog and per-skill actions".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}browse",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("browse catalog skills".to_string()),
        },
        InlineListItem {
            title: "List skills".to_string(),
            subtitle: Some("Show all discoverable skills".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}list",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("list skills".to_string()),
        },
        InlineListItem {
            title: "Search skills".to_string(),
            subtitle: Some("Filter skills by name or description".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}search",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("search query".to_string()),
        },
        InlineListItem {
            title: "Create skill".to_string(),
            subtitle: Some("Scaffold a new skill template".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}create",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("create scaffold".to_string()),
        },
        InlineListItem {
            title: "Enable skill".to_string(),
            subtitle: Some("Load a skill into the current session".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}load",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("enable load".to_string()),
        },
        InlineListItem {
            title: "Disable skill".to_string(),
            subtitle: Some("Unload an enabled skill from this session".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}unload",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("disable unload".to_string()),
        },
        InlineListItem {
            title: "View skill details".to_string(),
            subtitle: Some("Show metadata and instructions".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}info",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("details info metadata".to_string()),
        },
        InlineListItem {
            title: "Run skill".to_string(),
            subtitle: Some("Execute a skill with optional input".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}use",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("run execute use".to_string()),
        },
        InlineListItem {
            title: "Validate skill".to_string(),
            subtitle: Some("Validate skill structure".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}validate",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("validate lint".to_string()),
        },
        InlineListItem {
            title: "Package skill".to_string(),
            subtitle: Some("Package a skill to .skill file".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}package",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("package bundle".to_string()),
        },
        InlineListItem {
            title: "Regenerate index".to_string(),
            subtitle: Some("Rebuild skills index file".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}regen",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("index regenerate".to_string()),
        },
        InlineListItem {
            title: "Show help".to_string(),
            subtitle: Some("Display `/skills` command help".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}help",
                SKILL_ACTION_PREFIX
            ))),
            search_value: Some("help commands".to_string()),
        },
        InlineListItem {
            title: "Back".to_string(),
            subtitle: Some("Close skills manager".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                SKILL_ACTION_BACK.to_string(),
            )),
            search_value: Some("back close".to_string()),
        },
    ];

    ctx.renderer.show_list_modal(
        "Skills Manager",
        vec![
            "Configure skills interactively.".to_string(),
            "Use Enter to run an action, Esc to close.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(format!(
            "{}browse",
            SKILL_ACTION_PREFIX
        ))),
        None,
    );
}

fn show_skills_list_modal(ctx: &mut SlashCommandContext<'_>, entries: &[InteractiveSkillEntry]) {
    let mut items: Vec<InlineListItem> = entries
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

    items.push(InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Return to skills actions".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            SKILL_PICK_BACK_ACTION.to_string(),
        )),
        search_value: Some("back".to_string()),
    });

    let selected = entries.first().map(|entry| {
        InlineListSelection::ConfigAction(format!("{}{}", SKILL_OPEN_PREFIX, entry.name))
    });

    ctx.renderer.show_list_modal(
        "Skills Manager",
        vec![
            "Browse and manage discovered skills.".to_string(),
            "Browse skills and press Enter for actions.".to_string(),
        ],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search skills".to_string(),
            placeholder: Some("Type skill name or description".to_string()),
        }),
    );
}

fn show_skill_picker_modal(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    description: &str,
    entries: &[InteractiveSkillEntry],
) {
    let mut items: Vec<InlineListItem> = entries
        .iter()
        .map(|entry| InlineListItem {
            title: entry.name.clone(),
            subtitle: Some(entry.description.clone()),
            badge: Some(if entry.loaded { "Enabled" } else { "Available" }.to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                SKILL_PICK_PREFIX, entry.name
            ))),
            search_value: Some(format!("{} {}", entry.name, entry.description)),
        })
        .collect();

    items.push(InlineListItem {
        title: "Back".to_string(),
        subtitle: Some("Cancel and return".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(
            SKILL_PICK_BACK_ACTION.to_string(),
        )),
        search_value: Some("back cancel".to_string()),
    });

    let selected = entries.first().map(|entry| {
        InlineListSelection::ConfigAction(format!("{}{}", SKILL_PICK_PREFIX, entry.name))
    });

    ctx.renderer.show_list_modal(
        title,
        vec![description.to_string()],
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
        title: "Run skill".to_string(),
        subtitle: Some("Execute this skill with optional input".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            SKILL_USE_PREFIX, entry.name
        ))),
        search_value: Some("run execute use".to_string()),
    });

    items.push(InlineListItem {
        title: "Validate".to_string(),
        subtitle: Some("Validate this skill structure".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            SKILL_VALIDATE_PREFIX, entry.name
        ))),
        search_value: Some("validate".to_string()),
    });

    items.push(InlineListItem {
        title: "Package".to_string(),
        subtitle: Some("Package this skill to .skill".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{}{}",
            SKILL_PACKAGE_PREFIX, entry.name
        ))),
        search_value: Some("package bundle".to_string()),
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
