use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use vtcode_commons::VtCodePaths;
use vtcode_commons::reasoning::ReasoningEffortLevel;

use crate::constants::tools;
use crate::constants::ui;
use crate::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use crate::core::tools::ToolPolicy;
use crate::hooks::{HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig};

use self::permissions::is_mutating_tool_name;

mod builtin;
mod discovery;
mod parse;
mod permissions;

pub use builtin::{
    builtin_plan_agent, builtin_primary_auto_agent, builtin_primary_build_agent, builtin_primary_coordinator_agent,
    builtin_primary_duck_agent, builtin_subagents,
};
pub use discovery::{discover_subagents, load_subagent_from_file};

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
    const fn priority(&self) -> usize {
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
            Self::UserVtcode => "user:canonical-config".to_string(),
            Self::UserClaude => "user:~/.claude".to_string(),
            Self::UserCodex => "user:~/.codex".to_string(),
            Self::Plugin { plugin } => format!("plugin:{plugin}"),
            Self::Builtin => "builtin".to_string(),
        }
    }

    #[must_use]
    const fn vtcode_native(&self) -> bool {
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

/// Subagent isolation mode
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationMode {
    /// Full isolation (separate process)
    Full,
    /// Git worktree isolation
    Worktree,
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
    pub reasoning_effort: Option<ReasoningEffortLevel>,
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
    pub isolation: Option<IsolationMode>,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub source: SubagentSource,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Per-tool policy overrides applied when this agent becomes active.
    /// Keys are tool names, values are the policy to enforce.
    /// Applied on top of (and overriding) the global `[tools.policies]` from vtcode.toml.
    #[serde(default)]
    pub tool_policy_overrides: BTreeMap<String, ToolPolicy>,
}

impl Default for SubagentSpec {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            prompt: String::new(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig {
                default: PermissionDefault::Ask,
                allow: Vec::new(),
                ask: Vec::new(),
                auto: Vec::new(),
                deny: Vec::new(),
            },
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: AgentMode::default(),
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: BTreeMap::new(),
        }
    }
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
        let lower_tools = tools.iter().map(|tool| tool.to_ascii_lowercase()).collect::<Vec<_>>();
        let lower_denied = self
            .disallowed_tools
            .iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect::<Vec<_>>();

        let denies_writes = lower_denied.iter().any(|tool| is_mutating_tool_name(tool.as_str()));

        if self.tools.is_some() {
            let exposes_mutation = lower_tools.iter().any(|tool| is_mutating_tool_name(tool.as_str()));
            !exposes_mutation
        } else {
            denies_writes
        }
    }

    #[must_use]
    fn permissions_allows_mutation(&self) -> bool {
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
            || self.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(candidate))
    }
}

#[must_use]
pub(crate) fn classify_agent_spec_field(field: &str) -> Option<AgentSpecFieldClass> {
    match field.trim() {
        "name" | "prompt" => Some(AgentSpecFieldClass::Shared),
        "description" | "color" | "aliases" => Some(AgentSpecFieldClass::PrimaryMetadata),
        "tools" | "disallowed_tools" | "disallowedTools" | "permissions" | "model" | "reasoning_effort" | "skills"
        | "mcp_servers" | "mcpServers" | "hooks" | "memory" => Some(AgentSpecFieldClass::PrimaryRuntime),
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
    toggle_shortcut: String,
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

const fn default_subagents_enabled() -> bool {
    true
}

/// Hard ceiling for subagent concurrency. The configurable `max_concurrent` is
/// clamped to this value at runtime so it can never be exceeded.
pub const SUBAGENT_HARD_CONCURRENCY_LIMIT: usize = 5;

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
mod tests;
