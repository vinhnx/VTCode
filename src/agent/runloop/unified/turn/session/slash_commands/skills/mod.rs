use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;

use anyhow::Result;
use vtcode_core::skills::loader::EnhancedSkillLoader;
use vtcode_core::skills::types::SkillVariety;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep};

use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};
use crate::agent::runloop::{SkillCommandAction, handle_skill_command};

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
    kind: InteractiveSkillKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InteractiveSkillKind {
    Traditional,
    BuiltIn,
    Utility,
}

impl InteractiveSkillEntry {
    fn is_loadable(&self) -> bool {
        matches!(self.kind, InteractiveSkillKind::Traditional)
    }

    fn supports_validation(&self) -> bool {
        matches!(self.kind, InteractiveSkillKind::Traditional)
    }

    fn supports_packaging(&self) -> bool {
        matches!(self.kind, InteractiveSkillKind::Traditional)
    }

    fn badge(&self) -> String {
        if self.loaded {
            return "Enabled".to_string();
        }

        match self.kind {
            InteractiveSkillKind::Traditional => "Available".to_string(),
            InteractiveSkillKind::BuiltIn => "Built-in".to_string(),
            InteractiveSkillKind::Utility => "Utility".to_string(),
        }
    }
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

pub(crate) async fn handle_manage_skills(
    mut ctx: SlashCommandContext<'_>,
    action: SkillCommandAction,
) -> Result<SlashCommandControl> {
    super::activation::ensure_skills_context_activated(&ctx).await?;

    if matches!(action, SkillCommandAction::Interactive) {
        return run_interactive_skills_manager(&mut ctx).await;
    }

    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;
    outcome::apply_skill_command_outcome(&mut ctx, outcome).await
}

async fn execute_skill_action(ctx: &mut SlashCommandContext<'_>, action: SkillCommandAction) -> Result<()> {
    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;
    let _ = outcome::apply_skill_command_outcome(ctx, outcome).await?;
    Ok(())
}

async fn run_interactive_skills_manager(ctx: &mut SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        execute_skill_action(ctx, SkillCommandAction::Help).await?;
        return Ok(SlashCommandControl::Continue);
    }

    loop {
        show_skills_manager_actions_modal(ctx);
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
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
                    execute_skill_action(ctx, SkillCommandAction::List { query: Some(query) }).await?;
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
                    execute_skill_action(ctx, SkillCommandAction::Create { name, path: None }).await?;
                }
            }
            "load" => {
                if let Some(name) = pick_skill_name(
                    ctx,
                    SkillFilter::Unloaded,
                    "Enable Skill",
                    "Select a skill to enable for this session.",
                    PickerMode::TraditionalOnly,
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
                    PickerMode::TraditionalOnly,
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
                    PickerMode::Any,
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
                    PickerMode::Any,
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
                    PickerMode::TraditionalOnly,
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
                    PickerMode::TraditionalOnly,
                )
                .await?
                {
                    execute_skill_action(ctx, SkillCommandAction::Package { name }).await?;
                }
            }
            "regen" => execute_skill_action(ctx, SkillCommandAction::RegenerateIndex).await?,
            "doctor" => execute_skill_action(ctx, SkillCommandAction::Doctor).await?,
            "help" => execute_skill_action(ctx, SkillCommandAction::Help).await?,
            _ => continue,
        }
    }
}

async fn run_skill_browser(ctx: &mut SlashCommandContext<'_>) -> Result<()> {
    loop {
        let entries = discover_interactive_skills(ctx).await?;
        if entries.is_empty() {
            ctx.renderer
                .line(MessageStyle::Info, "No skills found. Use /skills --create <name> to scaffold a new one.")?;
            return Ok(());
        }

        show_skills_list_modal(ctx, &entries);
        let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
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
        let Some(entry) = entries.iter().find(|candidate| candidate.name == skill_name).cloned() else {
            continue;
        };

        show_skill_actions_modal(ctx, &entry);
        let Some(skill_action) = super::ui::wait_for_list_modal_selection(ctx).await else {
            continue;
        };
        let InlineListSelection::ConfigAction(skill_action) = skill_action else {
            continue;
        };
        if skill_action == SKILL_BACK_ACTION {
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_ENABLE_PREFIX) {
            execute_skill_action(ctx, SkillCommandAction::Load { name: name.to_string() }).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_DISABLE_PREFIX) {
            execute_skill_action(ctx, SkillCommandAction::Unload { name: name.to_string() }).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_INFO_PREFIX) {
            execute_skill_action(ctx, SkillCommandAction::Info { name: name.to_string() }).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_USE_PREFIX) {
            if let Some(input) = prompt_optional_text(
                ctx,
                &format!("Run Skill: {name}"),
                "Provide input for this skill run (optional).",
                "Input:",
                "Type skill input (optional)",
            )
            .await?
            {
                execute_skill_action(ctx, SkillCommandAction::Use { name: name.to_string(), input }).await?;
            }
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_VALIDATE_PREFIX) {
            execute_skill_action(ctx, SkillCommandAction::Validate { name: name.to_string() }).await?;
            continue;
        }

        if let Some(name) = skill_action.strip_prefix(SKILL_PACKAGE_PREFIX) {
            execute_skill_action(ctx, SkillCommandAction::Package { name: name.to_string() }).await?;
            continue;
        }
    }
}

