use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::constants::tools;
use crate::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use crate::hooks::{HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig};

const BUILTIN_DEFAULT_AGENT: &str = r#"You are the default VT Code execution subagent.

Work directly, keep context isolated from the parent session, and return concise summaries.
Match the repository's local patterns, verify changes, and avoid unrelated edits.
Never speculate about code you have not read. If a file is referenced, read it before answering.
Only make changes that are directly requested or clearly necessary. Keep solutions simple and focused.
Do not add features, refactor code, or make improvements beyond what was asked.
Verify your work by running the smallest relevant check before reporting completion."#;

const BUILTIN_EXPLORER_AGENT: &str = r#"You are a fast read-only exploration subagent.

Search the codebase, inspect relevant files, and return concise findings with file references.
Do not modify files or take mutating actions.
Read files before making claims about their contents. Never speculate about code you have not opened.
Use structural search and grep over shell exploration when possible.
When reading multiple files, read them all in parallel for efficiency.
Return findings with file paths and line numbers for easy navigation."#;

const BUILTIN_WORKER_AGENT: &str = r#"You are a write-capable worker subagent.

Handle bounded implementation work, verify results, and return a concise outcome summary with
any important risks or follow-up items.
Read files before editing them. Never speculate about code you have not read.
Only make changes that are directly requested. Keep solutions simple and focused.
Do not add features, refactor surrounding code, or make improvements beyond the scope.
Verify your changes by running relevant tests or checks before reporting completion.
If calls repeat without progress, re-plan instead of retrying identically."#;

const BUILTIN_BUILD_PRIMARY_AGENT: &str = r#"You are the build agent.

Understand the user's request, inspect relevant project context, make directly requested changes,
and verify them with the narrowest useful checks.
Keep changes focused. Do not add unrelated features or refactors.
When planning is needed, state the plan briefly before implementation. When the user only wants
discussion or review, do not edit files.
Report changed files, validation, and remaining risks clearly."#;

const BUILTIN_AUTO_PRIMARY_AGENT: &str = r#"You are the auto agent.

Work autonomously within the active permission policy, taking direct action when the request is clear.
Inspect the relevant repository context before editing, keep changes focused, and verify with the
narrowest useful checks before reporting completion.
Pause for user input when the scope is ambiguous, risky, or outside the requested work."#;

const BUILTIN_PLAN_AGENT: &str = r#"You are a read-only planning agent.

Gather the minimum repository context needed to support a plan or design decision.
Return findings, risks, and constraints clearly; do not modify files.
Read relevant files before making claims about the codebase. Never speculate.
Use structural search to find patterns across the repository.
When reading multiple files, read them all in parallel for efficiency.
Ground your recommendations in specific code references and file paths."#;

const BUILTIN_DUCK_PRIMARY_AGENT: &str = r#"You are the duck agent.

Be discussion-first. Help the user clarify scope, constraints, contradictions, and options before implementation.
Do not edit files, you are for rubber-ducking only.
If the user asks for edits, suggest pressing Tab to switch to the Build agent for implementation."#;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentSource {
    Cli,
    ProjectVtcode,
    ProjectClaude,
    ProjectCodex,
    UserVtcode,
    UserClaude,
    UserCodex,
    Plugin { plugin: String },
    Builtin,
}

