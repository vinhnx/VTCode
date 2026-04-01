use anyhow::{Result, anyhow, bail};
use serde_yaml::{Mapping as YamlMapping, Value as YamlValue};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
use vtcode_config::load_subagent_from_file;
use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
use vtcode_core::config::PermissionMode;
use vtcode_core::constants::tools;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::app::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, WizardModalMode, WizardStep,
};

use super::super::ui::{ensure_selection_ui_available, wait_for_list_modal_selection};
use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::model_picker::{SubagentModelSelection, pick_subagent_model};
use crate::agent::runloop::slash_commands::AgentDefinitionScope;
use crate::agent::runloop::unified::session_setup::refresh_local_agents;
use crate::agent::runloop::unified::wizard_modal::{
    WizardModalOutcome, show_wizard_modal_and_wait,
};

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
    "permissionMode",
    "permission_mode",
    "background",
    "maxTurns",
    "max_turns",
    "memory",
];

struct ToolOption {
    id: &'static str,
    description: &'static str,
    badge: &'static str,
}

const TOOL_OPTIONS: &[ToolOption] = &[
    ToolOption {
        id: tools::READ_FILE,
        description: "Read individual files directly.",
        badge: "Read",
    },
    ToolOption {
        id: tools::LIST_FILES,
        description: "List and discover files within the workspace.",
        badge: "Read",
    },
    ToolOption {
        id: tools::UNIFIED_SEARCH,
        description: "Search code and file contents.",
        badge: "Read",
    },
    ToolOption {
        id: tools::UNIFIED_EXEC,
        description: "Run shell commands and scripts.",
        badge: "Exec",
    },
    ToolOption {
        id: tools::UNIFIED_FILE,
        description: "Use the umbrella file-operations tool.",
        badge: "Write",
    },
    ToolOption {
        id: tools::EDIT_FILE,
        description: "Apply focused edits to existing files.",
        badge: "Write",
    },
    ToolOption {
        id: tools::WRITE_FILE,
        description: "Write whole-file contents.",
        badge: "Write",
    },
    ToolOption {
        id: tools::CREATE_FILE,
        description: "Create a new file at a specific path.",
        badge: "Write",
    },
    ToolOption {
        id: tools::DELETE_FILE,
        description: "Delete a file from the workspace.",
        badge: "Write",
    },
    ToolOption {
        id: tools::MOVE_FILE,
        description: "Move or rename files.",
        badge: "Write",
    },
    ToolOption {
        id: tools::COPY_FILE,
        description: "Copy files within the workspace.",
        badge: "Write",
    },
    ToolOption {
        id: tools::APPLY_PATCH,
        description: "Apply patch-based code changes.",
        badge: "Write",
    },
    ToolOption {
        id: tools::SEARCH_REPLACE,
        description: "Run focused search-and-replace edits.",
        badge: "Write",
    },
    ToolOption {
        id: tools::REQUEST_USER_INPUT,
        description: "Ask the user for targeted clarification.",
        badge: "HITL",
    },
    ToolOption {
        id: tools::SEARCH_TOOLS,
        description: "Search the tool catalog for available capabilities.",
        badge: "Meta",
    },
    ToolOption {
        id: tools::WEB_SEARCH,
        description: "Search the web when up-to-date information is required.",
        badge: "Web",
    },
    ToolOption {
        id: tools::FETCH_URL,
        description: "Fetch and inspect a specific URL.",
        badge: "Web",
    },
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
    permission_mode: Option<PermissionMode>,
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
    fn new(
        workspace_root: PathBuf,
        scope_hint: Option<AgentDefinitionScope>,
        name_hint: Option<&str>,
    ) -> Self {
        Self {
            workspace_root,
            scope: scope_hint.unwrap_or(AgentDefinitionScope::Project),
            name: name_hint.unwrap_or_default().trim().to_string(),
            description: super::DEFAULT_AGENT_DESCRIPTION_TEXT.to_string(),
            tools: default_agent_tools(),
            model: "inherit".to_string(),
            color: Some("blue".to_string()),
            reasoning_effort: Some("medium".to_string()),
            permission_mode: None,
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
        let mut extra_frontmatter = match serde_yaml::from_str::<YamlValue>(frontmatter)? {
            YamlValue::Mapping(mapping) => mapping,
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
            reasoning_effort: spec.reasoning_effort.clone(),
            permission_mode: spec.permission_mode,
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
        self.model = normalized_optional_string(Some(self.model.as_str()))
            .unwrap_or_else(|| "inherit".to_string());
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
        if let Some(color) = self
            .color
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
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
        if let Some(permission_mode) = self.permission_mode {
            insert_yaml_string(
                &mut frontmatter,
                "permissionMode",
                permission_mode_label(permission_mode),
            );
        }
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

        let mut yaml = serde_yaml::to_string(&frontmatter)?;
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
                "Interactive agent authoring requires inline UI. Use `/agents create project <name>` or `/agents create user <name>` when inline UI is unavailable.",
            )?;
            return Ok(SlashCommandControl::Continue);
        };
        let Some(name) = name else {
            ctx.renderer.line(
                MessageStyle::Info,
                "Provide an agent name when inline UI is unavailable.",
            )?;
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
                if let Some(SubagentModelSelection {
                    model,
                    reasoning_effort,
                }) = pick_subagent_model(
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
            "agents:author:permission" => {
                if let Some(permission_mode) =
                    prompt_permission_mode(ctx, draft.permission_mode).await?
                {
                    draft.permission_mode = permission_mode;
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

fn show_authoring_menu(
    ctx: &mut SlashCommandContext<'_>,
    draft: &NativeAgentDraft,
    creating: bool,
) -> Result<()> {
    let path = draft.current_path().ok();
    let title = if creating {
        "Create native VT Code agent".to_string()
    } else {
        format!("Edit native agent `{}`", draft.name)
    };
    let mut lines = vec![
        "Guided editing updates structured VT Code frontmatter only.".to_string(),
        "Use `Save + open prompt` if you want to refine the longer prompt body in your editor."
            .to_string(),
    ];
    if let Some(path) = path.as_ref() {
        lines.push(if creating {
            format!("Target file: {}", path.display())
        } else {
            format!("File: {}", path.display())
        });
    }

    let mut items = Vec::new();
    if creating {
        items.push(author_action_item(
            "Scope",
            &format!("Write to {}", scope_label(draft.scope)),
            Some("Required"),
            "scope",
        ));
        items.push(author_action_item(
            "Name",
            draft.name.as_str(),
            Some("Required"),
            "name",
        ));
    }
    items.push(author_action_item(
        "Description",
        draft.description.as_str(),
        Some("Required"),
        "description",
    ));
    items.push(author_action_item(
        "Tools",
        tools_summary(&draft.tools).as_str(),
        Some("Checklist"),
        "tools",
    ));
    items.push(author_action_item(
        "Model + Reasoning",
        model_summary(draft).as_str(),
        Some("Picker"),
        "model",
    ));
    items.push(author_action_item(
        "Permission Mode",
        permission_summary(draft.permission_mode).as_str(),
        None,
        "permission",
    ));
    items.push(author_action_item(
        "Color",
        draft.color.as_deref().unwrap_or("unset"),
        None,
        "color",
    ));
    items.push(author_action_item(
        "Background",
        if draft.background {
            "Enabled"
        } else {
            "Disabled"
        },
        None,
        "background",
    ));
    items.push(author_action_item(
        "Max Turns",
        max_turns_summary(draft.max_turns).as_str(),
        None,
        "max-turns",
    ));
    items.push(author_action_item(
        "Memory",
        memory_summary(draft.memory.as_ref()).as_str(),
        None,
        "memory",
    ));
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
            label: "Search fields".to_string(),
            placeholder: Some("name, tools, model, memory".to_string()),
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
    if creating && path.exists() {
        bail!("Agent file already exists at {}", path.display());
    }

    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| anyhow!("Invalid agent destination {}", path.display()))?,
    )?;
    fs::write(&path, draft.render_markdown()?)?;
    draft.file_path = Some(path.clone());

    if let Some(controller) = ctx.tool_registry.subagent_controller() {
        let _ = controller.reload().await;
        refresh_local_agents(ctx.handle, &controller).await?;
        super::refresh_agent_palette(ctx.handle, controller.as_ref()).await;
    }

    let action = if creating { "Created" } else { "Updated" };
    ctx.renderer.line(
        MessageStyle::Info,
        &format!("{} native agent definition at {}.", action, path.display()),
    )?;

    match save_behavior {
        SaveBehavior::SaveOnly => Ok(SlashCommandControl::Continue),
        SaveBehavior::SaveAndOpenEditor => {
            super::super::apps::handle_launch_editor(
                ctx.reborrow(),
                Some(path.display().to_string()),
            )
            .await
        }
    }
}

async fn prompt_scope(
    ctx: &mut SlashCommandContext<'_>,
    current: AgentDefinitionScope,
) -> Result<Option<AgentDefinitionScope>> {
    let items = vec![
        InlineListItem {
            title: "Project scope".to_string(),
            subtitle: Some("Write to `.vtcode/agents/<name>.md` in this workspace.".to_string()),
            badge: Some("Recommended".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "agents:author:scope:project".to_string(),
            )),
            search_value: Some("project workspace .vtcode agents".to_string()),
        },
        InlineListItem {
            title: "User scope".to_string(),
            subtitle: Some("Write to `~/.vtcode/agents/<name>.md` for all workspaces.".to_string()),
            badge: Some("User".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "agents:author:scope:user".to_string(),
            )),
            search_value: Some("user home shared agent".to_string()),
        },
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

async fn prompt_permission_mode(
    ctx: &mut SlashCommandContext<'_>,
    current: Option<PermissionMode>,
) -> Result<Option<Option<PermissionMode>>> {
    let items = vec![
        permission_item("No override", "Inherit the parent session mode.", None),
        permission_item(
            "Default",
            "Prompt when policy requires it.",
            Some(PermissionMode::Default),
        ),
        permission_item(
            "Plan",
            "Read-only planning mode.",
            Some(PermissionMode::Plan),
        ),
        permission_item(
            "Accept edits",
            "Auto-allow built-in file mutations for the child.",
            Some(PermissionMode::AcceptEdits),
        ),
        permission_item(
            "Don't ask",
            "Deny actions that are not explicitly allowed.",
            Some(PermissionMode::DontAsk),
        ),
        permission_item(
            "Auto",
            "Classifier-backed autonomous mode.",
            Some(PermissionMode::Auto),
        ),
        permission_item(
            "Bypass permissions",
            "Skip prompts except protected writes and sandbox escalation.",
            Some(PermissionMode::BypassPermissions),
        ),
    ];
    let selected = Some(InlineListSelection::ConfigAction(permission_action_key(
        current,
    )));
    ctx.handle.show_list_modal(
        "Permission mode".to_string(),
        vec!["Choose the permission mode override for this subagent.".to_string()],
        items,
        selected,
        None,
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    Ok(match selection {
        InlineListSelection::ConfigAction(action) => permission_mode_from_action(&action),
        _ => None,
    })
}

async fn prompt_background_mode(
    ctx: &mut SlashCommandContext<'_>,
    current: bool,
) -> Result<Option<bool>> {
    let items = vec![
        InlineListItem {
            title: "Disabled".to_string(),
            subtitle: Some("This agent does not default to background execution.".to_string()),
            badge: Some("Default".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "agents:author:background:false".to_string(),
            )),
            search_value: Some("background disabled".to_string()),
        },
        InlineListItem {
            title: "Enabled".to_string(),
            subtitle: Some("Mark this agent as background-capable by default.".to_string()),
            badge: Some("Background".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                "agents:author:background:true".to_string(),
            )),
            search_value: Some("background enabled".to_string()),
        },
    ];
    let selected = Some(InlineListSelection::ConfigAction(format!(
        "agents:author:background:{}",
        current
    )));
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
        InlineListSelection::ConfigAction(action) if action == "agents:author:background:false" => {
            Some(false)
        }
        InlineListSelection::ConfigAction(action) if action == "agents:author:background:true" => {
            Some(true)
        }
        _ => None,
    })
}

async fn prompt_memory_scope(
    ctx: &mut SlashCommandContext<'_>,
    current: Option<&SubagentMemoryScope>,
) -> Result<Option<Option<SubagentMemoryScope>>> {
    let items = vec![
        memory_item("No memory", "Do not set a persistent memory scope.", None),
        memory_item(
            "Project memory",
            "Use `.vtcode/agent-memory/<agent-name>/`.",
            Some(SubagentMemoryScope::Project),
        ),
        memory_item(
            "Local memory",
            "Use `.vtcode/agent-memory-local/<agent-name>/`.",
            Some(SubagentMemoryScope::Local),
        ),
        memory_item(
            "User memory",
            "Use `~/.vtcode/agent-memory/<agent-name>/`.",
            Some(SubagentMemoryScope::User),
        ),
    ];
    let selected = Some(InlineListSelection::ConfigAction(memory_action_key(
        current,
    )));
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

async fn prompt_max_turns(
    ctx: &mut SlashCommandContext<'_>,
    current: Option<usize>,
) -> Result<Option<Option<usize>>> {
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
                .map_err(|_| anyhow!("Max turns must be a positive integer"))?;
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
            items.push(InlineListItem {
                title: format!("Keep current ({})", current_trimmed),
                subtitle: Some("Leave this field unchanged.".to_string()),
                badge: Some("Current".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    KEEP_CURRENT_ACTION.to_string(),
                )),
                search_value: Some(format!("keep {}", current_trimmed)),
            });
        }
        if allow_clear {
            items.push(InlineListItem {
                title: "Clear value".to_string(),
                subtitle: Some("Remove the current override.".to_string()),
                badge: Some("Unset".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    CLEAR_VALUE_ACTION.to_string(),
                )),
                search_value: Some("clear unset remove".to_string()),
            });
        }
        items.push(InlineListItem {
            title: "Enter a value".to_string(),
            subtitle: Some("Press Tab to type inline, then Enter to submit.".to_string()),
            badge: Some("Input".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::RequestUserInputAnswer {
                question_id: FIELD_PROMPT_ID.to_string(),
                selected: vec![],
                other: Some(String::new()),
            }),
            search_value: Some("custom value input".to_string()),
        });

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
            InlineListSelection::RequestUserInputAnswer {
                other, selected, ..
            } => {
                let value = other
                    .or_else(|| selected.first().cloned())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if required && value.is_empty() {
                    ctx.renderer
                        .line(MessageStyle::Error, "A value is required for this field.")?;
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
        label: "Search tools".to_string(),
        placeholder: Some("tool id or capability".to_string()),
    });

    loop {
        let catalog = tool_catalog(current_tools, &selected);
        let mut items = catalog
            .iter()
            .map(|(tool_id, subtitle, badge)| InlineListItem {
                title: format!(
                    "[{}] {}",
                    if selected.contains(tool_id) { "x" } else { " " },
                    tool_id
                ),
                subtitle: Some(subtitle.clone()),
                badge: Some(badge.clone()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(format!(
                    "{TOOL_TOGGLE_PREFIX}{tool_id}"
                ))),
                search_value: Some(format!("{tool_id} {subtitle} {badge}")),
            })
            .collect::<Vec<_>>();
        items.push(InlineListItem {
            title: "Add custom tool id".to_string(),
            subtitle: Some(
                "Append an exact VT Code tool id that is not in the default list.".to_string(),
            ),
            badge: Some("Custom".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                TOOL_ADD_CUSTOM_ACTION.to_string(),
            )),
            search_value: Some("add custom tool id".to_string()),
        });
        items.push(InlineListItem {
            title: "Save selection".to_string(),
            subtitle: Some("Use the currently checked tool list.".to_string()),
            badge: Some("Action".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                TOOL_SAVE_ACTION.to_string(),
            )),
            search_value: Some("save tools".to_string()),
        });
        items.push(InlineListItem {
            title: "Cancel".to_string(),
            subtitle: Some("Keep the previous tool selection.".to_string()),
            badge: Some("Cancel".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                TOOL_CANCEL_ACTION.to_string(),
            )),
            search_value: Some("cancel".to_string()),
        });

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
            label: "Search tools".to_string(),
            placeholder: Some("tool id or capability".to_string()),
        });
    }
}