async fn discover_interactive_skills(ctx: &SlashCommandContext<'_>) -> Result<Vec<InteractiveSkillEntry>> {
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
                kind: if manifest.variety == SkillVariety::BuiltIn {
                    InteractiveSkillKind::BuiltIn
                } else {
                    InteractiveSkillKind::Traditional
                },
            }
        })
        .collect();
    entries.extend(discovered.tools.into_iter().map(|tool| InteractiveSkillEntry {
        name: tool.name,
        description: tool.description,
        loaded: false,
        kind: InteractiveSkillKind::Utility,
    }));

    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

async fn pick_skill_name(
    ctx: &mut SlashCommandContext<'_>,
    filter: SkillFilter,
    title: &str,
    description: &str,
    picker_mode: PickerMode,
) -> Result<Option<String>> {
    let entries = discover_interactive_skills(ctx).await?;
    let filtered: Vec<InteractiveSkillEntry> = entries
        .into_iter()
        .filter(|entry| filter.matches(entry) && picker_mode.allows(entry))
        .collect();

    if filtered.is_empty() {
        ctx.renderer
            .line(MessageStyle::Info, "No matching skills available for that action.")?;
        return Ok(None);
    }

    show_skill_picker_modal(ctx, title, description, &filtered);
    let Some(selection) = super::ui::wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };

    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    if action == SKILL_PICK_BACK_ACTION {
        return Ok(None);
    }

    Ok(action.strip_prefix(SKILL_PICK_PREFIX).map(ToString::to_string))
}

#[derive(Clone, Copy)]
enum PickerMode {
    Any,
    TraditionalOnly,
}

impl PickerMode {
    fn allows(self, entry: &InteractiveSkillEntry) -> bool {
        match self {
            PickerMode::Any => true,
            PickerMode::TraditionalOnly => entry.is_loadable(),
        }
    }
}

async fn prompt_required_text(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    freeform_label: &str,
    placeholder: &str,
) -> Result<Option<String>> {
    let Some(value) = prompt_text(ctx, title, question, freeform_label, placeholder, false).await? else {
        return Ok(None);
    };

    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        ctx.renderer.line(MessageStyle::Info, "Input was empty. Nothing executed.")?;
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
        items: vec![
            ui_list::choice(
                "Submit",
                Some("Press Tab to type text, then Enter to submit.".to_string()),
                Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: SKILL_PROMPT_QUESTION_ID.to_string(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
            )
            .with_search_value("submit input".to_string()),
        ],
        completed: false,
        answer: None,
        allow_freeform: true,
        freeform_label: Some(freeform_label.to_string()),
        freeform_placeholder: Some(placeholder.to_string()),
        freeform_default: None,
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
        WizardModalOutcome::Submitted(selections) => selections.into_iter().find_map(|selection| match selection {
            InlineListSelection::RequestUserInputAnswer { question_id, selected, other }
                if question_id == SKILL_PROMPT_QUESTION_ID =>
            {
                if let Some(other) = other {
                    Some(other)
                } else {
                    selected.first().cloned()
                }
            }
            _ => None,
        }),
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
        ui_list::group_header("Actions"),
        ui_list::action(
            "Browse skills",
            "Open the skills catalog and per-skill actions".to_string(),
            Some("Recommended".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}browse"))),
        )
        .with_search_value("browse catalog skills".to_string()),
        ui_list::action(
            "List skills",
            "Show all discoverable skills".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}list"))),
        )
        .with_search_value("list skills".to_string()),
        ui_list::action(
            "Search skills",
            "Filter skills by name or description".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}search"))),
        )
        .with_search_value("search query".to_string()),
        ui_list::action(
            "Create skill",
            "Scaffold a new skill template".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}create"))),
        )
        .with_search_value("create scaffold".to_string()),
        ui_list::action(
            "Enable skill",
            "Load a skill into the current session".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}load"))),
        )
        .with_search_value("enable load".to_string()),
        ui_list::action(
            "Disable skill",
            "Unload an enabled skill from this session".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}unload"))),
        )
        .with_search_value("disable unload".to_string()),
        ui_list::action(
            "View skill details",
            "Show metadata and instructions".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}info"))),
        )
        .with_search_value("details info metadata".to_string()),
        ui_list::action(
            "Run skill",
            "Execute a skill with optional input".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}use"))),
        )
        .with_search_value("run execute use".to_string()),
        ui_list::action(
            "Validate skill",
            "Validate skill structure".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}validate"))),
        )
        .with_search_value("validate lint".to_string()),
        ui_list::action(
            "Package skill",
            "Package a skill to .skill file".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}package"))),
        )
        .with_search_value("package bundle".to_string()),
        ui_list::action(
            "Regenerate index",
            "Rebuild skills index file".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}regen"))),
        )
        .with_search_value("index regenerate".to_string()),
        ui_list::action(
            "Skill health",
            "Show skill usage and prune candidates".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}doctor"))),
        )
        .with_search_value("health doctor usage stats".to_string()),
        ui_list::action(
            "Show help",
            "Display `/skills` command help".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}help"))),
        )
        .with_search_value("help commands".to_string()),
        ui_list::action(
            "Back",
            "Close skills manager".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(SKILL_ACTION_BACK.to_string())),
        )
        .with_search_value("back close".to_string()),
    ];

    ctx.renderer.show_list_modal(
        "Skills Manager",
        vec!["Configure skills interactively.".to_string()],
        items,
        Some(InlineListSelection::ConfigAction(format!("{SKILL_ACTION_PREFIX}browse"))),
        None,
    );
}

