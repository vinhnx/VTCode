use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use anyhow::{Result, anyhow, bail};
use serde_json::Value as YamlValue;
type YamlMapping = serde_json::Map<String, YamlValue>;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use vtcode_commons::VtCodePaths;
use vtcode_commons::fs::write_private_file_atomic;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
#[cfg(test)]
use vtcode_config::load_subagent_from_file;
use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
use vtcode_core::constants::tools;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_ui::tui::app::{InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep};

use super::super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::model_picker::{SubagentModelSelection, pick_subagent_model};
use crate::agent::runloop::slash_commands::AgentDefinitionScope;
use crate::agent::runloop::unified::session_setup::refresh_local_agents;
use crate::agent::runloop::unified::wizard_modal::{WizardModalOutcome, show_wizard_modal_and_wait};

const AUTHOR_ACTION_PREFIX: &str = "agents:author:";
const FIELD_PROMPT_ID: &str = "agent-field";
const KEEP_CURRENT_ACTION: &str = "agents:author:keep";
const CLEAR_VALUE_ACTION: &str = "agents:author:clear";
const TOOL_TOGGLE_PREFIX: &str = "agents:author:tool:toggle:";
const TOOL_SAVE_ACTION: &str = "agents:author:tool:save";
const TOOL_ADD_CUSTOM_ACTION: &str = "agents:author:tool:add-custom";
const TOOL_CANCEL_ACTION: &str = "agents:author:tool:cancel";
const EDITABLE_FRONTMATTER_KEYS: &[&str] = &[
    "name",
    "description",
    "tools",
    "allowed_tools",
    "enabled_tools",
    "model",
    "color",
    "badgeColor",
    "badge_color",
    "reasoning_effort",
    "model_reasoning_effort",
    "effort",
    "background",
    "maxTurns",
    "max_turns",
    "memory",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct NativeAgentDraft {
    workspace_root: PathBuf,
    scope: AgentDefinitionScope,
    name: String,
    description: String,
    tools: Vec<String>,
    model: String,
    color: Option<String>,
    reasoning_effort: Option<String>,
    permissions: AgentPermissionsConfig,
    background: bool,
    max_turns: Option<usize>,
    memory: Option<SubagentMemoryScope>,
    prompt_body: String,
    extra_frontmatter: YamlMapping,
    file_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveBehavior {
    SaveOnly,
    SaveAndOpenEditor,
}

enum PromptTextResult {
    KeepCurrent,
    Clear,
    Value(String),
}

impl NativeAgentDraft {
    fn new(workspace_root: PathBuf, scope_hint: Option<AgentDefinitionScope>, name_hint: Option<&str>) -> Self {
        Self {
            workspace_root,
            scope: scope_hint.unwrap_or(AgentDefinitionScope::Project),
            name: name_hint.unwrap_or_default().trim().to_string(),
            description: super::DEFAULT_AGENT_DESCRIPTION_TEXT.to_string(),
            tools: default_agent_tools(),
            model: "inherit".to_string(),
            color: Some("blue".to_string()),
            reasoning_effort: Some("medium".to_string()),
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            background: false,
            max_turns: None,
            memory: None,
            prompt_body: super::DEFAULT_AGENT_BODY_TEXT.to_string(),
            extra_frontmatter: YamlMapping::new(),
            file_path: None,
        }
    }

    fn from_spec(workspace_root: PathBuf, spec: &SubagentSpec, content: &str) -> Result<Self> {
        let scope = scope_from_source(&spec.source)?;
        let (frontmatter, prompt_body) = split_markdown_frontmatter(content)?;
        let mut extra_frontmatter = match serde_saphyr::from_str::<YamlValue>(frontmatter)? {
            YamlValue::Object(mapping) => mapping,
            _ => bail!("native agent frontmatter must be a YAML mapping"),
        };
        strip_editable_keys(&mut extra_frontmatter);

        Ok(Self {
            workspace_root,
            scope,
            name: spec.name.clone(),
            description: spec.description.clone(),
            tools: ordered_tools(spec.tools.clone().unwrap_or_default()),
            model: spec.model.clone().unwrap_or_else(|| "inherit".to_string()),
            color: spec.color.clone(),
            reasoning_effort: spec.reasoning_effort.map(|e| e.as_str().to_string()),
            permissions: spec.permissions.clone(),
            background: spec.background,
            max_turns: spec.max_turns,
            memory: spec.memory,
            prompt_body: prompt_body.to_string(),
            extra_frontmatter,
            file_path: spec.file_path.clone(),
        })
    }

    fn current_path(&self) -> Result<PathBuf> {
        if let Some(path) = self.file_path.as_ref() {
            return Ok(path.clone());
        }

        super::validate_agent_name(&self.name)?;
        Ok(match self.scope {
            AgentDefinitionScope::Project => workspace_agent_path(&self.workspace_root, &self.name),
            AgentDefinitionScope::User => user_agent_path(&self.name)?,
        })
    }

    fn normalize_for_save(&mut self) {
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        self.model = normalized_optional_string(Some(self.model.as_str())).unwrap_or_else(|| "inherit".to_string());
        self.color = normalized_optional_string(self.color.as_deref());
        self.reasoning_effort = normalized_optional_string(self.reasoning_effort.as_deref());
        self.tools = ordered_tools(std::mem::take(&mut self.tools));
    }

    fn render_markdown(&self) -> Result<String> {
        let mut frontmatter = YamlMapping::new();
        insert_yaml_string(&mut frontmatter, "name", self.name.as_str());
        insert_yaml_string(&mut frontmatter, "description", self.description.as_str());
        insert_yaml_string_list(&mut frontmatter, "tools", &self.tools);
        insert_yaml_string(&mut frontmatter, "model", self.model.as_str());
        if let Some(color) = self.color.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            insert_yaml_string(&mut frontmatter, "color", color);
        }
        if let Some(reasoning_effort) = self
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            insert_yaml_string(&mut frontmatter, "reasoning_effort", reasoning_effort);
        }
        insert_yaml_permissions(&mut frontmatter, &self.permissions);
        insert_yaml_bool(&mut frontmatter, "background", self.background);
        if let Some(max_turns) = self.max_turns {
            insert_yaml_u64(&mut frontmatter, "maxTurns", max_turns as u64);
        }
        if let Some(memory) = self.memory.as_ref() {
            insert_yaml_string(&mut frontmatter, "memory", memory_scope_label(memory));
        }

        for (key, value) in &self.extra_frontmatter {
            if !frontmatter.contains_key(key) {
                frontmatter.insert(key.clone(), value.clone());
            }
        }

        let mut yaml = serde_saphyr::to_string(&frontmatter)?;
        if !yaml.ends_with('\n') {
            yaml.push('\n');
        }

        Ok(format!("---\n{}---\n{}", yaml, self.prompt_body))
    }
}