fn tool_catalog(
    initial_tools: &[String],
    selected: &BTreeSet<String>,
) -> Vec<(String, String, String)> {
    let extra_tools = initial_tools
        .iter()
        .chain(selected.iter())
        .filter(|tool_id| {
            TOOL_OPTIONS
                .iter()
                .all(|entry| entry.id != tool_id.as_str())
        })
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut catalog = TOOL_OPTIONS
        .iter()
        .map(|entry| {
            (
                entry.id.to_string(),
                entry.description.to_string(),
                entry.badge.to_string(),
            )
        })
        .collect::<Vec<_>>();
    catalog.extend(extra_tools.into_iter().map(|tool_id| {
        (
            tool_id,
            "Existing custom tool id.".to_string(),
            "Custom".to_string(),
        )
    }));
    catalog
}

async fn select_native_agent_name(ctx: &mut SlashCommandContext<'_>) -> Result<Option<String>> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    let specs = controller
        .effective_specs()
        .await
        .into_iter()
        .filter(is_native_vtcode_spec)
        .collect::<Vec<_>>();
    if specs.is_empty() {
        ctx.renderer.line(
            MessageStyle::Info,
            "No native `.vtcode` agent definitions are currently loaded.",
        )?;
        return Ok(None);
    }

    let items = specs
        .iter()
        .map(|spec| InlineListItem {
            title: spec.name.clone(),
            subtitle: Some(super::agent_subtitle(spec, false)),
            badge: Some(super::agent_badge(spec)),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                AUTHOR_ACTION_PREFIX, spec.name
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
        "Edit native VT Code agent".to_string(),
        vec!["Select a native `.vtcode` agent definition to edit interactively.".to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: "Search native agents".to_string(),
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
        .strip_prefix(AUTHOR_ACTION_PREFIX)
        .map(ToString::to_string))
}

async fn resolve_named_custom_agent(
    ctx: &SlashCommandContext<'_>,
    name: &str,
) -> Result<SubagentSpec> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.matches_name(name))
        .ok_or_else(|| anyhow!("Unknown agent {}", name))
}

