//! Subagent definition parsing (markdown frontmatter, Codex TOML, JSON maps).

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::collections::BTreeMap;

use vtcode_commons::reasoning::ReasoningEffortLevel;

use crate::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use crate::hooks::{HookCommandConfig, HookCommandKind, HookGroupConfig, HooksConfig};

use super::permissions::{
    apply_plugin_restrictions, normalize_subagent_tool_list, normalize_subagent_tools, warn_legacy_permission_rules,
};
use super::{AgentMode, IsolationMode, SubagentMcpServer, SubagentMemoryScope, SubagentSource, SubagentSpec};

pub(super) fn parse_markdown_subagent(content: &str, source: SubagentSource) -> Result<SubagentSpec> {
    let trimmed = content.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        bail!("markdown subagent is missing YAML frontmatter");
    };
    let Some(end_idx) = rest.find("\n---") else {
        bail!("markdown subagent is missing closing frontmatter delimiter");
    };
    let frontmatter_text = rest[..end_idx].trim();
    let prompt_body = rest[end_idx + 4..].trim().to_string();
    let frontmatter =
        serde_saphyr::from_str::<JsonValue>(frontmatter_text).context("failed to parse subagent YAML frontmatter")?;
    let Some(object) = frontmatter.as_object() else {
        bail!("subagent frontmatter must be a YAML mapping");
    };
    let prompt = if prompt_body.is_empty() {
        object.get("prompt").and_then(JsonValue::as_str).unwrap_or_default().to_string()
    } else {
        prompt_body
    };

    let mut spec = subagent_spec_from_json_map(object, prompt, source.clone())?;
    if matches!(source, SubagentSource::Plugin { .. }) {
        apply_plugin_restrictions(&mut spec);
    }
    Ok(spec)
}

pub(super) fn parse_codex_toml_subagent(content: &str, source: SubagentSource) -> Result<SubagentSpec> {
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

pub(super) fn subagent_spec_from_json_map(
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
    let model = object.get("model").and_then(JsonValue::as_str).map(ToString::to_string);
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
        .and_then(ReasoningEffortLevel::parse);
    let permissions = parse_required_permissions(object)?;
    let skills = optional_string_list(object.get("skills"))?.unwrap_or_default();
    let mcp_servers = optional_mcp_servers(object.get("mcpServers").or_else(|| object.get("mcp_servers")))?;
    let hooks = optional_hooks(object.get("hooks"))?;
    let background = object.get("background").and_then(JsonValue::as_bool).unwrap_or(false);
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
    let nickname_candidates = optional_string_list(object.get("nickname_candidates"))?.unwrap_or_default();
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
        .map(parse_isolation_mode)
        .transpose()?;
    let aliases = optional_string_list(object.get("aliases"))?.unwrap_or_default();
    let mut warnings = primary_agent_subagent_only_field_warnings(object, mode);
    warnings.extend(warn_legacy_permission_rules(&permissions));

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
        tool_policy_overrides: BTreeMap::new(),
    })
}

pub(super) fn primary_agent_subagent_only_field_warnings(
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
    .map(|(field, _)| format!("field '{field}' is for subagents only and is ignored by primary agents"))
    .collect()
}

pub(super) fn parse_required_permissions(object: &JsonMap<String, JsonValue>) -> Result<AgentPermissionsConfig> {
    if let Some(legacy_field) = object.keys().find(|field| is_legacy_permission_field(field)) {
        bail!("unsupported legacy subagent field '{legacy_field}'; use 'permissions.default'");
    }

    let Some(value) = object.get("permissions") else {
        return Ok(AgentPermissionsConfig::new(PermissionDefault::Ask));
    };

    serde_json::from_value::<AgentPermissionsConfig>(value.clone()).context("failed to parse subagent permissions")
}

pub(super) fn is_legacy_permission_field(field: &str) -> bool {
    field
        .strip_prefix("permission")
        .is_some_and(|suffix| matches!(suffix, "Mode" | "_mode"))
}

pub(super) fn required_string(object: &JsonMap<String, JsonValue>, key: &str) -> Result<String> {
    object
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| anyhow!("missing required subagent field '{key}'"))
}

pub(super) fn optional_string_list(value: Option<&JsonValue>) -> Result<Option<Vec<String>>> {
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

pub(super) fn optional_mcp_servers(value: Option<&JsonValue>) -> Result<Vec<SubagentMcpServer>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    match value {
        JsonValue::Null => Ok(Vec::new()),
        JsonValue::Array(entries) => entries.iter().map(parse_mcp_server_value).collect::<Result<Vec<_>>>(),
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

pub(super) fn parse_mcp_server_value(value: &JsonValue) -> Result<SubagentMcpServer> {
    match value {
        JsonValue::String(name) => Ok(SubagentMcpServer::Named(name.clone())),
        JsonValue::Object(map) => {
            Ok(SubagentMcpServer::Inline(map.iter().map(|(key, value)| (key.clone(), value.clone())).collect()))
        }
        _ => bail!("invalid mcp_servers entry"),
    }
}

pub(super) fn optional_hooks(value: Option<&JsonValue>) -> Result<Option<HooksConfig>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let object = value.as_object().ok_or_else(|| anyhow!("subagent hooks must be an object"))?;

    if object.contains_key("lifecycle") {
        let hooks =
            serde_json::from_value::<HooksConfig>(value.clone()).context("failed to parse VT Code lifecycle hooks")?;
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

pub(super) fn parse_hook_groups(value: &JsonValue) -> Result<Vec<HookGroupConfig>> {
    let groups = value.as_array().ok_or_else(|| anyhow!("hook groups must be arrays"))?;
    let mut parsed = Vec::with_capacity(groups.len());
    for group in groups {
        let Some(object) = group.as_object() else {
            bail!("hook group must be an object");
        };
        let matcher = object.get("matcher").and_then(JsonValue::as_str).map(ToString::to_string);
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

pub(super) fn parse_hook_command(value: &JsonValue) -> Result<HookCommandConfig> {
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

pub(super) fn parse_memory_scope(value: &str) -> Result<SubagentMemoryScope> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(SubagentMemoryScope::User),
        "project" => Ok(SubagentMemoryScope::Project),
        "local" => Ok(SubagentMemoryScope::Local),
        other => bail!("unsupported subagent memory scope '{other}'"),
    }
}

pub(super) fn parse_agent_mode(value: &str) -> Result<AgentMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "primary" => Ok(AgentMode::Primary),
        "subagent" => Ok(AgentMode::Subagent),
        "all" => Ok(AgentMode::All),
        other => bail!("unsupported agent mode '{other}'"),
    }
}

pub(super) fn parse_isolation_mode(value: &str) -> Result<IsolationMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(IsolationMode::Full),
        "worktree" => Ok(IsolationMode::Worktree),
        other => bail!("unsupported isolation mode '{other}'"),
    }
}

pub(super) fn toml_table_to_json_object(
    table: &toml::map::Map<String, toml::Value>,
) -> Result<JsonMap<String, JsonValue>> {
    let value = serde_json::to_value(table).context("failed to convert TOML table to JSON")?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("expected TOML table to convert into a JSON object"))
}