pub(super) async fn handle_create_agent(
    mut ctx: SlashCommandContext<'_>,
    scope: Option<AgentDefinitionScope>,
    name: Option<&str>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        let Some(scope) = scope else {
            ctx.renderer.line(
                MessageStyle::Info,
                "Interactive agent authoring requires inline UI. Use `/agent create project <name>` or `/agent create user <name>` when inline UI is unavailable.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };
        let Some(name) = name else {
            ctx.renderer
                .line(MessageStyle::Info, "Provide an agent name when inline UI is unavailable.")?;
            return Ok(SlashCommandControl::Continue);
        };
        return super::legacy_create_agent_scaffold(&mut ctx, scope, name).await;
    }

    if !ensure_selection_ui_available(&mut ctx, "opening guided agent authoring")? {
        return Ok(SlashCommandControl::Continue);
    }

    let mut draft = NativeAgentDraft::new(ctx.config.workspace.clone(), scope, name);
    run_authoring_flow(&mut ctx, &mut draft, true).await
}

pub(super) async fn handle_edit_agent(
    mut ctx: SlashCommandContext<'_>,
    name: Option<&str>,
) -> Result<SlashCommandControl> {
    if !ctx.renderer.supports_inline_ui() {
        let Some(name) = name else {
            ctx.renderer.line(
                MessageStyle::Info,
                "Interactive agent editing requires inline UI. Provide an agent name to open its file directly.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };
        return super::legacy_open_agent_editor(ctx, name).await;
    }

    if !ensure_selection_ui_available(&mut ctx, "opening guided agent editing")? {
        return Ok(SlashCommandControl::Continue);
    }

    let spec = match name {
        Some(name) => resolve_named_custom_agent(&ctx, name).await?,
        None => {
            let Some(name) = select_native_agent_name(&mut ctx).await? else {
                return Ok(SlashCommandControl::Continue);
            };
            resolve_named_custom_agent(&ctx, &name).await?
        }
    };

    if !is_native_vtcode_spec(&spec) {
        let Some(name) = name.or(Some(spec.name.as_str())) else {
            return Ok(SlashCommandControl::Continue);
        };
        return super::legacy_open_agent_editor(ctx, name).await;
    }

    let path = spec
        .file_path
        .as_ref()
        .ok_or_else(|| anyhow!("Native VT Code agent is missing a file path"))?;
    let content = fs::read_to_string(path)?;
    let mut draft = NativeAgentDraft::from_spec(ctx.config.workspace.clone(), &spec, &content)?;
    run_authoring_flow(&mut ctx, &mut draft, false).await
}

async fn run_authoring_flow(
    ctx: &mut SlashCommandContext<'_>,
    draft: &mut NativeAgentDraft,
    creating: bool,
) -> Result<SlashCommandControl> {
    loop {
        let Some(save_behavior) = edit_native_agent_draft(ctx, draft, creating).await? else {
            return Ok(SlashCommandControl::Continue);
        };

        match save_native_agent(ctx, draft, creating, save_behavior).await {
            Ok(control) => return Ok(control),
            Err(err) => {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("Failed to save agent: {err}"))?;
            }
        }
    }
}

async fn edit_native_agent_draft(
    ctx: &mut SlashCommandContext<'_>,
    draft: &mut NativeAgentDraft,
    creating: bool,
) -> Result<Option<SaveBehavior>> {
    loop {
        show_authoring_menu(ctx, draft, creating)?;
        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(None);
        };

        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };

        match action.as_str() {
            "agents:author:scope" if creating => {
                if let Some(scope) = prompt_scope(ctx, draft.scope).await? {
                    draft.scope = scope;
                }
            }
            "agents:author:name" if creating => {
                if let Some(result) = prompt_text_value(
                    ctx,
                    "Agent name",
                    "Enter a lowercase hyphenated agent name.",
                    draft.name.as_str(),
                    "example-agent",
                    true,
                    false,
                )
                .await?
                {
                    match result {
                        PromptTextResult::KeepCurrent => {}
                        PromptTextResult::Clear => {}
                        PromptTextResult::Value(value) => {
                            super::validate_agent_name(&value)?;
                            draft.name = value;
                        }
                    }
                }
            }
            "agents:author:description" => {
                if let Some(result) = prompt_text_value(
                    ctx,
                    "Description",
                    "Describe when VT Code should delegate to this agent.",
                    draft.description.as_str(),
                    "Read-only reviewer for auth changes",
                    true,
                    false,
                )
                .await?
                {
                    match result {
                        PromptTextResult::KeepCurrent => {}
                        PromptTextResult::Clear => {}
                        PromptTextResult::Value(value) => {
                            draft.description = value;
                        }
                    }
                }
            }
            "agents:author:tools" => {
                if let Some(tools) = edit_tools_checklist(ctx, &draft.tools).await? {
                    draft.tools = tools;
                }
            }
            "agents:author:model" => {
                if let Some(SubagentModelSelection { model, reasoning_effort }) = pick_subagent_model(
                    ctx.renderer,
                    ctx.handle,
                    ctx.session,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    ctx.vt_cfg.as_ref(),
                    Some(ctx.config.workspace.as_path()),
                    draft.model.as_str(),
                    draft.reasoning_effort.as_deref(),
                )
                .await?
                {
                    draft.model = model;
                    draft.reasoning_effort = reasoning_effort;
                }
            }
            "agents:author:color" => {
                let current = draft.color.as_deref().unwrap_or_default();
                if let Some(result) = prompt_text_value(
                    ctx,
                    "Color",
                    "Set the TUI badge color for this agent.",
                    current,
                    "blue or #4f8fd8",
                    false,
                    true,
                )
                .await?
                {
                    match result {
                        PromptTextResult::KeepCurrent => {}
                        PromptTextResult::Clear => draft.color = None,
                        PromptTextResult::Value(value) => draft.color = Some(value),
                    }
                }
            }
            "agents:author:background" => {
                if let Some(background) = prompt_background_mode(ctx, draft.background).await? {
                    draft.background = background;
                }
            }
            "agents:author:max-turns" => {
                if let Some(max_turns) = prompt_max_turns(ctx, draft.max_turns).await? {
                    draft.max_turns = max_turns;
                }
            }
            "agents:author:memory" => {
                if let Some(memory) = prompt_memory_scope(ctx, draft.memory.as_ref()).await? {
                    draft.memory = memory;
                }
            }
            "agents:author:save" => return Ok(Some(SaveBehavior::SaveOnly)),
            "agents:author:save-edit" => return Ok(Some(SaveBehavior::SaveAndOpenEditor)),
            "agents:author:cancel" => return Ok(None),
            _ => {}
        }
    }
}