fn is_native_vtcode_spec(spec: &SubagentSpec) -> bool {
    matches!(
        spec.source,
        SubagentSource::ProjectVtcode | SubagentSource::UserVtcode
    ) && spec.file_path.is_some()
}

fn scope_from_source(source: &SubagentSource) -> Result<AgentDefinitionScope> {
    match source {
        SubagentSource::ProjectVtcode => Ok(AgentDefinitionScope::Project),
        SubagentSource::UserVtcode => Ok(AgentDefinitionScope::User),
        _ => bail!("Only native `.vtcode` agents are editable in the guided flow"),
    }
}

fn strip_editable_keys(mapping: &mut YamlMapping) {
    for key in EDITABLE_FRONTMATTER_KEYS {
        mapping.remove(YamlValue::String((*key).to_string()));
    }
}

fn split_markdown_frontmatter(contents: &str) -> Result<(&str, &str)> {
    let Some((frontmatter, body)) = split_frontmatter(contents) else {
        bail!("native agent markdown is missing YAML frontmatter");
    };
    Ok((frontmatter, body))
}

fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let mut lines = contents.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end() != "---" {
        return None;
    }

    let mut offset = first.len();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            let body_start = offset + line.len();
            let frontmatter = &contents[first.len()..offset];
            let body = contents.get(body_start..).unwrap_or_default();
            return Some((frontmatter, body));
        }
        offset += line.len();
    }
    None
}