fn show_skills_list_modal(ctx: &mut SlashCommandContext<'_>, entries: &[InteractiveSkillEntry]) {
    let mut items: Vec<InlineListItem> = entries
        .iter()
        .map(|entry| {
            ui_list::action(
                entry.name.clone(),
                entry.description.clone(),
                Some(entry.badge()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_OPEN_PREFIX, entry.name))),
            )
            .with_search_value(format!("{} {}", entry.name, entry.description))
        })
        .collect();

    items.push(
        ui_list::action(
            "Back",
            "Return to skills actions".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(SKILL_PICK_BACK_ACTION.to_string())),
        )
        .with_search_value("back".to_string()),
    );

    let selected = entries
        .first()
        .map(|entry| InlineListSelection::ConfigAction(format!("{}{}", SKILL_OPEN_PREFIX, entry.name)));

    ctx.renderer.show_list_modal(
        "Skills Manager",
        vec!["Browse and manage discovered skills.".to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("skill name or description".to_string()),
            fuzzy: false,
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
        .map(|entry| {
            ui_list::action(
                entry.name.clone(),
                entry.description.clone(),
                Some(entry.badge()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_PICK_PREFIX, entry.name))),
            )
            .with_search_value(format!("{} {}", entry.name, entry.description))
        })
        .collect();

    items.push(
        ui_list::action(
            "Back",
            "Cancel and return".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(SKILL_PICK_BACK_ACTION.to_string())),
        )
        .with_search_value("back cancel".to_string()),
    );

    let selected = entries
        .first()
        .map(|entry| InlineListSelection::ConfigAction(format!("{}{}", SKILL_PICK_PREFIX, entry.name)));

    ctx.renderer.show_list_modal(
        title,
        vec![description.to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("skill name or description".to_string()),
            fuzzy: false,
        }),
    );
}

fn show_skill_actions_modal(ctx: &mut SlashCommandContext<'_>, entry: &InteractiveSkillEntry) {
    let mut items = Vec::new();
    if entry.is_loadable() && entry.loaded {
        items.push(
            ui_list::action(
                "Disable for this session",
                "Unload this skill from the active session".to_string(),
                None,
                Tone::Neutral,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_DISABLE_PREFIX, entry.name))),
            )
            .with_search_value("disable unload session".to_string()),
        );
    } else if entry.is_loadable() {
        items.push(
            ui_list::action(
                "Enable for this session",
                "Load this skill into the active session".to_string(),
                Some("Recommended".to_string()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_ENABLE_PREFIX, entry.name))),
            )
            .with_search_value("enable load session".to_string()),
        );
    }

    items.push(
        ui_list::action(
            "View details",
            "Show full skill metadata and instructions".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_INFO_PREFIX, entry.name))),
        )
        .with_search_value("details info metadata".to_string()),
    );

    items.push(
        ui_list::action(
            "Run skill",
            "Execute this skill with optional input".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_USE_PREFIX, entry.name))),
        )
        .with_search_value("run execute use".to_string()),
    );

    if entry.supports_validation() {
        items.push(
            ui_list::action(
                "Validate",
                "Validate this skill structure".to_string(),
                None,
                Tone::Neutral,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_VALIDATE_PREFIX, entry.name))),
            )
            .with_search_value("validate".to_string()),
        );
    }

    if entry.supports_packaging() {
        items.push(
            ui_list::action(
                "Package",
                "Package this skill to .skill".to_string(),
                None,
                Tone::Neutral,
                Some(InlineListSelection::ConfigAction(format!("{}{}", SKILL_PACKAGE_PREFIX, entry.name))),
            )
            .with_search_value("package bundle".to_string()),
        );
    }

    items.push(
        ui_list::action(
            "Back",
            "Return to skills list".to_string(),
            None,
            Tone::Neutral,
            Some(InlineListSelection::ConfigAction(SKILL_BACK_ACTION.to_string())),
        )
        .with_search_value("back return".to_string()),
    );

    let default_selection = items.first().and_then(|item| item.selection.clone());
    let title = format!("Skill: {}", entry.name);
    ctx.renderer
        .show_list_modal(&title, vec![entry.description.clone()], items, default_selection, None);
}

mod outcome;

#[cfg(test)]
mod tests;