impl SubagentSource {
    #[must_use]
    pub const fn priority(&self) -> usize {
        match self {
            Self::Cli => 0,
            Self::ProjectVtcode => 1,
            Self::ProjectClaude => 2,
            Self::ProjectCodex => 3,
            Self::UserVtcode => 4,
            Self::UserClaude => 5,
            Self::UserCodex => 6,
            Self::Plugin { .. } => 7,
            Self::Builtin => 8,
        }
    }

    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Cli => "cli".to_string(),
            Self::ProjectVtcode => "project:.vtcode".to_string(),
            Self::ProjectClaude => "project:.claude".to_string(),
            Self::ProjectCodex => "project:.codex".to_string(),
            Self::UserVtcode => "user:~/.vtcode".to_string(),
            Self::UserClaude => "user:~/.claude".to_string(),
            Self::UserCodex => "user:~/.codex".to_string(),
            Self::Plugin { plugin } => format!("plugin:{plugin}"),
            Self::Builtin => "builtin".to_string(),
        }
    }

    #[must_use]
    pub const fn vtcode_native(&self) -> bool {
        matches!(self, Self::ProjectVtcode | Self::UserVtcode | Self::Cli)
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentMemoryScope {
    User,
    Project,
    Local,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Primary,
    #[default]
    Subagent,
    All,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum SubagentMcpServer {
    Named(String),
    Inline(BTreeMap<String, JsonValue>),
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentSpecFieldClass {
    Shared,
    PrimaryMetadata,
    PrimaryRuntime,
    SubagentOnly,
    Availability,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubagentSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    pub permissions: AgentPermissionsConfig,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<SubagentMcpServer>,
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub mode: AgentMode,
    #[serde(default)]
    pub max_turns: Option<usize>,
    #[serde(default)]
    pub nickname_candidates: Vec<String>,
    #[serde(default)]
    pub initial_prompt: Option<String>,
    #[serde(default)]
    pub memory: Option<SubagentMemoryScope>,
    #[serde(default)]
    pub isolation: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub source: SubagentSource,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl SubagentSpec {
    #[must_use]
    pub const fn is_primary(&self) -> bool {
        matches!(self.mode, AgentMode::Primary | AgentMode::All)
    }

    #[must_use]
    pub const fn is_subagent(&self) -> bool {
        matches!(self.mode, AgentMode::Subagent | AgentMode::All)
    }

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        if !self.permissions_allows_mutation() {
            return true;
        }

        let tools = self.tools.as_ref().map_or_else(Vec::new, Clone::clone);
        let lower_tools = tools
            .iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let lower_denied = self
            .disallowed_tools
            .iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect::<Vec<_>>();

        let denies_writes = lower_denied
            .iter()
            .any(|tool| is_mutating_tool_name(tool.as_str()));

        if self.tools.is_some() {
            let exposes_mutation = lower_tools
                .iter()
                .any(|tool| is_mutating_tool_name(tool.as_str()));
            !exposes_mutation
        } else {
            denies_writes
        }
    }

    #[must_use]
    pub fn permissions_allows_mutation(&self) -> bool {
        if matches!(
            self.permissions.default,
            PermissionDefault::Ask | PermissionDefault::Allow | PermissionDefault::Auto
        ) {
            return true;
        }

        self.permissions
            .allow
            .iter()
            .chain(self.permissions.auto.iter())
            .map(|rule| rule.to_ascii_lowercase())
            .any(|rule| is_mutating_tool_name(rule.as_str()))
    }

    #[must_use]
    pub fn matches_name(&self, candidate: &str) -> bool {
        self.name.eq_ignore_ascii_case(candidate)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(candidate))
    }
}

#[must_use]
pub fn classify_agent_spec_field(field: &str) -> Option<AgentSpecFieldClass> {
    match field.trim() {
        "name" | "prompt" => Some(AgentSpecFieldClass::Shared),
        "description" | "color" | "aliases" => Some(AgentSpecFieldClass::PrimaryMetadata),
        "tools" | "disallowed_tools" | "disallowedTools" | "permissions" | "model"
        | "reasoning_effort" | "skills" | "mcp_servers" | "mcpServers" | "hooks" | "memory" => {
            Some(AgentSpecFieldClass::PrimaryRuntime)
        }
        "background"
        | "max_turns"
        | "maxTurns"
        | "initial_prompt"
        | "initialPrompt"
        | "nickname_candidates"
        | "isolation" => Some(AgentSpecFieldClass::SubagentOnly),
        "mode" => Some(AgentSpecFieldClass::Availability),
        _ => None,
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct BackgroundSubagentConfig {
    #[serde(default = "default_background_subagents_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub default_agent: Option<String>,
    #[serde(default = "default_background_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
    #[serde(default = "default_background_auto_restore")]
    pub auto_restore: bool,
    #[serde(default = "default_background_toggle_shortcut")]
    pub toggle_shortcut: String,
}

impl Default for BackgroundSubagentConfig {
    fn default() -> Self {
        Self {
            enabled: default_background_subagents_enabled(),
            default_agent: None,
            refresh_interval_ms: default_background_refresh_interval_ms(),
            auto_restore: default_background_auto_restore(),
            toggle_shortcut: default_background_toggle_shortcut(),
        }
    }
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SubagentRuntimeLimits {
    #[serde(default = "default_subagents_enabled")]
    pub enabled: bool,
    #[serde(default = "default_subagents_max_concurrent")]
    pub max_concurrent: usize,
    #[serde(default = "default_subagents_max_depth")]
    pub max_depth: usize,
    #[serde(default = "default_subagents_default_timeout_seconds")]
    pub default_timeout_seconds: u64,
    #[serde(default = "default_subagents_auto_delegate_read_only")]
    pub auto_delegate_read_only: bool,
    #[serde(default)]
    pub background: BackgroundSubagentConfig,
}

impl Default for SubagentRuntimeLimits {
    fn default() -> Self {
        Self {
            enabled: default_subagents_enabled(),
            max_concurrent: default_subagents_max_concurrent(),
            max_depth: default_subagents_max_depth(),
            default_timeout_seconds: default_subagents_default_timeout_seconds(),
            auto_delegate_read_only: default_subagents_auto_delegate_read_only(),
            background: BackgroundSubagentConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredSubagents {
    pub effective: Vec<SubagentSpec>,
    pub shadowed: Vec<SubagentSpec>,
}

#[derive(Debug, Clone)]
pub struct SubagentDiscoveryInput {
    pub workspace_root: PathBuf,
    pub cli_agents: Option<JsonValue>,
    pub plugin_agent_files: Vec<(String, PathBuf)>,
    pub include_user_agents: bool,
}

impl SubagentDiscoveryInput {
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            cli_agents: None,
            plugin_agent_files: Vec::new(),
            include_user_agents: true,
        }
    }
}

impl Default for SubagentDiscoveryInput {
    fn default() -> Self {
        Self::new(PathBuf::new())
    }
}

pub fn discover_subagents(input: &SubagentDiscoveryInput) -> Result<DiscoveredSubagents> {
    let mut discovered = Vec::new();
    discovered.extend(builtin_subagents());

    if input.include_user_agents
        && let Some(home) = dirs::home_dir()
    {
        discovered.extend(load_subagents_from_dir(
            &home.join(".codex/agents"),
            SubagentSource::UserCodex,
        )?);
        discovered.extend(load_subagents_from_dir(
            &home.join(".claude/agents"),
            SubagentSource::UserClaude,
        )?);
        discovered.extend(load_subagents_from_dir(
            &home.join(".vtcode/agents"),
            SubagentSource::UserVtcode,
        )?);
    }

    discovered.extend(load_subagents_from_dir(
        &input.workspace_root.join(".codex/agents"),
        SubagentSource::ProjectCodex,
    )?);
    discovered.extend(load_subagents_from_dir(
        &input.workspace_root.join(".claude/agents"),
        SubagentSource::ProjectClaude,
    )?);
    discovered.extend(load_subagents_from_dir(
        &input.workspace_root.join(".vtcode/agents"),
        SubagentSource::ProjectVtcode,
    )?);

    for (plugin_name, path) in &input.plugin_agent_files {
        if !path.exists() || !path.is_file() {
            continue;
        }
        let source = SubagentSource::Plugin {
            plugin: plugin_name.clone(),
        };
        discovered.push(load_subagent_from_file(path, source)?);
    }

    if let Some(cli_agents) = input.cli_agents.as_ref() {
        discovered.extend(load_cli_agents(cli_agents)?);
    }

    discovered.sort_by_key(|spec| spec.source.priority());

    let mut effective_by_name: BTreeMap<String, SubagentSpec> = BTreeMap::new();
    let mut shadowed = Vec::new();
    for spec in discovered {
        let key = spec.name.clone();
        if let Some(existing) = effective_by_name.get(&key) {
            if should_replace(existing, &spec) {
                shadowed.push(existing.clone());
                effective_by_name.insert(key, spec);
            } else {
                shadowed.push(spec);
            }
        } else {
            effective_by_name.insert(key, spec);
        }
    }

    Ok(DiscoveredSubagents {
        effective: effective_by_name.into_values().collect(),
        shadowed,
    })
}

pub fn builtin_subagents() -> Vec<SubagentSpec> {
    vec![
        builtin_primary_build_agent(),
        builtin_primary_auto_agent(),
        builtin_primary_duck_agent(),
        builtin_plan_agent(),
        SubagentSpec {
            name: "default".to_string(),
            description: "Default inheriting subagent for general delegated work.".to_string(),
            prompt: BUILTIN_DEFAULT_AGENT.to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            color: Some("blue".to_string()),
            reasoning_effort: None,
            permissions: mutating_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["default".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        },
        SubagentSpec {
            name: "explorer".to_string(),
            description: "Read-only exploration specialist. Use proactively for code search, file discovery, and repository understanding.".to_string(),
            prompt: BUILTIN_EXPLORER_AGENT.to_string(),
            tools: Some(builtin_readonly_tool_ids()),
            disallowed_tools: builtin_readonly_disallowed_tool_ids(),
            model: Some("small".to_string()),
            color: Some("cyan".to_string()),
            reasoning_effort: Some("low".to_string()),
            permissions: readonly_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["explore".to_string(), "search".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: vec!["explore".to_string()],
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        },
        SubagentSpec {
            name: "worker".to_string(),
            description: "Write-capable execution subagent for bounded implementation or multi-step action.".to_string(),
            prompt: BUILTIN_WORKER_AGENT.to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            color: Some("magenta".to_string()),
            reasoning_effort: None,
            permissions: mutating_agent_permissions(),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::Subagent,
            max_turns: None,
            nickname_candidates: vec!["general".to_string(), "worker".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: vec!["general".to_string(), "general-purpose".to_string()],
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        },
    ]
}

pub fn builtin_primary_build_agent() -> SubagentSpec {
    SubagentSpec {
        name: "build".to_string(),
        description: "Built-in implementation agent for the main session.".to_string(),
        prompt: BUILTIN_BUILD_PRIMARY_AGENT.to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        model: Some("inherit".to_string()),
        color: Some("magenta".to_string()),
        reasoning_effort: None,
        permissions: mutating_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["build".to_string(), "builder".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["builder".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
    }
}

pub fn builtin_primary_auto_agent() -> SubagentSpec {
    SubagentSpec {
        name: "auto".to_string(),
        description: "Built-in autonomous implementation agent for the main session.".to_string(),
        prompt: BUILTIN_AUTO_PRIMARY_AGENT.to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        model: Some("inherit".to_string()),
        color: Some("green".to_string()),
        reasoning_effort: None,
        permissions: auto_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["auto".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["autonomous".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
    }
}

pub fn builtin_plan_agent() -> SubagentSpec {
    SubagentSpec {
        name: "plan".to_string(),
        description: "Built-in read-only planning agent definition.".to_string(),
        prompt: BUILTIN_PLAN_AGENT.to_string(),
        tools: Some(builtin_readonly_tool_ids()),
        disallowed_tools: builtin_readonly_disallowed_tool_ids(),
        model: Some("inherit".to_string()),
        color: Some("yellow".to_string()),
        reasoning_effort: Some("medium".to_string()),
        permissions: readonly_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::All,
        max_turns: None,
        nickname_candidates: vec!["plan".to_string(), "planner".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: vec!["planner".to_string()],
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
    }
}

pub fn builtin_primary_duck_agent() -> SubagentSpec {
    SubagentSpec {
        name: "duck".to_string(),
        description: "Built-in discussion-first agent for the main session.".to_string(),
        prompt: BUILTIN_DUCK_PRIMARY_AGENT.to_string(),
        tools: Some(builtin_readonly_tool_ids()),
        disallowed_tools: builtin_readonly_disallowed_tool_ids(),
        model: Some("inherit".to_string()),
        color: Some("cyan".to_string()),
        reasoning_effort: Some("low".to_string()),
        permissions: readonly_agent_permissions(),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: AgentMode::Primary,
        max_turns: None,
        nickname_candidates: vec!["duck".to_string()],
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
    }
}

fn builtin_readonly_tool_ids() -> Vec<String> {
    vec![
        tools::UNIFIED_SEARCH.to_string(),
        tools::UNIFIED_FILE.to_string(),
        tools::UNIFIED_EXEC.to_string(),
    ]
}

fn builtin_readonly_disallowed_tool_ids() -> Vec<String> {
    vec![tools::UNIFIED_FILE.to_string()]
}

fn mutating_agent_permissions() -> AgentPermissionsConfig {
    AgentPermissionsConfig::new(PermissionDefault::Ask)
}

fn auto_agent_permissions() -> AgentPermissionsConfig {
    AgentPermissionsConfig::new(PermissionDefault::Auto)
}

fn readonly_agent_permissions() -> AgentPermissionsConfig {
    let mut permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    permissions.allow = vec![
        tools::READ_FILE.to_string(),
        tools::LIST_FILES.to_string(),
        tools::UNIFIED_SEARCH.to_string(),
    ];
    permissions
}

fn should_replace(existing: &SubagentSpec, candidate: &SubagentSpec) -> bool {
    let existing_priority = existing.source.priority();
    let candidate_priority = candidate.source.priority();
    if candidate_priority != existing_priority {
        return candidate_priority < existing_priority;
    }

    candidate.source.vtcode_native() && !existing.source.vtcode_native()
}

fn load_subagents_from_dir(dir: &Path, source: SubagentSource) -> Result<Vec<SubagentSpec>> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(Vec::new());
    }

    let extension = match source {
        SubagentSource::ProjectCodex | SubagentSource::UserCodex => "toml",
        _ => "md",
    };
    let mut loaded = Vec::new();
    for entry in fs::read_dir(dir)
        .with_context(|| format!("failed to read subagent directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some(extension) {
            continue;
        }
        loaded.push(load_subagent_from_file(&path, source.clone())?);
    }

    Ok(loaded)
}

pub fn load_subagent_from_file(path: &Path, source: SubagentSource) -> Result<SubagentSpec> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut spec = match source {
        SubagentSource::ProjectCodex | SubagentSource::UserCodex => {
            parse_codex_toml_subagent(&content, source.clone())?
        }
        _ => parse_markdown_subagent(&content, source.clone())?,
    };
    spec.file_path = Some(path.to_path_buf());
    Ok(spec)
}

fn load_cli_agents(value: &JsonValue) -> Result<Vec<SubagentSpec>> {
    let Some(object) = value.as_object() else {
        bail!("CLI subagent payload must be a JSON object");
    };

    let mut specs = Vec::with_capacity(object.len());
    for (name, raw) in object {
        let Some(config) = raw.as_object() else {
            bail!("CLI subagent '{name}' must be an object");
        };
        let description = required_string(config, "description")
            .with_context(|| format!("CLI subagent '{name}' is missing description"))?;
        let prompt = config
            .get("prompt")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string();
        let tools = optional_string_list(config.get("tools"))?;
        let disallowed_tools =
            optional_string_list(config.get("disallowedTools"))?.unwrap_or_default();
        let model = config
            .get("model")
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let color = config
            .get("color")
            .or_else(|| config.get("badgeColor"))
            .or_else(|| config.get("badge_color"))
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let reasoning_effort = config
            .get("reasoning_effort")
            .or_else(|| config.get("model_reasoning_effort"))
            .or_else(|| config.get("effort"))
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let permissions = parse_required_permissions(config)?;
        let skills = optional_string_list(config.get("skills"))?.unwrap_or_default();
        let mcp_servers = optional_mcp_servers(
            config
                .get("mcpServers")
                .or_else(|| config.get("mcp_servers")),
        )?;
        let hooks = optional_hooks(config.get("hooks"))?;
        let max_turns = config
            .get("maxTurns")
            .or_else(|| config.get("max_turns"))
            .and_then(JsonValue::as_u64)
            .map(|value| value as usize);
        let background = config
            .get("background")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let mode = config
            .get("mode")
            .and_then(JsonValue::as_str)
            .map(parse_agent_mode)
            .transpose()?
            .unwrap_or_default();
        let nickname_candidates =
            optional_string_list(config.get("nickname_candidates"))?.unwrap_or_default();
        let initial_prompt = config
            .get("initialPrompt")
            .or_else(|| config.get("initial_prompt"))
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let memory = config
            .get("memory")
            .and_then(JsonValue::as_str)
            .map(parse_memory_scope)
            .transpose()?;
        let isolation = config
            .get("isolation")
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let aliases = optional_string_list(config.get("aliases"))?.unwrap_or_default();
        let warnings = primary_agent_subagent_only_field_warnings(config, mode);

        specs.push(SubagentSpec {
            name: name.clone(),
            description,
            prompt,
            tools,
            disallowed_tools,
            model,
            color,
            reasoning_effort,
            permissions,
            skills,
            mcp_servers,
            hooks,
            background,
            mode,
            max_turns,
            nickname_candidates,
            initial_prompt,
            memory,
            isolation,
            aliases,
            source: SubagentSource::Cli,
            file_path: None,
            warnings,
        });
    }

    Ok(specs)
}

fn parse_markdown_subagent(content: &str, source: SubagentSource) -> Result<SubagentSpec> {
    let trimmed = content.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        bail!("markdown subagent is missing YAML frontmatter");
    };
    let Some(end_idx) = rest.find("\n---") else {
        bail!("markdown subagent is missing closing frontmatter delimiter");
    };
    let frontmatter_text = rest[..end_idx].trim();
    let prompt_body = rest[end_idx + 4..].trim().to_string();
    let frontmatter = serde_saphyr::from_str::<JsonValue>(frontmatter_text)
        .context("failed to parse subagent YAML frontmatter")?;
    let Some(object) = frontmatter.as_object() else {
        bail!("subagent frontmatter must be a YAML mapping");
    };
    let prompt = if prompt_body.is_empty() {
        object
            .get("prompt")
            .and_then(JsonValue::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        prompt_body
    };

    let mut spec = subagent_spec_from_json_map(object, prompt, source.clone())?;
    if matches!(source, SubagentSource::Plugin { .. }) {
        apply_plugin_restrictions(&mut spec);
    }
    Ok(spec)
}

fn parse_codex_toml_subagent(content: &str, source: SubagentSource) -> Result<SubagentSpec> {
    let root = toml::from_str::<toml::Value>(content).context("failed to parse subagent TOML")?;
    let Some(table) = root.as_table() else {
        bail!("Codex subagent TOML must be a table");
    };
    let object = toml_table_to_json_object(table)?;
    let prompt = object
        .get("prompt")
        .or_else(|| object.get("developer_instructions"))
        .or_else(|| object.get("instructions"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();

    let spec = subagent_spec_from_json_map(&object, prompt, source)?;
    if spec.description.trim().is_empty() {
        bail!("Codex subagent TOML requires a description");
    }
    if spec.name.trim().is_empty() {
        bail!("Codex subagent TOML requires a name");
    }
    Ok(spec)
}

fn subagent_spec_from_json_map(
    object: &JsonMap<String, JsonValue>,
    prompt: String,
    source: SubagentSource,
) -> Result<SubagentSpec> {
    let name = required_string(object, "name")?;
    let description = required_string(object, "description")?;
    let tools = normalize_subagent_tool_list(optional_string_list(
        object
            .get("tools")
            .or_else(|| object.get("allowed_tools"))
            .or_else(|| object.get("enabled_tools")),
    )?);
    let disallowed_tools = normalize_subagent_tools(
        optional_string_list(
            object
                .get("disallowedTools")
                .or_else(|| object.get("disallowed_tools"))
                .or_else(|| object.get("disabled_tools")),
        )?
        .unwrap_or_default(),
    );
    let model = object
        .get("model")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let color = object
        .get("color")
        .or_else(|| object.get("badgeColor"))
        .or_else(|| object.get("badge_color"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let reasoning_effort = object
        .get("reasoning_effort")
        .or_else(|| object.get("model_reasoning_effort"))
        .or_else(|| object.get("effort"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let permissions = parse_required_permissions(object)?;
    let skills = optional_string_list(object.get("skills"))?.unwrap_or_default();
    let mcp_servers = optional_mcp_servers(
        object
            .get("mcpServers")
            .or_else(|| object.get("mcp_servers")),
    )?;
    let hooks = optional_hooks(object.get("hooks"))?;
    let background = object
        .get("background")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    let mode = object
        .get("mode")
        .and_then(JsonValue::as_str)
        .map(parse_agent_mode)
        .transpose()?
        .unwrap_or_default();
    let max_turns = object
        .get("maxTurns")
        .or_else(|| object.get("max_turns"))
        .and_then(JsonValue::as_u64)
        .map(|value| value as usize);
    let nickname_candidates =
        optional_string_list(object.get("nickname_candidates"))?.unwrap_or_default();
    let initial_prompt = object
        .get("initialPrompt")
        .or_else(|| object.get("initial_prompt"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let memory = object
        .get("memory")
        .and_then(JsonValue::as_str)
        .map(parse_memory_scope)
        .transpose()?;
    let isolation = object
        .get("isolation")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let aliases = optional_string_list(object.get("aliases"))?.unwrap_or_default();
    let warnings = primary_agent_subagent_only_field_warnings(object, mode);

    Ok(SubagentSpec {
        name,
        description,
        prompt,
        tools,
        disallowed_tools,
        model,
        color,
        reasoning_effort,
        permissions,
        skills,
        mcp_servers,
        hooks,
        background,
        mode,
        max_turns,
        nickname_candidates,
        initial_prompt,
        memory,
        isolation,
        aliases,
        source,
        file_path: None,
        warnings,
    })
}

fn primary_agent_subagent_only_field_warnings(
    object: &JsonMap<String, JsonValue>,
    mode: AgentMode,
) -> Vec<String> {
    if !matches!(mode, AgentMode::Primary | AgentMode::All) {
        return Vec::new();
    }

    [
        ("background", &["background"][..]),
        ("max_turns", &["max_turns", "maxTurns"][..]),
        ("initial_prompt", &["initial_prompt", "initialPrompt"][..]),
        ("nickname_candidates", &["nickname_candidates"][..]),
        ("isolation", &["isolation"][..]),
    ]
    .into_iter()
    .filter(|(_, aliases)| aliases.iter().any(|field| object.contains_key(*field)))
    .map(|(field, _)| {
        format!("field '{field}' is for subagents only and is ignored by primary agents")
    })
    .collect()
}

fn normalize_subagent_tool_list(tools: Option<Vec<String>>) -> Option<Vec<String>> {
    tools.map(normalize_subagent_tools)
}

fn normalize_subagent_tools(tools: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::with_capacity(tools.len());
    for tool in tools {
        let trimmed = tool.trim();
        let mapped_names = normalize_subagent_tool_name(trimmed);
        if mapped_names.is_empty() {
            if !trimmed.is_empty()
                && !normalized
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(trimmed))
            {
                normalized.push(trimmed.to_string());
            }
            continue;
        }

        for mapped in mapped_names {
            if !normalized.iter().any(|existing| existing == mapped) {
                normalized.push(mapped.to_string());
            }
        }
    }
    normalized
}

fn normalize_subagent_tool_name(tool: &str) -> &'static [&'static str] {
    match tool.trim().to_ascii_lowercase().as_str() {
        "read" => &[tools::READ_FILE],
        "write" => &[tools::WRITE_FILE],
        "edit" | "multiedit" | "multi_edit" | "multi-edit" => &[tools::EDIT_FILE],
        "grep" | "grep_file" | "grepfile" => &[tools::UNIFIED_SEARCH],
        "glob" | "list" | "list_files" | "listfiles" => &[tools::LIST_FILES],
        "bash" | "shell" | "command" => &[tools::UNIFIED_EXEC],
        "patch" | "applypatch" | "apply_patch" => &[tools::APPLY_PATCH],
        "agent" | "task" => &[tools::SPAWN_AGENT],
        "askuserquestion" | "ask_user_question" | "requestuserinput" | "request_user_input" => {
            &[tools::REQUEST_USER_INPUT]
        }
        _ => &[],
    }
}

fn is_mutating_tool_name(tool: &str) -> bool {
    tool == "edit"
        || tool == "bash"
        || tool == "shell"
        || tool == "command"
        || tool == "write"
        || tool == tools::UNIFIED_EXEC
        || tool == tools::EDIT_FILE
        || tool == tools::WRITE_FILE
        || tool == tools::UNIFIED_FILE
        || tool == tools::APPLY_PATCH
        || tool == tools::CREATE_FILE
        || tool == tools::DELETE_FILE
        || tool == tools::MOVE_FILE
        || tool == tools::COPY_FILE
        || tool == tools::SEARCH_REPLACE
}

fn permission_rule_allows_mutation(rule: &str) -> bool {
    let tool_name = permission_rule_tool_name(rule);
    let normalized = tool_name.to_ascii_lowercase();
    if is_mutating_tool_name(normalized.as_str()) {
        return true;
    }

    normalize_subagent_tool_name(tool_name)
        .iter()
        .any(|tool| is_mutating_tool_name(tool))
}

fn permission_rule_tool_name(rule: &str) -> &str {
    let trimmed = rule.trim();
    if let Some((tool_name, specifier)) = trimmed.split_once('(')
        && specifier.trim_end().ends_with(')')
    {
        return tool_name.trim();
    }
    trimmed
}

fn apply_plugin_restrictions(spec: &mut SubagentSpec) {
    if spec.hooks.take().is_some() {
        spec.warnings
            .push("plugin subagent hooks are ignored for safety".to_string());
    }
    if !spec.mcp_servers.is_empty() {
        spec.mcp_servers.clear();
        spec.warnings
            .push("plugin subagent mcp_servers are ignored for safety".to_string());
    }
    let default_restricted = matches!(
        spec.permissions.default,
        PermissionDefault::Allow | PermissionDefault::Auto
    );
    if default_restricted {
        spec.permissions.default = PermissionDefault::Ask;
    }

    let allow_len = spec.permissions.allow.len();
    spec.permissions
        .allow
        .retain(|rule| !permission_rule_allows_mutation(rule));
    let auto_len = spec.permissions.auto.len();
    spec.permissions
        .auto
        .retain(|rule| !permission_rule_allows_mutation(rule));

    if default_restricted
        || spec.permissions.allow.len() != allow_len
        || spec.permissions.auto.len() != auto_len
    {
        spec.warnings
            .push("plugin subagent permission overrides are restricted for safety".to_string());
    }
}

fn parse_required_permissions(
    object: &JsonMap<String, JsonValue>,
) -> Result<AgentPermissionsConfig> {
    if let Some(legacy_field) = object
        .keys()
        .find(|field| is_legacy_permission_field(field))
    {
        bail!("unsupported legacy subagent field '{legacy_field}'; use 'permissions.default'");
    }

    let Some(value) = object.get("permissions") else {
        return Ok(AgentPermissionsConfig::new(PermissionDefault::Ask));
    };

    serde_json::from_value::<AgentPermissionsConfig>(value.clone())
        .context("failed to parse subagent permissions")
}

fn is_legacy_permission_field(field: &str) -> bool {
    field
        .strip_prefix("permission")
        .is_some_and(|suffix| matches!(suffix, "Mode" | "_mode"))
}

fn required_string(object: &JsonMap<String, JsonValue>, key: &str) -> Result<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("missing required subagent field '{key}'"))
}

fn optional_string_list(value: Option<&JsonValue>) -> Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        JsonValue::Null => Ok(None),
        JsonValue::String(text) => Ok(Some(
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
        )),
        JsonValue::Array(items) => Ok(Some(
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToString::to_string)
                .collect(),
        )),
        JsonValue::Bool(enabled) => {
            if *enabled {
                Ok(Some(Vec::new()))
            } else {
                Ok(None)
            }
        }
        _ => bail!("expected string or string array for subagent list field"),
    }
}

fn optional_mcp_servers(value: Option<&JsonValue>) -> Result<Vec<SubagentMcpServer>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    match value {
        JsonValue::Null => Ok(Vec::new()),
        JsonValue::Array(entries) => entries
            .iter()
            .map(parse_mcp_server_value)
            .collect::<Result<Vec<_>>>(),
        JsonValue::Object(map) => {
            let mut servers = Vec::with_capacity(map.len());
            for (name, config) in map {
                let mut inline = BTreeMap::new();
                inline.insert(name.clone(), config.clone());
                servers.push(SubagentMcpServer::Inline(inline));
            }
            Ok(servers)
        }
        _ => bail!("expected object or array for mcp_servers"),
    }
}

fn parse_mcp_server_value(value: &JsonValue) -> Result<SubagentMcpServer> {
    match value {
        JsonValue::String(name) => Ok(SubagentMcpServer::Named(name.clone())),
        JsonValue::Object(map) => Ok(SubagentMcpServer::Inline(
            map.iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        )),
        _ => bail!("invalid mcp_servers entry"),
    }
}

fn optional_hooks(value: Option<&JsonValue>) -> Result<Option<HooksConfig>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("subagent hooks must be an object"))?;

    if object.contains_key("lifecycle") {
        let hooks = serde_json::from_value::<HooksConfig>(value.clone())
            .context("failed to parse VT Code lifecycle hooks")?;
        return Ok(Some(hooks));
    }

    let mut config = HooksConfig::default();
    for (event, raw_groups) in object {
        let target = match event.as_str() {
            "PreToolUse" | "pre_tool_use" => &mut config.lifecycle.pre_tool_use,
            "PostToolUse" | "post_tool_use" => &mut config.lifecycle.post_tool_use,
            "PermissionRequest" | "permission_request" => &mut config.lifecycle.permission_request,
            "Stop" | "stop" => &mut config.lifecycle.stop,
            "SubagentStart" | "subagent_start" => &mut config.lifecycle.subagent_start,
            "SubagentStop" | "subagent_stop" => &mut config.lifecycle.subagent_stop,
            _ => continue,
        };
        target.extend(parse_hook_groups(raw_groups)?);
    }

    Ok(Some(config))
}

fn parse_hook_groups(value: &JsonValue) -> Result<Vec<HookGroupConfig>> {
    let groups = value
        .as_array()
        .ok_or_else(|| anyhow!("hook groups must be arrays"))?;
    let mut parsed = Vec::with_capacity(groups.len());
    for group in groups {
        let Some(object) = group.as_object() else {
            bail!("hook group must be an object");
        };
        let matcher = object
            .get("matcher")
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let hooks = object
            .get("hooks")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| anyhow!("hook group requires hooks array"))?
            .iter()
            .map(parse_hook_command)
            .collect::<Result<Vec<_>>>()?;
        parsed.push(HookGroupConfig { matcher, hooks });
    }
    Ok(parsed)
}

fn parse_hook_command(value: &JsonValue) -> Result<HookCommandConfig> {
    let Some(object) = value.as_object() else {
        bail!("hook command must be an object");
    };
    let command = object
        .get("command")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("hook command requires a command string"))?;
    let timeout_seconds = object.get("timeout_seconds").and_then(JsonValue::as_u64);
    Ok(HookCommandConfig {
        kind: HookCommandKind::Command,
        command,
        timeout_seconds,
    })
}

fn parse_memory_scope(value: &str) -> Result<SubagentMemoryScope> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(SubagentMemoryScope::User),
        "project" => Ok(SubagentMemoryScope::Project),
        "local" => Ok(SubagentMemoryScope::Local),
        other => bail!("unsupported subagent memory scope '{other}'"),
    }
}

fn parse_agent_mode(value: &str) -> Result<AgentMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "primary" => Ok(AgentMode::Primary),
        "subagent" => Ok(AgentMode::Subagent),
        "all" => Ok(AgentMode::All),
        other => bail!("unsupported agent mode '{other}'"),
    }
}

fn toml_table_to_json_object(
    table: &toml::map::Map<String, toml::Value>,
) -> Result<JsonMap<String, JsonValue>> {
    let value = serde_json::to_value(table).context("failed to convert TOML table to JSON")?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("expected TOML table to convert into a JSON object"))
}

const fn default_subagents_enabled() -> bool {
    true
}

const fn default_subagents_max_concurrent() -> usize {
    3
}

const fn default_subagents_max_depth() -> usize {
    1
}

const fn default_subagents_default_timeout_seconds() -> u64 {
    300
}

const fn default_subagents_auto_delegate_read_only() -> bool {
    true
}

const fn default_background_subagents_enabled() -> bool {
    false
}

const fn default_background_refresh_interval_ms() -> u64 {
    2_000
}

const fn default_background_auto_restore() -> bool {
    false
}

fn default_background_toggle_shortcut() -> String {
    "ctrl+b".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        AgentMode, AgentSpecFieldClass, BackgroundSubagentConfig, SubagentDiscoveryInput,
        SubagentMcpServer, SubagentMemoryScope, SubagentRuntimeLimits, SubagentSource,
        builtin_subagents, classify_agent_spec_field, discover_subagents, load_cli_agents,
        load_subagent_from_file,
    };
    use crate::constants::tools;
    use crate::core::permissions::PermissionDefault;
    use anyhow::Result;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_agent_spec_fields_for_primary_and_subagent_roles() {
        assert_eq!(
            classify_agent_spec_field("name"),
            Some(AgentSpecFieldClass::Shared)
        );
        assert_eq!(
            classify_agent_spec_field("description"),
            Some(AgentSpecFieldClass::PrimaryMetadata)
        );
        assert_eq!(
            classify_agent_spec_field("aliases"),
            Some(AgentSpecFieldClass::PrimaryMetadata)
        );
        assert_eq!(
            classify_agent_spec_field("disallowedTools"),
            Some(AgentSpecFieldClass::PrimaryRuntime)
        );
        assert_eq!(
            classify_agent_spec_field("permissions"),
            Some(AgentSpecFieldClass::PrimaryRuntime)
        );
        assert_eq!(
            classify_agent_spec_field("mcpServers"),
            Some(AgentSpecFieldClass::PrimaryRuntime)
        );
        assert_eq!(
            classify_agent_spec_field("maxTurns"),
            Some(AgentSpecFieldClass::SubagentOnly)
        );
        assert_eq!(
            classify_agent_spec_field("initial_prompt"),
            Some(AgentSpecFieldClass::SubagentOnly)
        );
        assert_eq!(
            classify_agent_spec_field("mode"),
            Some(AgentSpecFieldClass::Availability)
        );
        assert_eq!(classify_agent_spec_field("unknown"), None);
    }

    #[test]
    fn parses_agent_availability_modes() -> Result<()> {
        let temp = TempDir::new()?;
        for (name, mode, expected) in [
            ("primary", "primary", AgentMode::Primary),
            ("subagent", "subagent", AgentMode::Subagent),
            ("all", "all", AgentMode::All),
        ] {
            let path = temp.path().join(format!("{name}.md"));
            fs::write(
                &path,
                format!(
                    r#"---
name: {name}
description: {name} agent
mode: {mode}
permissions:
  default: ask
---
Prompt."#
                ),
            )?;

            let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
            assert_eq!(spec.mode, expected);
        }

        Ok(())
    }

    #[test]
    fn defaults_missing_permissions_to_ask() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("missing-permissions.md");
        fs::write(
            &path,
            r#"---
name: missing-permissions
description: Missing permissions
---
Prompt."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert!(spec.permissions.allow.is_empty());
        assert!(spec.permissions.ask.is_empty());
        assert!(spec.permissions.auto.is_empty());
        assert!(spec.permissions.deny.is_empty());
        Ok(())
    }

    #[test]
    fn rejects_invalid_permissions_default() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("invalid-permissions.md");
        fs::write(
            &path,
            r#"---
name: invalid-permissions
description: Invalid permissions
permissions:
  default: plan
---
Prompt."#,
        )?;

        let err = load_subagent_from_file(&path, SubagentSource::ProjectVtcode).unwrap_err();
        assert!(
            err.to_string()
                .contains("failed to parse subagent permissions")
        );
        Ok(())
    }

    #[test]
    fn rejects_legacy_top_level_permission_fields() -> Result<()> {
        let temp = TempDir::new()?;

        for legacy_field in ["permissionMode", "permission_mode"] {
            let markdown_path = temp.path().join(format!("{legacy_field}.md"));
            fs::write(
                &markdown_path,
                format!(
                    r#"---
name: {legacy_field}
description: Legacy frontmatter permissions
permissions:
  default: ask
{legacy_field}: allow
---
Prompt."#
                ),
            )?;

            let markdown_err =
                load_subagent_from_file(&markdown_path, SubagentSource::ProjectVtcode).unwrap_err();
            assert!(markdown_err.to_string().contains(legacy_field));

            let toml_path = temp.path().join(format!("{legacy_field}.toml"));
            fs::write(
                &toml_path,
                format!(
                    r#"name = "{legacy_field}"
description = "Legacy TOML permissions"
prompt = "Prompt."
permissions = {{ default = "ask" }}
{legacy_field} = "allow"
"#
                ),
            )?;

            let toml_err =
                load_subagent_from_file(&toml_path, SubagentSource::ProjectCodex).unwrap_err();
            assert!(toml_err.to_string().contains(legacy_field));

            let cli_payload = json!({
                legacy_field: {
                    "description": "Legacy CLI permissions",
                    "permissions": { "default": "ask" },
                    legacy_field: "allow"
                }
            });

            let cli_err = load_cli_agents(&cli_payload).unwrap_err();
            assert!(cli_err.to_string().contains(legacy_field));
        }

        Ok(())
    }

    #[test]
    fn primary_agent_parser_accepts_supported_fields() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("build.md");
        fs::write(
            &path,
            r#"---
name: build
description: Primary build agent
tools: [Read, Bash]
disallowedTools: [Write]
model: gpt-5.4
color: blue
reasoning_effort: high
permissions:
  default: ask
  allow: [read_file]
  ask: [unified_exec]
  auto: [unified_search]
  deny: [write_file]
skills: [rust, repo]
mcpServers:
  - filesystem
  - demo:
      command: demo-mcp
hooks:
  PreToolUse:
    - matcher: Bash
      hooks:
        - command: echo pre
memory: project
aliases: [builder, implementer]
mode: primary
---
Primary prompt."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

        assert_eq!(spec.name, "build");
        assert_eq!(spec.description, "Primary build agent");
        assert_eq!(spec.prompt, "Primary prompt.");
        assert_eq!(
            spec.tools,
            Some(vec![
                tools::READ_FILE.to_string(),
                tools::UNIFIED_EXEC.to_string(),
            ])
        );
        assert_eq!(spec.disallowed_tools, vec![tools::WRITE_FILE.to_string()]);
        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert_eq!(spec.permissions.allow, vec![tools::READ_FILE.to_string()]);
        assert_eq!(spec.permissions.ask, vec![tools::UNIFIED_EXEC.to_string()]);
        assert_eq!(
            spec.permissions.auto,
            vec![tools::UNIFIED_SEARCH.to_string()]
        );
        assert_eq!(spec.permissions.deny, vec![tools::WRITE_FILE.to_string()]);
        assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(spec.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(spec.skills, vec!["rust".to_string(), "repo".to_string()]);
        assert_eq!(spec.mcp_servers.len(), 2);
        assert!(matches!(spec.mcp_servers[0], SubagentMcpServer::Named(_)));
        assert!(matches!(spec.mcp_servers[1], SubagentMcpServer::Inline(_)));
        assert_eq!(spec.memory, Some(SubagentMemoryScope::Project));
        assert_eq!(spec.color.as_deref(), Some("blue"));
        assert_eq!(
            spec.aliases,
            vec!["builder".to_string(), "implementer".to_string()]
        );
        assert_eq!(spec.mode, AgentMode::Primary);
        assert!(spec.hooks.is_some());
        assert!(spec.warnings.is_empty());
        Ok(())
    }

    #[test]
    fn primary_agent_specs_warn_for_subagent_only_fields() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("primary.md");
        fs::write(
            &path,
            r#"---
name: primary
description: Primary with child-only fields
mode: primary
permissions:
  default: ask
background: true
maxTurns: 4
initialPrompt: Start here
nickname_candidates: [helper]
isolation: full
---
Prompt."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

        assert!(spec.background);
        assert_eq!(spec.max_turns, Some(4));
        assert_eq!(spec.initial_prompt.as_deref(), Some("Start here"));
        assert_eq!(spec.nickname_candidates, vec!["helper".to_string()]);
        assert_eq!(spec.isolation.as_deref(), Some("full"));
        assert_eq!(
            spec.warnings,
            vec![
                "field 'background' is for subagents only and is ignored by primary agents"
                    .to_string(),
                "field 'max_turns' is for subagents only and is ignored by primary agents"
                    .to_string(),
                "field 'initial_prompt' is for subagents only and is ignored by primary agents"
                    .to_string(),
                "field 'nickname_candidates' is for subagents only and is ignored by primary agents"
                    .to_string(),
                "field 'isolation' is for subagents only and is ignored by primary agents"
                    .to_string(),
            ]
        );
        Ok(())
    }

    #[test]
    fn aliases_do_not_replace_canonical_agent_names() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("canonical.md");
        fs::write(
            &path,
            r#"---
name: canonical
description: Canonical primary
mode: primary
permissions:
  default: ask
aliases: [alias]
---
Prompt."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;

        assert_eq!(spec.name, "canonical");
        assert!(spec.matches_name("alias"));
        assert!(spec.matches_name("canonical"));
        Ok(())
    }

    #[test]
    fn primary_agent_parser_preserves_baseline_runtime_fields() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("baseline.toml");
        fs::write(
            &path,
            r#"name = "baseline"
description = "Baseline primary"
prompt = "Baseline prompt"
mode = "primary"
tools = ["unified_search", "unified_exec"]
disallowed_tools = ["unified_exec"]
permissions = { default = "allow" }
model = "gpt-5.4"
reasoning_effort = "medium"
"#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;

        assert_eq!(
            spec.tools,
            Some(vec![
                tools::UNIFIED_SEARCH.to_string(),
                tools::UNIFIED_EXEC.to_string(),
            ])
        );
        assert_eq!(spec.disallowed_tools, vec![tools::UNIFIED_EXEC.to_string()]);
        assert_eq!(spec.permissions.default, PermissionDefault::Allow);
        assert!(spec.permissions.allow.is_empty());
        assert!(spec.permissions.ask.is_empty());
        assert!(spec.permissions.auto.is_empty());
        assert!(spec.permissions.deny.is_empty());
        assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(spec.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(spec.prompt, "Baseline prompt");
        assert!(spec.warnings.is_empty());
        Ok(())
    }

    #[test]
    fn parses_claude_markdown_frontmatter() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("reviewer.md");
        fs::write(
            &path,
            r#"---
name: reviewer
description: Review code
tools: [Read, Grep, Glob]
disallowedTools: [Write]
model: sonnet
color: blue
permissions:
  default: deny
  allow: [read_file, unified_search, list_files]
skills: [rust]
memory: project
background: true
mode: primary
maxTurns: 7
nickname_candidates: [rev]
---

Review the target changes."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
        assert_eq!(spec.name, "reviewer");
        assert_eq!(spec.description, "Review code");
        assert_eq!(spec.model.as_deref(), Some("sonnet"));
        assert_eq!(spec.color.as_deref(), Some("blue"));
        assert_eq!(
            spec.tools,
            Some(vec![
                tools::READ_FILE.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
                tools::LIST_FILES.to_string(),
            ])
        );
        assert_eq!(spec.disallowed_tools, vec![tools::WRITE_FILE.to_string()]);
        assert!(spec.background);
        assert_eq!(spec.mode, AgentMode::Primary);
        assert_eq!(spec.max_turns, Some(7));
        assert_eq!(spec.prompt, "Review the target changes.");
        Ok(())
    }

    #[test]
    fn normalizes_claude_tool_aliases_to_vtcode_tools() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("debugger.md");
        fs::write(
            &path,
            r#"---
name: debugger
description: Debug agent
permissions:
  default: allow
tools: [Read, Bash, Edit, Write, Glob, Grep]
disallowedTools: [Task]
---
Debug the issue."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
        assert_eq!(
            spec.tools,
            Some(vec![
                tools::READ_FILE.to_string(),
                tools::UNIFIED_EXEC.to_string(),
                tools::EDIT_FILE.to_string(),
                tools::WRITE_FILE.to_string(),
                tools::LIST_FILES.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
            ])
        );
        assert_eq!(spec.disallowed_tools, vec![tools::SPAWN_AGENT.to_string()]);
        assert!(!spec.is_read_only());
        Ok(())
    }

    #[test]
    fn shell_only_agents_are_not_read_only() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("shell.md");
        fs::write(
            &path,
            r#"---
name: shell
description: Shell-capable agent
permissions:
  default: allow
tools: [Bash]
---
Run shell commands."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
        assert_eq!(spec.tools, Some(vec![tools::UNIFIED_EXEC.to_string()]));
        assert!(!spec.is_read_only());
        Ok(())
    }

    #[test]
    fn parses_codex_toml_definition() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("worker.toml");
        fs::write(
            &path,
            r##"name = "worker"
description = "Write-capable implementation agent"
developer_instructions = "Implement the assigned change."
model = "gpt-5.4"
color = "#4f8fd8"
model_reasoning_effort = "high"
nickname_candidates = ["builder"]
permissions = { default = "ask" }
"##,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;
        assert_eq!(spec.name, "worker");
        assert_eq!(spec.description, "Write-capable implementation agent");
        assert_eq!(spec.prompt, "Implement the assigned change.");
        assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(spec.color.as_deref(), Some("#4f8fd8"));
        assert_eq!(spec.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(spec.nickname_candidates, vec!["builder".to_string()]);
        Ok(())
    }

    #[test]
    fn precedence_prefers_project_vtcode_then_claude_then_codex_then_user() -> Result<()> {
        let temp = TempDir::new()?;
        fs::create_dir_all(temp.path().join(".codex/agents"))?;
        fs::create_dir_all(temp.path().join(".claude/agents"))?;
        fs::create_dir_all(temp.path().join(".vtcode/agents"))?;

        fs::write(
            temp.path().join(".codex/agents/example.toml"),
            r#"name = "example"
description = "codex"
developer_instructions = "codex"
permissions = { default = "ask" }
"#,
        )?;
        fs::write(
            temp.path().join(".claude/agents/example.md"),
            r#"---
name: example
description: claude
permissions:
  default: ask
---
claude"#,
        )?;
        fs::write(
            temp.path().join(".vtcode/agents/example.md"),
            r#"---
name: example
description: vtcode
permissions:
  default: ask
---
vtcode"#,
        )?;

        let mut input = SubagentDiscoveryInput::new(temp.path().to_path_buf());
        input.include_user_agents = false;
        let discovered = discover_subagents(&input)?;
        let effective = discovered
            .effective
            .into_iter()
            .find(|spec| spec.name == "example")
            .expect("example effective");
        assert_eq!(effective.description, "vtcode");
        assert_eq!(effective.source, SubagentSource::ProjectVtcode);
        Ok(())
    }

    #[test]
    fn agent_definitions_with_same_name_shadow_by_precedence() -> Result<()> {
        let temp = TempDir::new()?;
        let project_vtcode_agents = temp.path().join(".vtcode/agents");
        let project_claude_agents = temp.path().join(".claude/agents");
        fs::create_dir_all(&project_vtcode_agents)?;
        fs::create_dir_all(&project_claude_agents)?;
        fs::write(
            project_claude_agents.join("plan.md"),
            r#"---
name: plan
description: Project delegated plan child
permissions:
  default: ask
---
Project child plan."#,
        )?;
        fs::write(
            project_vtcode_agents.join("plan.md"),
            r#"---
name: plan
description: Project primary plan
mode: primary
permissions:
  default: ask
---
Project primary plan."#,
        )?;

        let mut input = SubagentDiscoveryInput::new(temp.path().to_path_buf());
        input.include_user_agents = false;
        let discovered = discover_subagents(&input)?;
        let project_plan_specs = discovered
            .effective
            .iter()
            .filter(|spec| spec.name == "plan")
            .collect::<Vec<_>>();

        assert_eq!(project_plan_specs.len(), 1);
        assert_eq!(project_plan_specs[0].description, "Project primary plan");
        assert_eq!(project_plan_specs[0].mode, AgentMode::Primary);
        assert_eq!(project_plan_specs[0].source, SubagentSource::ProjectVtcode);
        Ok(())
    }

    #[test]
    fn plugin_restrictions_strip_unsafe_overrides() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("plugin-agent.md");
        fs::write(
            &path,
            r#"---
name: plugin-agent
description: Plugin agent
permissions:
  default: ask
mcpServers:
  - github
hooks:
  PreToolUse:
    - matcher: Bash
      hooks:
        - type: command
          command: ./check.sh
---
Plugin prompt"#,
        )?;

        let spec = load_subagent_from_file(
            &path,
            SubagentSource::Plugin {
                plugin: "demo".to_string(),
            },
        )?;
        assert!(spec.mcp_servers.is_empty());
        assert!(spec.hooks.is_none());
        assert_eq!(spec.warnings.len(), 2);
        Ok(())
    }

    #[test]
    fn plugin_restrictions_normalize_permission_overrides() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("plugin-agent.md");
        fs::write(
            &path,
            r#"---
name: plugin-agent
description: Plugin agent
permissions:
  default: auto
  allow: [read_file, "Read(*)", Bash, "Bash(*)", edit_file, "Edit(/src/**)"]
  ask: [unified_exec]
  auto: [unified_search, "Glob(**/*.rs)", Write, "Write(*)", apply_patch, "apply_patch(*)"]
---
Plugin prompt"#,
        )?;

        let spec = load_subagent_from_file(
            &path,
            SubagentSource::Plugin {
                plugin: "demo".to_string(),
            },
        )?;

        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert_eq!(
            spec.permissions.allow,
            vec![tools::READ_FILE.to_string(), "Read(*)".to_string()]
        );
        assert_eq!(spec.permissions.ask, vec![tools::UNIFIED_EXEC.to_string()]);
        assert_eq!(
            spec.permissions.auto,
            vec![
                tools::UNIFIED_SEARCH.to_string(),
                "Glob(**/*.rs)".to_string(),
            ]
        );
        assert!(spec.warnings.iter().any(|warning| {
            warning == "plugin subagent permission overrides are restricted for safety"
        }));
        Ok(())
    }

    #[test]
    fn parses_subagent_lifecycle_hooks_from_frontmatter() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("hooks.md");
        fs::write(
            &path,
            r#"---
name: hook-agent
description: Hooked agent
permissions:
  default: ask
hooks:
  SubagentStart:
    - matcher: worker
      hooks:
        - type: command
          command: echo start
  SubagentStop:
    - hooks:
        - type: command
          command: echo stop
---
Hook prompt"#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
        let hooks = spec.hooks.expect("hooks");
        assert_eq!(hooks.lifecycle.subagent_start.len(), 1);
        assert_eq!(hooks.lifecycle.subagent_stop.len(), 1);
        assert_eq!(
            hooks.lifecycle.subagent_start[0].matcher.as_deref(),
            Some("worker")
        );
        Ok(())
    }

    #[test]
    fn builtin_aliases_cover_compat_names() {
        let builtins = builtin_subagents();
        let explorer = builtins
            .iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer builtin");
        let worker = builtins
            .iter()
            .find(|spec| spec.name == "worker")
            .expect("worker builtin");
        assert!(explorer.matches_name("explore"));
        assert!(worker.matches_name("general"));
        assert!(worker.matches_name("general-purpose"));
    }

    #[test]
    fn builtin_primary_agents_are_available() {
        let builtins = builtin_subagents();
        let default = builtins
            .iter()
            .find(|spec| spec.name == "default")
            .expect("missing default built-in");
        assert_eq!(default.permissions.default, PermissionDefault::Ask);
        let explorer = builtins
            .iter()
            .find(|spec| spec.name == "explorer")
            .expect("missing explorer built-in");
        assert_eq!(explorer.permissions.default, PermissionDefault::Deny);
        assert!(
            explorer
                .permissions
                .allow
                .contains(&tools::READ_FILE.to_string())
        );

        for name in ["build", "auto", "duck", "plan"] {
            let spec = builtins
                .iter()
                .find(|spec| spec.name == name && spec.is_primary())
                .unwrap_or_else(|| panic!("missing built-in primary agent {name}"));
            assert_eq!(spec.source, SubagentSource::Builtin);
            let expected_default = match name {
                "build" => PermissionDefault::Ask,
                "auto" => PermissionDefault::Auto,
                "duck" | "plan" => PermissionDefault::Deny,
                _ => unreachable!("unexpected built-in primary agent"),
            };
            assert_eq!(spec.permissions.default, expected_default);
        }
        let plan = builtins
            .iter()
            .find(|spec| spec.name == "plan" && spec.mode == AgentMode::All)
            .expect("missing built-in all-mode plan agent");
        assert_eq!(plan.source, SubagentSource::Builtin);
        assert_eq!(plan.permissions.default, PermissionDefault::Deny);

        let auto = builtins
            .iter()
            .find(|spec| spec.name == "auto" && spec.mode == AgentMode::Primary)
            .expect("missing built-in auto primary agent");
        assert_eq!(auto.permissions.default, PermissionDefault::Auto);
        assert!(
            builtins.iter().all(|spec| spec.name != "review"),
            "review must not be a built-in primary or subagent"
        );
    }

    #[test]
    fn ask_default_mutating_builtins_are_not_read_only() {
        let builtins = builtin_subagents();

        for name in ["default", "worker", "build"] {
            let spec = builtins
                .iter()
                .find(|spec| spec.name == name)
                .unwrap_or_else(|| panic!("missing built-in mutating agent {name}"));
            assert_eq!(spec.permissions.default, PermissionDefault::Ask);
            assert!(!spec.is_read_only());
        }

        let auto = builtins
            .iter()
            .find(|spec| spec.name == "auto")
            .expect("missing built-in auto agent");
        assert_eq!(auto.permissions.default, PermissionDefault::Auto);
        assert!(!auto.is_read_only());

        for name in ["duck", "explorer", "plan"] {
            let spec = builtins
                .iter()
                .find(|spec| spec.name == name)
                .unwrap_or_else(|| panic!("missing built-in read-only agent {name}"));
            assert_eq!(spec.permissions.default, PermissionDefault::Deny);
            assert!(spec.is_read_only());
        }
    }

    #[test]
    fn background_subagent_runtime_defaults_match_documented_shortcuts() {
        let config = BackgroundSubagentConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.default_agent, None);
        assert_eq!(config.refresh_interval_ms, 2_000);
        assert!(!config.auto_restore);
        assert_eq!(config.toggle_shortcut, "ctrl+b");
    }

    #[test]
    fn subagent_runtime_limits_embed_background_defaults() {
        let limits = SubagentRuntimeLimits::default();
        assert_eq!(limits.max_concurrent, 3);
        assert_eq!(limits.background.default_agent, None);
        assert_eq!(limits.background.toggle_shortcut, "ctrl+b");
    }

    #[test]
    fn background_subagent_runtime_deserializes_explicit_default_agent() {
        let config: BackgroundSubagentConfig = toml::from_str(
            r#"
enabled = true
default_agent = "rust-engineer"
refresh_interval_ms = 1500
auto_restore = true
toggle_shortcut = "ctrl+b"
"#,
        )
        .expect("background config");

        assert!(config.enabled);
        assert_eq!(config.default_agent.as_deref(), Some("rust-engineer"));
        assert_eq!(config.refresh_interval_ms, 1_500);
        assert!(config.auto_restore);
        assert_eq!(config.toggle_shortcut, "ctrl+b");
    }
}