fn default_agent_tools() -> Vec<String> {
    ordered_tools(
        super::DEFAULT_AGENT_TOOL_IDS
            .iter()
            .map(|tool| (*tool).to_string())
            .collect(),
    )
}

fn ordered_tools(tools: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let known_order = TOOL_OPTIONS
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id, index))
        .collect::<std::collections::HashMap<_, _>>();

    let mut deduped = tools
        .into_iter()
        .filter(|tool_id| seen.insert(tool_id.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    deduped.sort_by(|left, right| {
        let left_key = known_order
            .get(left.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        let right_key = known_order
            .get(right.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        left_key
            .cmp(&right_key)
            .then_with(|| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()))
    });
    deduped
}

fn workspace_agent_path(workspace_root: &Path, name: &str) -> PathBuf {
    workspace_root
        .join(".vtcode/agents")
        .join(format!("{name}.md"))
}

fn user_agent_path(name: &str) -> Result<PathBuf> {
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow!("Cannot resolve home directory for user-scope agent"))?
        .join(".vtcode/agents")
        .join(format!("{name}.md")))
}

fn insert_yaml_string(mapping: &mut YamlMapping, key: &str, value: &str) {
    mapping.insert(
        YamlValue::String(key.to_string()),
        YamlValue::String(value.to_string()),
    );
}

