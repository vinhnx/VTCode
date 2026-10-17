//! Subagent discovery and file/CLI loading.

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use vtcode_commons::VtCodePaths;
use vtcode_commons::reasoning::ReasoningEffortLevel;

use super::parse::{
    optional_hooks, optional_mcp_servers, optional_string_list, parse_agent_mode, parse_codex_toml_subagent,
    parse_isolation_mode, parse_markdown_subagent, parse_memory_scope, parse_required_permissions,
    primary_agent_subagent_only_field_warnings, required_string,
};
use super::permissions::{normalize_subagent_tool_list, warn_legacy_permission_rules};
use super::{DiscoveredSubagents, SubagentDiscoveryInput, SubagentSource, SubagentSpec, builtin_subagents};

pub fn discover_subagents(input: &SubagentDiscoveryInput) -> Result<DiscoveredSubagents> {
    let mut discovered = Vec::new();
    discovered.extend(builtin_subagents());

    if input.include_user_agents
        && let Some(home) = dirs::home_dir()
    {
        discovered.extend(load_subagents_from_dir(&home.join(".codex/agents"), SubagentSource::UserCodex)?);
        discovered.extend(load_subagents_from_dir(&home.join(".claude/agents"), SubagentSource::UserClaude)?);
        // Read the legacy root first so a canonical XDG agent with the same
        // name wins the same-priority compatibility tie below.
        let user_vtcode_roots = VtCodePaths::resolve()
            .map(|paths| vec![paths.legacy_dir().join("agents"), paths.config_dir().join("agents")])
            .unwrap_or_else(|_| vec![home.join(".vtcode/agents")]);
        for root in user_vtcode_roots {
            discovered.extend(load_subagents_from_dir(&root, SubagentSource::UserVtcode)?);
        }
    }

    discovered
        .extend(load_subagents_from_dir(&input.workspace_root.join(".codex/agents"), SubagentSource::ProjectCodex)?);
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
        let source = SubagentSource::Plugin { plugin: plugin_name.clone() };
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
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read subagent directory {}", dir.display()))? {
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
    let content = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut spec = match source {
        SubagentSource::ProjectCodex | SubagentSource::UserCodex => {
            parse_codex_toml_subagent(&content, source.clone())?
        }
        _ => parse_markdown_subagent(&content, source.clone())?,
    };
    spec.file_path = Some(path.to_path_buf());
    Ok(spec)
}

pub(super) fn load_cli_agents(value: &JsonValue) -> Result<Vec<SubagentSpec>> {
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
        let prompt = config.get("prompt").and_then(JsonValue::as_str).unwrap_or_default().to_string();
        let tools = optional_string_list(config.get("tools"))?;
        let disallowed_tools = optional_string_list(config.get("disallowedTools"))?.unwrap_or_default();
        let model = config.get("model").and_then(JsonValue::as_str).map(ToString::to_string);
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
            .and_then(ReasoningEffortLevel::parse);
        let permissions = parse_required_permissions(config)?;
        let skills = optional_string_list(config.get("skills"))?.unwrap_or_default();
        let mcp_servers = optional_mcp_servers(config.get("mcpServers").or_else(|| config.get("mcp_servers")))?;
        let hooks = optional_hooks(config.get("hooks"))?;
        let max_turns = config
            .get("maxTurns")
            .or_else(|| config.get("max_turns"))
            .and_then(JsonValue::as_u64)
            .map(|value| value as usize);
        let background = config.get("background").and_then(JsonValue::as_bool).unwrap_or(false);
        let mode = config
            .get("mode")
            .and_then(JsonValue::as_str)
            .map(parse_agent_mode)
            .transpose()?
            .unwrap_or_default();
        let nickname_candidates = optional_string_list(config.get("nickname_candidates"))?.unwrap_or_default();
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
            .map(parse_isolation_mode)
            .transpose()?;
        let aliases = optional_string_list(config.get("aliases"))?.unwrap_or_default();
        let mut warnings = primary_agent_subagent_only_field_warnings(config, mode);
        warnings.extend(warn_legacy_permission_rules(&permissions));

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
            tool_policy_overrides: BTreeMap::new(),
        });
    }

    Ok(specs)
}
