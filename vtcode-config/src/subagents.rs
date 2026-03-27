use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::core::PermissionMode;
use crate::hooks::{HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig};

const BUILTIN_DEFAULT_AGENT: &str = r#"You are the default VT Code execution subagent.

Work directly, keep context isolated from the parent session, and return concise summaries.
Match the repository's local patterns, verify changes, and avoid unrelated edits."#;

const BUILTIN_EXPLORER_AGENT: &str = r#"You are a fast read-only exploration subagent.

Search the codebase, inspect relevant files, and return concise findings with file references.
Do not modify files or take mutating actions."#;

const BUILTIN_PLAN_AGENT: &str = r#"You are a read-only planning research subagent.

Gather the minimum repository context needed to support a plan or design decision.
Return findings, risks, and constraints clearly; do not modify files."#;

const BUILTIN_WORKER_AGENT: &str = r#"You are a write-capable worker subagent.

Handle bounded implementation work, verify results, and return a concise outcome summary with
any important risks or follow-up items."#;

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
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum SubagentMcpServer {
    Named(String),
    Inline(BTreeMap<String, JsonValue>),
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
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<SubagentMcpServer>,
    #[serde(default)]
    pub hooks: Option<HooksConfig>,
    #[serde(default)]
    pub background: bool,
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
    pub fn is_read_only(&self) -> bool {
        if matches!(self.permission_mode, Some(PermissionMode::Plan)) {
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

        let denies_writes = lower_denied.iter().any(|tool| {
            tool == "edit"
                || tool == "write"
                || tool == "unified_file"
                || tool == "apply_patch"
                || tool == "create_file"
                || tool == "delete_file"
        });

        if self.tools.is_some() {
            let exposes_mutation = lower_tools.iter().any(|tool| {
                tool == "edit"
                    || tool == "write"
                    || tool == "unified_file"
                    || tool == "apply_patch"
                    || tool == "create_file"
                    || tool == "delete_file"
            });
            !exposes_mutation
        } else {
            denies_writes
        }
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
}

impl Default for SubagentRuntimeLimits {
    fn default() -> Self {
        Self {
            enabled: default_subagents_enabled(),
            max_concurrent: default_subagents_max_concurrent(),
            max_depth: default_subagents_max_depth(),
            default_timeout_seconds: default_subagents_default_timeout_seconds(),
            auto_delegate_read_only: default_subagents_auto_delegate_read_only(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredSubagents {
    pub effective: Vec<SubagentSpec>,
    pub shadowed: Vec<SubagentSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct SubagentDiscoveryInput {
    pub workspace_root: PathBuf,
    pub cli_agents: Option<JsonValue>,
    pub plugin_agent_files: Vec<(String, PathBuf)>,
}

impl SubagentDiscoveryInput {
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            cli_agents: None,
            plugin_agent_files: Vec::new(),
        }
    }
}

pub fn discover_subagents(input: &SubagentDiscoveryInput) -> Result<DiscoveredSubagents> {
    let mut discovered = Vec::new();
    discovered.extend(builtin_subagents());

    if let Some(home) = dirs::home_dir() {
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
        if let Some(existing) = effective_by_name.get(spec.name.as_str()) {
            if should_replace(existing, &spec) {
                shadowed.push(existing.clone());
                effective_by_name.insert(spec.name.clone(), spec);
            } else {
                shadowed.push(spec);
            }
        } else {
            effective_by_name.insert(spec.name.clone(), spec);
        }
    }

    Ok(DiscoveredSubagents {
        effective: effective_by_name.into_values().collect(),
        shadowed,
    })
}

pub fn builtin_subagents() -> Vec<SubagentSpec> {
    vec![
        SubagentSpec {
            name: "default".to_string(),
            description: "Default inheriting subagent for general delegated work.".to_string(),
            prompt: BUILTIN_DEFAULT_AGENT.to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: Some("inherit".to_string()),
            reasoning_effort: None,
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
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
            tools: Some(vec![
                "unified_search".to_string(),
                "unified_file".to_string(),
                "unified_exec".to_string(),
            ]),
            disallowed_tools: vec!["unified_file".to_string()],
            model: Some("small".to_string()),
            reasoning_effort: Some("low".to_string()),
            permission_mode: Some(PermissionMode::Plan),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
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
            name: "plan".to_string(),
            description: "Read-only planning researcher. Use proactively while gathering context for implementation plans.".to_string(),
            prompt: BUILTIN_PLAN_AGENT.to_string(),
            tools: Some(vec![
                "unified_search".to_string(),
                "unified_file".to_string(),
                "unified_exec".to_string(),
            ]),
            disallowed_tools: vec!["unified_file".to_string()],
            model: Some("inherit".to_string()),
            reasoning_effort: Some("medium".to_string()),
            permission_mode: Some(PermissionMode::Plan),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: vec!["planner".to_string()],
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
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
            reasoning_effort: None,
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
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
        let reasoning_effort = config
            .get("reasoning_effort")
            .or_else(|| config.get("model_reasoning_effort"))
            .or_else(|| config.get("effort"))
            .and_then(JsonValue::as_str)
            .map(ToString::to_string);
        let permission_mode = config
            .get("permissionMode")
            .or_else(|| config.get("permission_mode"))
            .and_then(JsonValue::as_str)
            .map(parse_permission_mode)
            .transpose()?;
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

        specs.push(SubagentSpec {
            name: name.clone(),
            description,
            prompt,
            tools,
            disallowed_tools,
            model,
            reasoning_effort,
            permission_mode,
            skills,
            mcp_servers,
            hooks,
            background,
            max_turns,
            nickname_candidates,
            initial_prompt,
            memory,
            isolation,
            aliases: Vec::new(),
            source: SubagentSource::Cli,
            file_path: None,
            warnings: Vec::new(),
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
    let prompt = rest[end_idx + 4..].trim().to_string();
    let frontmatter = serde_yaml::from_str::<JsonValue>(frontmatter_text)
        .context("failed to parse subagent YAML frontmatter")?;
    let Some(object) = frontmatter.as_object() else {
        bail!("subagent frontmatter must be a YAML mapping");
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
        .get("developer_instructions")
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
    let tools = optional_string_list(
        object
            .get("tools")
            .or_else(|| object.get("allowed_tools"))
            .or_else(|| object.get("enabled_tools")),
    )?;
    let disallowed_tools = optional_string_list(
        object
            .get("disallowedTools")
            .or_else(|| object.get("disallowed_tools"))
            .or_else(|| object.get("disabled_tools")),
    )?
    .unwrap_or_default();
    let model = object
        .get("model")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let reasoning_effort = object
        .get("reasoning_effort")
        .or_else(|| object.get("model_reasoning_effort"))
        .or_else(|| object.get("effort"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let permission_mode = object
        .get("permissionMode")
        .or_else(|| object.get("permission_mode"))
        .and_then(JsonValue::as_str)
        .map(parse_permission_mode)
        .transpose()?;
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

    Ok(SubagentSpec {
        name,
        description,
        prompt,
        tools,
        disallowed_tools,
        model,
        reasoning_effort,
        permission_mode,
        skills,
        mcp_servers,
        hooks,
        background,
        max_turns,
        nickname_candidates,
        initial_prompt,
        memory,
        isolation,
        aliases: Vec::new(),
        source,
        file_path: None,
        warnings: Vec::new(),
    })
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
    if spec.permission_mode.take().is_some() {
        spec.warnings
            .push("plugin subagent permission_mode is ignored for safety".to_string());
    }
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
            "Stop" | "stop" => &mut config.lifecycle.task_completed,
            "SubagentStart" | "subagent_start" => &mut config.lifecycle.session_start,
            "SubagentStop" | "subagent_stop" => &mut config.lifecycle.session_end,
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

fn parse_permission_mode(value: &str) -> Result<PermissionMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "default" => Ok(PermissionMode::Default),
        "acceptedits" | "accept_edits" | "accept-edits" => Ok(PermissionMode::AcceptEdits),
        "dontask" | "dont_ask" | "dont-ask" => Ok(PermissionMode::DontAsk),
        "bypasspermissions" | "bypass_permissions" | "bypass-permissions" => {
            Ok(PermissionMode::BypassPermissions)
        }
        "plan" => Ok(PermissionMode::Plan),
        "auto" => Ok(PermissionMode::Auto),
        other => bail!("unsupported subagent permission mode '{other}'"),
    }
}

fn parse_memory_scope(value: &str) -> Result<SubagentMemoryScope> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(SubagentMemoryScope::User),
        "project" => Ok(SubagentMemoryScope::Project),
        "local" => Ok(SubagentMemoryScope::Local),
        other => bail!("unsupported subagent memory scope '{other}'"),
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
    4
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

#[cfg(test)]
mod tests {
    use super::{
        SubagentDiscoveryInput, SubagentSource, builtin_subagents, discover_subagents,
        load_subagent_from_file,
    };
    use anyhow::Result;
    use std::fs;
    use tempfile::TempDir;

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
permissionMode: plan
skills: [rust]
memory: project
background: true
maxTurns: 7
nickname_candidates: [rev]
---

Review the target changes."#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectClaude)?;
        assert_eq!(spec.name, "reviewer");
        assert_eq!(spec.description, "Review code");
        assert_eq!(spec.model.as_deref(), Some("sonnet"));
        assert_eq!(spec.tools.as_ref().map(Vec::len), Some(3));
        assert_eq!(spec.disallowed_tools, vec!["Write".to_string()]);
        assert!(spec.background);
        assert_eq!(spec.max_turns, Some(7));
        assert_eq!(spec.prompt, "Review the target changes.");
        Ok(())
    }

    #[test]
    fn parses_codex_toml_definition() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("worker.toml");
        fs::write(
            &path,
            r#"name = "worker"
description = "Write-capable implementation agent"
developer_instructions = "Implement the assigned change."
model = "gpt-5.4"
model_reasoning_effort = "high"
nickname_candidates = ["builder"]
"#,
        )?;

        let spec = load_subagent_from_file(&path, SubagentSource::ProjectCodex)?;
        assert_eq!(spec.name, "worker");
        assert_eq!(spec.description, "Write-capable implementation agent");
        assert_eq!(spec.prompt, "Implement the assigned change.");
        assert_eq!(spec.model.as_deref(), Some("gpt-5.4"));
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
"#,
        )?;
        fs::write(
            temp.path().join(".claude/agents/example.md"),
            r#"---
name: example
description: claude
---
claude"#,
        )?;
        fs::write(
            temp.path().join(".vtcode/agents/example.md"),
            r#"---
name: example
description: vtcode
---
vtcode"#,
        )?;

        let discovered =
            discover_subagents(&SubagentDiscoveryInput::new(temp.path().to_path_buf()))?;
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
    fn plugin_restrictions_strip_unsafe_overrides() -> Result<()> {
        let temp = TempDir::new()?;
        let path = temp.path().join("plugin-agent.md");
        fs::write(
            &path,
            r#"---
name: plugin-agent
description: Plugin agent
permissionMode: bypassPermissions
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
        assert!(spec.permission_mode.is_none());
        assert!(spec.mcp_servers.is_empty());
        assert!(spec.hooks.is_none());
        assert_eq!(spec.warnings.len(), 3);
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
}