fn insert_yaml_bool(mapping: &mut YamlMapping, key: &str, value: bool) {
    mapping.insert(YamlValue::String(key.to_string()), YamlValue::Bool(value));
}

fn insert_yaml_u64(mapping: &mut YamlMapping, key: &str, value: u64) {
    mapping.insert(
        YamlValue::String(key.to_string()),
        YamlValue::Number(value.into()),
    );
}

fn insert_yaml_string_list(mapping: &mut YamlMapping, key: &str, values: &[String]) {
    mapping.insert(
        YamlValue::String(key.to_string()),
        YamlValue::Sequence(
            values
                .iter()
                .map(|value| YamlValue::String(value.clone()))
                .collect(),
        ),
    );
}

fn scope_label(scope: AgentDefinitionScope) -> &'static str {
    match scope {
        AgentDefinitionScope::Project => ".vtcode/agents/<name>.md",
        AgentDefinitionScope::User => "~/.vtcode/agents/<name>.md",
    }
}

fn permission_mode_label(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Default => "default",
        PermissionMode::AcceptEdits => "acceptEdits",
        PermissionMode::Auto => "auto",
        PermissionMode::Plan => "plan",
        PermissionMode::DontAsk => "dontAsk",
        PermissionMode::BypassPermissions => "bypassPermissions",
    }
}