fn show_authoring_menu(ctx: &mut SlashCommandContext<'_>, draft: &NativeAgentDraft, creating: bool) -> Result<()> {
    let path = draft.current_path().ok();
    let title = if creating {
        "Create native VT Code agent".to_string()
    } else {
        format!("Edit native agent `{}`", draft.name)
    };
    let mut lines = vec![
        "Guided editing updates structured VT Code frontmatter only.".to_string(),
        "Use `Save + open prompt` if you want to refine the longer prompt body in your editor.".to_string(),
    ];
    if let Some(path) = path.as_ref() {
        lines.push(if creating {
            format!("Target file: {}", path.display())
        } else {
            format!("File: {}", path.display())
        });
    }
    lines.push(format!(
        "Agent availability: {}; permission rules default decision: {}.",
        availability_summary(draft),
        permission_default_label(draft.permissions.default)
    ));

    let mut items = Vec::new();
    if creating {
        items.push(author_action_item(
            "Scope",
            &format!("Write to {}", scope_label(draft.scope)),
            Some("Required"),
            "scope",
        ));
        items.push(author_action_item("Name", draft.name.as_str(), Some("Required"), "name"));
    }
    items.push(author_action_item("Description", draft.description.as_str(), Some("Required"), "description"));
    items.push(author_action_item("Tools", tools_summary(&draft.tools).as_str(), Some("Checklist"), "tools"));
    items.push(author_action_item("Model + Reasoning", model_summary(draft).as_str(), Some("Picker"), "model"));
    items.push(author_action_item("Color", draft.color.as_deref().unwrap_or("unset"), None, "color"));
    items.push(author_action_item(
        "Background",
        if draft.background { "Enabled" } else { "Disabled" },
        None,
        "background",
    ));
    items.push(author_action_item("Max Turns", max_turns_summary(draft.max_turns).as_str(), None, "max-turns"));
    items.push(author_action_item("Memory", memory_summary(draft.memory.as_ref()).as_str(), None, "memory"));
    items.push(author_action_item(
        "Save",
        "Write the agent file without opening an editor.",
        Some("Action"),
        "save",
    ));
    items.push(author_action_item(
        "Save + open prompt",
        "Write the file and open it in your external editor for prompt-body edits.",
        Some("Action"),
        "save-edit",
    ));
    items.push(author_action_item(
        "Cancel",
        "Close guided authoring without changing the file.",
        Some("Cancel"),
        "cancel",
    ));

    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        title,
        lines,
        items,
        selected,
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("name, tools, model, memory".to_string()),
            fuzzy: false,
        }),
    );
    Ok(())
}

