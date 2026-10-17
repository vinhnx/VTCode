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
mod tests {
    use super::builtin::{readonly_agent_permissions, readonly_interview_agent_permissions};
    use super::discovery::load_cli_agents;
    use super::permissions::normalize_subagent_tools;
    use super::{
        AgentMode, AgentSpecFieldClass, BackgroundSubagentConfig, IsolationMode, ReasoningEffortLevel,
        SubagentDiscoveryInput, SubagentMcpServer, SubagentMemoryScope, SubagentRuntimeLimits, SubagentSource,
        builtin_plan_agent, builtin_primary_auto_agent, builtin_primary_build_agent, builtin_primary_duck_agent,
        builtin_subagents, classify_agent_spec_field, discover_subagents, load_subagent_from_file,
    };
    use crate::constants::tools;
    use crate::core::permissions::PermissionDefault;
    use anyhow::Result;
    use serde_json::json;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn classifies_agent_spec_fields_for_primary_and_subagent_roles() {
        assert_eq!(classify_agent_spec_field("name"), Some(AgentSpecFieldClass::Shared));
        assert_eq!(classify_agent_spec_field("description"), Some(AgentSpecFieldClass::PrimaryMetadata));
        assert_eq!(classify_agent_spec_field("aliases"), Some(AgentSpecFieldClass::PrimaryMetadata));
        assert_eq!(classify_agent_spec_field("disallowedTools"), Some(AgentSpecFieldClass::PrimaryRuntime));
        assert_eq!(classify_agent_spec_field("permissions"), Some(AgentSpecFieldClass::PrimaryRuntime));
        assert_eq!(classify_agent_spec_field("mcpServers"), Some(AgentSpecFieldClass::PrimaryRuntime));
        assert_eq!(classify_agent_spec_field("maxTurns"), Some(AgentSpecFieldClass::SubagentOnly));
        assert_eq!(classify_agent_spec_field("initial_prompt"), Some(AgentSpecFieldClass::SubagentOnly));
        assert_eq!(classify_agent_spec_field("mode"), Some(AgentSpecFieldClass::Availability));
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
        assert!(err.to_string().contains("failed to parse subagent permissions"));
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

            let markdown_err = load_subagent_from_file(&markdown_path, SubagentSource::ProjectVtcode).unwrap_err();
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

            let toml_err = load_subagent_from_file(&toml_path, SubagentSource::ProjectCodex).unwrap_err();
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
  allow: [code_search]
  ask: [exec_command]
  auto: [code_search]
  deny: [apply_patch]
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
        assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
        assert_eq!(spec.disallowed_tools, vec![tools::APPLY_PATCH.to_string()]);
        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert_eq!(spec.permissions.allow, vec![tools::CODE_SEARCH.to_string()]);
        assert_eq!(spec.permissions.ask, vec![tools::EXEC_COMMAND.to_string()]);
        assert_eq!(spec.permissions.auto, vec![tools::CODE_SEARCH.to_string()]);
        assert_eq!(spec.permissions.deny, vec![tools::APPLY_PATCH.to_string()]);
        assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
        assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::High));
        assert_eq!(spec.skills, vec!["rust".to_string(), "repo".to_string()]);
        assert_eq!(spec.mcp_servers.len(), 2);
        assert!(matches!(spec.mcp_servers[0], SubagentMcpServer::Named(_)));
        assert!(matches!(spec.mcp_servers[1], SubagentMcpServer::Inline(_)));
        assert_eq!(spec.memory, Some(SubagentMemoryScope::Project));
        assert_eq!(spec.color.as_deref(), Some("blue"));
        assert_eq!(spec.aliases, vec!["builder".to_string(), "implementer".to_string()]);
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
        assert_eq!(spec.isolation, Some(IsolationMode::Full));
        assert_eq!(
            spec.warnings,
            vec![
                "field 'background' is for subagents only and is ignored by primary agents".to_string(),
                "field 'max_turns' is for subagents only and is ignored by primary agents".to_string(),
                "field 'initial_prompt' is for subagents only and is ignored by primary agents".to_string(),
                "field 'nickname_candidates' is for subagents only and is ignored by primary agents".to_string(),
                "field 'isolation' is for subagents only and is ignored by primary agents".to_string(),
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
tools = ["code_search", "exec_command"]
disallowed_tools = ["exec_command"]
permissions = { default = "allow" }
model = "gpt-5.6-sol"
reasoning_effort = "medium"
"#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;

        assert_eq!(spec.tools, Some(vec![tools::CODE_SEARCH.to_string(), tools::EXEC_COMMAND.to_string(),]));
        assert_eq!(spec.disallowed_tools, vec![tools::EXEC_COMMAND.to_string()]);
        assert_eq!(spec.permissions.default, PermissionDefault::Allow);
        assert!(spec.permissions.allow.is_empty());
        assert!(spec.permissions.ask.is_empty());
        assert!(spec.permissions.auto.is_empty());
        assert!(spec.permissions.deny.is_empty());
        assert_eq!(spec.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::Medium));
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
  allow: [code_search]
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
        assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
        assert_eq!(spec.disallowed_tools, vec![tools::APPLY_PATCH.to_string()]);
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
        assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string(), tools::APPLY_PATCH.to_string(),]));
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
        assert_eq!(spec.tools, Some(vec![tools::EXEC_COMMAND.to_string()]));
        assert!(!spec.is_read_only());
        Ok(())
    }

    #[test]
    fn normalizes_claude_read_aliases_to_one_exec_command() {
        let normalized = normalize_subagent_tools(
            ["Read", "Grep", "Glob", "list_files", "LISTFILES", "Read(*)"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        );

        assert_eq!(normalized, vec![tools::EXEC_COMMAND.to_string()]);
    }

    #[test]
    fn keeps_explicit_code_search_distinct_from_claude_read_aliases() {
        let normalized = normalize_subagent_tools(
            ["code_search", "Read", "Code_Search", "Glob(*)"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        );

        assert_eq!(normalized, vec![tools::CODE_SEARCH.to_string(), tools::EXEC_COMMAND.to_string(),]);
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
model = "gpt-5.6-sol"
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
        assert_eq!(spec.model.as_deref(), Some("gpt-5.6-sol"));
        assert_eq!(spec.color.as_deref(), Some("#4f8fd8"));
        assert_eq!(spec.reasoning_effort, Some(ReasoningEffortLevel::High));
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

        let spec = load_subagent_from_file(&path, SubagentSource::Plugin { plugin: "demo".to_string() })?;
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
  allow: [code_search, "Read(*)", Bash, "Bash(*)", apply_patch, "Edit(/src/**)"]
  ask: [exec_command]
  auto: [code_search, "Glob(**/*.rs)", Write, "Write(*)", apply_patch, "apply_patch(*)"]
---
Plugin prompt"#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::Plugin { plugin: "demo".to_string() })?;

        assert_eq!(spec.permissions.default, PermissionDefault::Ask);
        assert_eq!(spec.permissions.allow, vec![tools::CODE_SEARCH.to_string(), "Read(*)".to_string()]);
        assert_eq!(spec.permissions.ask, vec![tools::EXEC_COMMAND.to_string()]);
        assert_eq!(spec.permissions.auto, vec![tools::CODE_SEARCH.to_string(), "Glob(**/*.rs)".to_string(),]);
        assert!(
            spec.warnings
                .iter()
                .any(|warning| { warning == "plugin subagent permission overrides are restricted for safety" })
        );
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
        assert_eq!(hooks.lifecycle.subagent_start[0].matcher.as_deref(), Some("worker"));
        Ok(())
    }

    #[test]
    fn builtin_aliases_cover_compat_names() {
        let builtins = builtin_subagents();
        let explorer = builtins.iter().find(|spec| spec.name == "explorer").expect("explorer builtin");
        let worker = builtins.iter().find(|spec| spec.name == "worker").expect("worker builtin");
        assert!(explorer.matches_name("explore"));
        assert!(worker.matches_name("general"));
        assert!(worker.matches_name("general-purpose"));
    }

    #[test]
    fn builtin_primary_agents_are_available() {
        let builtins = builtin_subagents();
        let expected_readonly_tools = vec![
            tools::CODE_SEARCH.to_string(),
            tools::EXEC_COMMAND.to_string(),
            tools::GREP_FILE.to_string(),
            tools::READ_FILE.to_string(),
            tools::LIST_FILES.to_string(),
        ];
        let mut expected_primary_readonly_tools = expected_readonly_tools.clone();
        expected_primary_readonly_tools.push(tools::REQUEST_USER_INPUT.to_string());
        expected_primary_readonly_tools.push(tools::RECORD_DECISION.to_string());
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
        assert_eq!(explorer.permissions.allow, vec!["read".to_string()]);
        assert_eq!(explorer.tools.as_deref(), Some(expected_readonly_tools.as_slice()));
        assert!(
            !explorer
                .tools
                .as_deref()
                .unwrap_or_default()
                .contains(&tools::APPLY_PATCH.to_string())
        );
        assert!(explorer.disallowed_tools.is_empty());

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
            if matches!(name, "duck" | "plan") {
                assert_eq!(spec.tools.as_deref(), Some(expected_primary_readonly_tools.as_slice()));
                assert!(
                    !spec
                        .tools
                        .as_deref()
                        .unwrap_or_default()
                        .contains(&tools::APPLY_PATCH.to_string())
                );
                assert!(spec.disallowed_tools.is_empty());
                // The allow list must keep every listed tool wire-visible:
                // `request_user_input` for the interview, and (plan only)
                // `bash` for read-only exec_command inspection gated by the
                // planning dispatch checks.
                let mut expected_allow = vec![
                    "read".to_string(),
                    "request_user_input".to_string(),
                    tools::RECORD_DECISION.to_string(),
                ];
                if name == "plan" {
                    expected_allow.push("bash".to_string());
                }
                assert_eq!(spec.permissions.allow, expected_allow);
            }
        }
        let plan = builtins
            .iter()
            .find(|spec| spec.name == "plan" && spec.mode == AgentMode::Primary)
            .expect("missing built-in primary-only plan agent");
        assert_eq!(plan.source, SubagentSource::Builtin);
        assert_eq!(plan.permissions.default, PermissionDefault::Deny);
        // `plan` is primary-only (like `duck`); projects that want a delegatable
        // plan subagent define their own `.vtcode/agents/plan.md` (mode: subagent).
        assert!(!plan.is_subagent());

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
    fn build_and_auto_primary_agents_have_identical_tool_authority() {
        let builtins = builtin_subagents();
        let build = builtins.iter().find(|spec| spec.name == "build").expect("missing build");
        let auto = builtins.iter().find(|spec| spec.name == "auto").expect("missing auto");

        assert_eq!(build.tools, auto.tools);
        assert_eq!(build.disallowed_tools, auto.disallowed_tools);
        assert_eq!(build.tool_policy_overrides, auto.tool_policy_overrides);
        assert_eq!(build.mode, auto.mode);
        assert_eq!(build.mcp_servers, auto.mcp_servers);
        assert_eq!(build.skills, auto.skills);
        assert_eq!(build.permissions.allow, auto.permissions.allow);
        assert_eq!(build.permissions.ask, auto.permissions.ask);
        assert_eq!(build.permissions.auto, auto.permissions.auto);
        assert_eq!(build.permissions.deny, auto.permissions.deny);
        assert_eq!(build.permissions.default, PermissionDefault::Ask);
        assert_eq!(auto.permissions.default, PermissionDefault::Auto);
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

        for name in ["duck", "explorer"] {
            let spec = builtins
                .iter()
                .find(|spec| spec.name == name)
                .unwrap_or_else(|| panic!("missing built-in read-only agent {name}"));
            assert_eq!(spec.permissions.default, PermissionDefault::Deny);
            assert!(spec.is_read_only());
        }

        // `plan` permits read-only `bash` so exec_command stays wire-visible
        // during planning, so the static `is_read_only()` heuristic
        // classifies it as mutation-capable. Its mutations are instead
        // blocked dynamically by the planning-workflow dispatch gate, and
        // `resolve_approved_plan_execution_agent` excludes it by name from
        // executing approved plans.
        let plan = builtins
            .iter()
            .find(|spec| spec.name == "plan")
            .expect("missing built-in plan agent");
        assert_eq!(plan.permissions.default, PermissionDefault::Deny);
        assert!(!plan.is_read_only());
        assert!(
            !plan.permissions.allow.iter().any(|rule| rule == "edit" || rule == "write"),
            "plan must not gain direct file-mutation permissions"
        );
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

    #[test]
    fn emits_warning_for_legacy_tool_name_permissions() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("legacy.md");
        fs::write(
            &path,
            r#"---
name: legacy
description: Agent with legacy permission rules
permissions:
  default: deny
  allow: [read_file, "write_file(/docs/**)"]
  deny: [run_pty_cmd]
---
Legacy prompt."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectVtcode)?;
        assert!(spec.warnings.iter().any(|w| w.contains("read_file")));
        assert!(spec.warnings.iter().any(|w| w.contains("write_file")));
        assert!(spec.warnings.iter().any(|w| w.contains("run_pty_cmd")));
        Ok(())
    }

    #[test]
    fn builtin_plan_agent_exposes_request_user_input() {
        let spec = builtin_plan_agent();
        let tools = spec.tools.as_ref().expect("plan agent should have tool list");
        assert!(
            tools.iter().any(|t| t == tools::REQUEST_USER_INPUT),
            "plan agent should expose request_user_input for clarifying questions"
        );
        // The wire catalog filters advertised tools through these permission
        // rules; a static tool-list entry is not enough (turn_912 regression:
        // the planning wire catalog collapsed to only `code_search` because
        // `request_user_input`/`exec_command` hit the default-deny fallback).
        assert!(
            spec.permissions.allow.iter().any(|rule| rule == "request_user_input"),
            "plan agent permissions must allow request_user_input so it survives wire shaping"
        );
        assert!(
            spec.permissions.allow.iter().any(|rule| rule == "bash"),
            "plan agent permissions must allow bash so exec_command survives wire shaping; \
             the planning-workflow dispatch gate keeps execution read-only"
        );
    }

    #[test]
    fn builtin_plan_agent_and_duck_share_discussion_first_guidance() {
        let plan_prompt = builtin_plan_agent().prompt;
        let duck_prompt = builtin_primary_duck_agent().prompt;

        for prompt in [&plan_prompt, &duck_prompt] {
            assert!(prompt.contains("discussion-first"));
            assert!(prompt.contains("Clarify scope, constraints, contradictions, and options"));
            assert!(prompt.contains("Resolve ordinary ambiguity from repository evidence when possible"));
            assert!(prompt.contains("ask the user directly only when material ambiguity is critical"));
            assert!(prompt.contains("Stop researching when existing evidence supports a decision"));
        }
        assert!(duck_prompt.contains("rubber-ducking only"));
        assert!(duck_prompt.contains("pressing Tab to switch to the Build agent"));
        assert!(!duck_prompt.contains("<proposed_plan>"));
        assert!(!duck_prompt.contains("plan file"));
    }

    #[test]
    fn builtin_plan_agent_prompt_requires_grounded_discovery_and_approval() {
        let prompt = builtin_plan_agent().prompt;

        assert!(prompt.contains("repository-grounded, read-only discovery"));
        assert!(prompt.contains("exactly one final <proposed_plan> block"));
        assert!(prompt.contains("Never write the plan file with shell or file-editing tools"));
        assert!(prompt.contains("wait for explicit user approval before implementation"));
        assert!(prompt.contains("instead of suggesting an immediate edit"));
        assert_eq!(prompt.matches("wait for").count(), 1, "approval rule should be stated once");
    }

    #[test]
    fn builtin_agent_prompts_ground_claims_without_repeated_absolutes() {
        let mut prompts: Vec<(String, String)> =
            builtin_subagents().into_iter().map(|spec| (spec.name, spec.prompt)).collect();
        for spec in [
            builtin_primary_build_agent(),
            builtin_primary_auto_agent(),
            builtin_plan_agent(),
            builtin_primary_duck_agent(),
        ] {
            prompts.push((spec.name, spec.prompt));
        }

        for (name, prompt) in &prompts {
            assert!(!prompt.contains("Never speculate"), "{name} prompt repeats a bare absolute");
            assert!(!prompt.contains("in parallel for efficiency"), "{name} prompt coaches read strategy");
        }
        for (name, prompt) in prompts.iter().filter(|(name, _)| name != "duck" && name != "auto") {
            assert!(
                prompt.to_lowercase().contains("read") && prompt.contains("before"),
                "{name} prompt should still ask to read files before claiming or editing"
            );
        }
    }

    #[test]
    fn builtin_coordinator_restricts_tools_without_denying_worker_permissions() {
        let coordinator = super::builtin_primary_coordinator_agent();
        assert_eq!(coordinator.mode, AgentMode::Primary);
        assert_eq!(coordinator.permissions, builtin_primary_build_agent().permissions);
        let tool_ids = coordinator.tools.as_ref().unwrap();
        assert_eq!(
            tool_ids,
            &[
                "matrix",
                "request_user_input",
                "agent",
                "record_decision",
                "task_tracker"
            ]
        );
        assert!(coordinator.tool_policy_overrides.is_empty());
        assert!(
            coordinator
                .prompt
                .contains("Delegate all shell execution, file changes, and verification")
        );
        assert!(coordinator.prompt.contains("while the matrix is idle"));
        assert!(builtin_subagents().iter().any(|spec| spec.name == "coordinator"));
        assert_eq!(builtin_primary_build_agent().tools, None);
    }

    #[test]
    fn readonly_permissions_allow_request_user_input_but_not_bash() {
        let readonly = readonly_agent_permissions();
        assert_eq!(readonly.default, PermissionDefault::Deny);
        assert_eq!(readonly.allow, vec!["read".to_string()]);

        let interview = readonly_interview_agent_permissions();
        assert_eq!(interview.default, PermissionDefault::Deny);
        assert!(interview.allow.iter().any(|rule| rule == "read"));
        assert!(interview.allow.iter().any(|rule| rule == "request_user_input"));
        assert!(!interview.allow.iter().any(|rule| rule == "bash"), "duck must keep exec_command denied");
    }

    #[test]
    fn builtin_duck_agent_exposes_request_user_input() {
        let spec = builtin_primary_duck_agent();
        let tools = spec.tools.as_ref().expect("duck agent should have tool list");
        assert!(
            tools.iter().any(|t| t == tools::REQUEST_USER_INPUT),
            "duck agent should expose request_user_input for clarifying questions"
        );
    }
}