fn permission_summary(mode: Option<PermissionMode>) -> String {
    mode.map(permission_mode_label)
        .unwrap_or("inherit")
        .to_string()
}

fn permission_item(title: &str, subtitle: &str, mode: Option<PermissionMode>) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(permission_action_key(
            mode,
        ))),
        search_value: Some(format!("{} {}", title, subtitle)),
    }
}

fn permission_action_key(mode: Option<PermissionMode>) -> String {
    match mode {
        None => "agents:author:permission:unset".to_string(),
        Some(mode) => format!("agents:author:permission:{}", permission_mode_label(mode)),
    }
}

fn permission_mode_from_action(action: &str) -> Option<Option<PermissionMode>> {
    match action.strip_prefix("agents:author:permission:") {
        Some("unset") => Some(None),
        Some("default") => Some(Some(PermissionMode::Default)),
        Some("acceptEdits") => Some(Some(PermissionMode::AcceptEdits)),
        Some("auto") => Some(Some(PermissionMode::Auto)),
        Some("plan") => Some(Some(PermissionMode::Plan)),
        Some("dontAsk") => Some(Some(PermissionMode::DontAsk)),
        Some("bypassPermissions") => Some(Some(PermissionMode::BypassPermissions)),
        _ => None,
    }
}

fn memory_scope_label(scope: &SubagentMemoryScope) -> &'static str {
    match scope {
        SubagentMemoryScope::User => "user",
        SubagentMemoryScope::Project => "project",
        SubagentMemoryScope::Local => "local",
    }
}

fn memory_summary(scope: Option<&SubagentMemoryScope>) -> String {
    scope.map(memory_scope_label).unwrap_or("unset").to_string()
}