async fn save_native_agent(
    ctx: &mut SlashCommandContext<'_>,
    draft: &mut NativeAgentDraft,
    creating: bool,
    save_behavior: SaveBehavior,
) -> Result<SlashCommandControl> {
    draft.normalize_for_save();
    super::validate_agent_name(&draft.name)?;
    if draft.description.trim().is_empty() {
        bail!("Agent description cannot be empty");
    }

    let path = draft.current_path()?;
    if creating {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to use symlinked agent file {}", path.display());
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Agent path is not a regular file: {}", path.display());
            }
            Ok(_) => bail!("Agent file already exists at {}", path.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }

    VtCodePaths::ensure_user_dir(
        path.parent()
            .ok_or_else(|| anyhow!("Invalid agent destination {}", path.display()))?,
    )?;
    let contents = draft.render_markdown()?;
    if draft.scope == AgentDefinitionScope::User {
        write_private_file_atomic(&path, contents.as_bytes()).await?;
    } else {
        fs::write(&path, contents)?;
    }
    draft.file_path = Some(path.clone());

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_local_agents(ctx.handle, Some(&controller), ctx.tool_registry.exec_session_manager()).await?;
        super::refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }

    let action = if creating { "Created" } else { "Updated" };
    ctx.renderer
        .line(MessageStyle::Info, &format!("{} native agent definition at {}.", action, path.display()))?;

    match save_behavior {
        SaveBehavior::SaveOnly => Ok(SlashCommandControl::Continue),
        SaveBehavior::SaveAndOpenEditor => {
            super::super::apps::handle_launch_editor(ctx.reborrow(), Some(path.display().to_string())).await
        }
    }
}