fn memory_item(title: &str, subtitle: &str, scope: Option<SubagentMemoryScope>) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(memory_action_key(
            scope.as_ref(),
        ))),
        search_value: Some(format!("{} {}", title, subtitle)),
    }
}

fn memory_action_key(scope: Option<&SubagentMemoryScope>) -> String {
    match scope {
        None => "agents:author:memory:unset".to_string(),
        Some(scope) => format!("agents:author:memory:{}", memory_scope_label(scope)),
    }
}

fn memory_scope_from_action(action: &str) -> Option<Option<SubagentMemoryScope>> {
    match action.strip_prefix("agents:author:memory:") {
        Some("unset") => Some(None),
        Some("user") => Some(Some(SubagentMemoryScope::User)),
        Some("project") => Some(Some(SubagentMemoryScope::Project)),
        Some("local") => Some(Some(SubagentMemoryScope::Local)),
        _ => None,
    }
}

fn normalized_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn author_action_item(
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    action: &str,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(ToString::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!(
            "{AUTHOR_ACTION_PREFIX}{action}"
        ))),
        search_value: Some(format!("{} {}", title, subtitle)),
    }
}

fn tools_summary(tools: &[String]) -> String {
    if tools.is_empty() {
        "No explicit allowlist".to_string()
    } else {
        tools.join(", ")
    }
}

fn model_summary(draft: &NativeAgentDraft) -> String {
    let reasoning = draft
        .reasoning_effort
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unset");
    format!("{} | reasoning {}", draft.model, reasoning)
}

fn max_turns_summary(max_turns: Option<usize>) -> String {
    max_turns
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unset".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn native_agent_render_preserves_advanced_frontmatter_keys() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("reviewer.md");
        fs::write(
            &path,
            r#"---
name: reviewer
description: Review code
tools:
  - read_file
  - list_files
  - unified_search
model: inherit
color: blue
reasoning_effort: medium
permissionMode: plan
background: true
maxTurns: 7
memory: project
skills:
  - rust-skills
nickname_candidates:
  - rev
---

Review the target changes."#,
        )
        .expect("write file");

        let spec =
            load_subagent_from_file(&path, SubagentSource::ProjectVtcode).expect("load subagent");
        let content = fs::read_to_string(&path).expect("read file");
        let mut draft =
            NativeAgentDraft::from_spec(temp.path().to_path_buf(), &spec, &content).expect("draft");
        draft.description = "Updated description".to_string();
        let rendered = draft.render_markdown().expect("rendered markdown");

        assert!(rendered.contains("description: Updated description"));
        assert!(rendered.contains("skills:"));
        assert!(rendered.contains("nickname_candidates:"));
        assert!(rendered.ends_with("\nReview the target changes."));
    }

    #[test]
    fn ordered_tools_keeps_known_tools_stable_and_sorts_custom_values() {
        let ordered = ordered_tools(vec![
            "zzz".to_string(),
            tools::UNIFIED_SEARCH.to_string(),
            tools::READ_FILE.to_string(),
            "aaa".to_string(),
        ]);

        assert_eq!(
            ordered,
            vec![
                tools::READ_FILE.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
                "aaa".to_string(),
                "zzz".to_string(),
            ]
        );
    }

    #[test]
    fn create_defaults_use_expected_frontmatter_and_prompt_body() {
        let workspace = TempDir::new().expect("temp dir");
        let draft = NativeAgentDraft::new(
            workspace.path().to_path_buf(),
            Some(AgentDefinitionScope::Project),
            Some("reviewer"),
        );

        assert_eq!(draft.model, "inherit");
        assert_eq!(draft.color.as_deref(), Some("blue"));
        assert_eq!(draft.reasoning_effort.as_deref(), Some("medium"));
        assert!(!draft.background);
        assert_eq!(draft.tools, default_agent_tools());
        assert_eq!(
            draft.current_path().expect("path"),
            workspace.path().join(".vtcode/agents/reviewer.md")
        );

        let rendered = draft.render_markdown().expect("render");
        assert!(rendered.contains("name: reviewer\n"));
        assert!(rendered.contains("background: false\n"));
        assert!(rendered.contains("\nYou are a focused VT Code subagent.\n"));
    }

    #[test]
    fn normalize_for_save_trims_and_defaults_fields() {
        let mut draft = NativeAgentDraft::new(
            PathBuf::from("/tmp/workspace"),
            Some(AgentDefinitionScope::Project),
            Some(" reviewer "),
        );
        draft.description = "  Review auth changes  ".to_string();
        draft.model = "   ".to_string();
        draft.color = Some(" teal ".to_string());
        draft.reasoning_effort = Some(" medium ".to_string());
        draft.tools = vec![
            "zzz".to_string(),
            tools::READ_FILE.to_string(),
            tools::READ_FILE.to_string(),
        ];

        draft.normalize_for_save();

        assert_eq!(draft.name, "reviewer");
        assert_eq!(draft.description, "Review auth changes");
        assert_eq!(draft.model, "inherit");
        assert_eq!(draft.color.as_deref(), Some("teal"));
        assert_eq!(draft.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(draft.tools.len(), 2);
    }

    #[test]
    fn render_markdown_keeps_prompt_body_exact_and_canonical_order() {
        let mut draft = NativeAgentDraft::new(
            PathBuf::from("/tmp/workspace"),
            Some(AgentDefinitionScope::Project),
            Some("reviewer"),
        );
        draft.description = "Review auth changes".to_string();
        draft.tools = vec![
            tools::READ_FILE.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
        ];
        draft.color = Some("teal".to_string());
        draft.reasoning_effort = Some("high".to_string());
        draft.permission_mode = Some(PermissionMode::Plan);
        draft.background = true;
        draft.max_turns = Some(5);
        draft.memory = Some(SubagentMemoryScope::Project);
        draft.prompt_body = "\nPrompt body\n  with indentation\n".to_string();
        draft.extra_frontmatter.insert(
            YamlValue::String("skills".to_string()),
            YamlValue::Sequence(vec![YamlValue::String("rust-skills".to_string())]),
        );

        let rendered = draft.render_markdown().expect("render");
        let model_index = rendered.find("model: inherit").expect("model key");
        let color_index = rendered.find("color: teal").expect("color key");
        let reasoning_index = rendered
            .find("reasoning_effort: high")
            .expect("reasoning key");
        let permission_index = rendered
            .find("permissionMode: plan")
            .expect("permission key");
        let background_index = rendered.find("background: true").expect("background key");
        let max_turns_index = rendered.find("maxTurns: 5").expect("maxTurns key");
        let memory_index = rendered.find("memory: project").expect("memory key");
        let skills_index = rendered.find("skills:").expect("skills key");

        assert!(model_index < color_index);
        assert!(color_index < reasoning_index);
        assert!(reasoning_index < permission_index);
        assert!(permission_index < background_index);
        assert!(background_index < max_turns_index);
        assert!(max_turns_index < memory_index);
        assert!(memory_index < skills_index);
        assert!(rendered.ends_with("\nPrompt body\n  with indentation\n"));
    }

    #[test]
    fn is_native_vtcode_spec_filters_to_vtcode_markdown_agents() {
        let base_spec = SubagentSpec {
            name: "reviewer".to_string(),
            description: "desc".to_string(),
            prompt: String::new(),
            tools: Some(default_agent_tools()),
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            color: Some("blue".to_string()),
            reasoning_effort: Some("medium".to_string()),
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: Some(PathBuf::from(".vtcode/agents/reviewer.md")),
            warnings: Vec::new(),
        };
        assert!(is_native_vtcode_spec(&base_spec));

        let imported_claude = SubagentSpec {
            source: SubagentSource::ProjectClaude,
            ..base_spec.clone()
        };
        assert!(!is_native_vtcode_spec(&imported_claude));

        let imported_codex = SubagentSpec {
            source: SubagentSource::UserCodex,
            ..base_spec.clone()
        };
        assert!(!is_native_vtcode_spec(&imported_codex));

        let missing_path = SubagentSpec {
            file_path: None,
            ..base_spec
        };
        assert!(!is_native_vtcode_spec(&missing_path));
    }
}