async fn prompt_scope(
    ctx: &mut SlashCommandContext<'_>,
    current: AgentDefinitionScope,
) -> Result<Option<AgentDefinitionScope>> {
    let items = vec![
        ui_list::action(
            "Project scope",
            "Write to `.vtcode/agents/<name>.md` in this workspace.".to_string(),
            Some("Recommended".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction("agents:author:scope:project".to_string())),
        )
        .with_search_value("project workspace .vtcode agents".to_string()),
        ui_list::action(
            "User scope",
            "Write to the canonical user config directory for all workspaces.".to_string(),
            Some("User".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction("agents:author:scope:user".to_string())),
        )
        .with_search_value("user home shared agent".to_string()),
    ];
    let selected = Some(InlineListSelection::ConfigAction(match current {
        AgentDefinitionScope::Project => "agents:author:scope:project".to_string(),
        AgentDefinitionScope::User => "agents:author:scope:user".to_string(),
    }));
    ctx.handle.show_list_modal(
        "Agent scope".to_string(),
        vec!["Choose where VT Code should save the native agent definition.".to_string()],
        items,
        selected,
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    Ok(match selection {
        InlineListSelection::ConfigAction(action) if action == "agents:author:scope:project" => {
            Some(AgentDefinitionScope::Project)
        }
        InlineListSelection::ConfigAction(action) if action == "agents:author:scope:user" => {
            Some(AgentDefinitionScope::User)
        }
        _ => None,
    })
}

async fn prompt_background_mode(ctx: &mut SlashCommandContext<'_>, current: bool) -> Result<Option<bool>> {
    let items = vec![
        ui_list::action(
            "Disabled",
            "This agent does not default to background execution.".to_string(),
            Some("Default".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction("agents:author:background:false".to_string())),
        )
        .with_search_value("background disabled".to_string()),
        ui_list::action(
            "Enabled",
            "Mark this agent as background-capable by default.".to_string(),
            Some("Background".to_string()),
            Tone::Accent,
            Some(InlineListSelection::ConfigAction("agents:author:background:true".to_string())),
        )
        .with_search_value("background enabled".to_string()),
    ];
    let selected = Some(InlineListSelection::ConfigAction(format!("agents:author:background:{current}")));
    ctx.handle.show_list_modal(
        "Background mode".to_string(),
        vec!["Choose whether this agent should default to background execution.".to_string()],
        items,
        selected,
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    Ok(match selection {
        InlineListSelection::ConfigAction(action) if action == "agents:author:background:false" => Some(false),
        InlineListSelection::ConfigAction(action) if action == "agents:author:background:true" => Some(true),
        _ => None,
    })
}

async fn prompt_memory_scope(
    ctx: &mut SlashCommandContext<'_>,
    current: Option<&SubagentMemoryScope>,
) -> Result<Option<Option<SubagentMemoryScope>>> {
    let items = vec![
        memory_item("No memory", "Do not set a persistent memory scope.", None),
        memory_item("Project memory", "Use `.vtcode/agent-memory/<agent-name>/`.", Some(SubagentMemoryScope::Project)),
        memory_item(
            "Local memory",
            "Use `.vtcode/agent-memory-local/<agent-name>/`.",
            Some(SubagentMemoryScope::Local),
        ),
        memory_item(
            "User memory",
            "Use the user state directory's `agent-memory/<agent-name>/` path.",
            Some(SubagentMemoryScope::User),
        ),
    ];
    let selected = Some(InlineListSelection::ConfigAction(memory_action_key(current)));
    ctx.handle.show_list_modal(
        "Memory scope".to_string(),
        vec!["Choose the persistent memory scope for this agent.".to_string()],
        items,
        selected,
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    Ok(match selection {
        InlineListSelection::ConfigAction(action) => memory_scope_from_action(&action),
        _ => None,
    })
}

async fn prompt_max_turns(ctx: &mut SlashCommandContext<'_>, current: Option<usize>) -> Result<Option<Option<usize>>> {
    let current_value = current.map(|value| value.to_string()).unwrap_or_default();
    let Some(result) = prompt_text_value(
        ctx,
        "Max turns",
        "Set the maximum number of turns for this agent, or leave it unset.",
        current_value.as_str(),
        "6",
        false,
        true,
    )
    .await?
    else {
        return Ok(None);
    };

    Ok(Some(match result {
        PromptTextResult::KeepCurrent => current,
        PromptTextResult::Clear => None,
        PromptTextResult::Value(value) => {
            let parsed = value
                .parse::<usize>()
                .map_err(|e| anyhow!("Max turns must be a positive integer: {e}"))?;
            if parsed == 0 {
                bail!("Max turns must be a positive integer");
            }
            Some(parsed)
        }
    }))
}

async fn prompt_text_value(
    ctx: &mut SlashCommandContext<'_>,
    title: &str,
    question: &str,
    current: &str,
    placeholder: &str,
    required: bool,
    allow_clear: bool,
) -> Result<Option<PromptTextResult>> {
    loop {
        let mut items = Vec::new();
        let current_trimmed = current.trim();
        if !current_trimmed.is_empty() {
            items.push(
                ui_list::current_choice(
                    format!("Keep current ({current_trimmed})"),
                    Some("Leave this field unchanged.".to_string()),
                    Some(InlineListSelection::ConfigAction(KEEP_CURRENT_ACTION.to_string())),
                )
                .with_search_value(format!("keep {current_trimmed}")),
            );
        }
        if allow_clear {
            items.push(
                ui_list::action(
                    "Clear value",
                    "Remove the current override.".to_string(),
                    Some("Unset".to_string()),
                    Tone::Accent,
                    Some(InlineListSelection::ConfigAction(CLEAR_VALUE_ACTION.to_string())),
                )
                .with_search_value("clear unset remove".to_string()),
            );
        }
        items.push(
            ui_list::choice(
                "Enter a value",
                Some("Press Tab to type inline, then Enter to submit.".to_string()),
                Some(InlineListSelection::RequestUserInputAnswer {
                    question_id: FIELD_PROMPT_ID.to_string(),
                    selected: vec![],
                    other: Some(String::new()),
                }),
            )
            .with_search_value("custom value input".to_string()),
        );

        let outcome = show_wizard_modal_and_wait(
            ctx.handle,
            ctx.session,
            title.to_string(),
            vec![WizardStep {
                title: title.to_string(),
                question: question.to_string(),
                items,
                completed: false,
                answer: None,
                allow_freeform: true,
                freeform_label: Some("Value".to_string()),
                freeform_placeholder: Some(placeholder.to_string()),
                freeform_default: None,
            }],
            0,
            None,
            WizardModalMode::MultiStep,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
        )
        .await?;

        let Some(selection) = (match outcome {
            WizardModalOutcome::Submitted(selections) => selections.into_iter().next(),
            WizardModalOutcome::Cancelled { .. } => None,
        }) else {
            return Ok(None);
        };

        match selection {
            InlineListSelection::ConfigAction(action) if action == KEEP_CURRENT_ACTION => {
                return Ok(Some(PromptTextResult::KeepCurrent));
            }
            InlineListSelection::ConfigAction(action) if action == CLEAR_VALUE_ACTION => {
                return Ok(Some(PromptTextResult::Clear));
            }
            InlineListSelection::RequestUserInputAnswer { other, selected, .. } => {
                let value = other
                    .or_else(|| selected.first().cloned())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if required && value.is_empty() {
                    ctx.renderer.line(MessageStyle::Error, "A value is required for this field.")?;
                    continue;
                }
                return Ok(Some(PromptTextResult::Value(value)));
            }
            _ => {}
        }
    }
}

async fn edit_tools_checklist(
    ctx: &mut SlashCommandContext<'_>,
    current_tools: &[String],
) -> Result<Option<Vec<String>>> {
    let mut selected = current_tools.iter().cloned().collect::<BTreeSet<_>>();
    let mut search_config = Some(InlineListSearchConfig {
        label: String::new(),
        placeholder: Some("tool id or capability".to_string()),
        fuzzy: false,
    });

    loop {
        let catalog = tool_catalog(current_tools, &selected);
        let mut items = catalog
            .iter()
            .map(|(tool_id, subtitle, badge)| InlineListItem {
                title: format!("[{}] {}", if selected.contains(tool_id) { "x" } else { " " }, tool_id),
                subtitle: Some(subtitle.clone()),
                badge: Some(badge.clone()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(format!("{TOOL_TOGGLE_PREFIX}{tool_id}"))),
                search_value: Some(format!("{tool_id} {subtitle} {badge}")),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        items.push(
            ui_list::action(
                "Add custom tool id",
                "Append an exact VT Code tool id that is not in the default list.".to_string(),
                Some("Custom".to_string()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(TOOL_ADD_CUSTOM_ACTION.to_string())),
            )
            .with_search_value("add custom tool id".to_string()),
        );
        items.push(
            ui_list::action(
                "Save selection",
                "Use the currently checked tool list.".to_string(),
                Some("Action".to_string()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(TOOL_SAVE_ACTION.to_string())),
            )
            .with_search_value("save tools".to_string()),
        );
        items.push(
            ui_list::action(
                "Cancel",
                "Keep the previous tool selection.".to_string(),
                Some("Cancel".to_string()),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(TOOL_CANCEL_ACTION.to_string())),
            )
            .with_search_value("cancel".to_string()),
        );

        let selected_item = items.first().and_then(|item| item.selection.clone());
        ctx.handle.show_list_modal(
            "Agent tools".to_string(),
            vec![
                format!("{} tool(s) selected.", selected.len()),
                "Enter toggles the selected tool. Use `Save selection` when finished.".to_string(),
            ],
            items,
            selected_item,
            search_config.take(),
        );

        let Some(selection) = wait_for_list_modal_selection(ctx).await else {
            return Ok(None);
        };
        let InlineListSelection::ConfigAction(action) = selection else {
            continue;
        };
        if action == TOOL_SAVE_ACTION {
            return Ok(Some(ordered_tools(selected.into_iter().collect())));
        }
        if action == TOOL_CANCEL_ACTION {
            return Ok(None);
        }
        if action == TOOL_ADD_CUSTOM_ACTION {
            let current = "";
            if let Some(PromptTextResult::Value(value)) = prompt_text_value(
                ctx,
                "Custom tool id",
                "Enter an exact VT Code tool id to add to the allowlist.",
                current,
                "request_user_input",
                true,
                false,
            )
            .await?
            {
                selected.insert(value);
            }
            continue;
        }
        if let Some(tool_id) = action.strip_prefix(TOOL_TOGGLE_PREFIX)
            && !selected.remove(tool_id)
        {
            selected.insert(tool_id.to_string());
        }
        search_config = Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("tool id or capability".to_string()),
            fuzzy: false,
        });
    }
}

mod support;

use support::{
    author_action_item, availability_summary, default_agent_tools, insert_yaml_bool, insert_yaml_permissions,
    insert_yaml_string, insert_yaml_string_list, insert_yaml_u64, is_native_vtcode_spec, max_turns_summary,
    memory_action_key, memory_item, memory_scope_from_action, memory_scope_label, memory_summary, model_summary,
    normalized_optional_string, ordered_tools, permission_default_label, resolve_named_custom_agent, scope_from_source,
    scope_label, select_native_agent_name, split_markdown_frontmatter, strip_editable_keys, tool_catalog,
    tools_summary, user_agent_path, workspace_agent_path,
};

#[cfg(test)]
mod tests;
