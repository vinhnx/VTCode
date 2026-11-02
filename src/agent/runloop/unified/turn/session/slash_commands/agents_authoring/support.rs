use super::*;

pub(super) fn tool_catalog(initial_tools: &[String], selected: &BTreeSet<String>) -> Vec<(String, String, String)> {
    let extra_tools = initial_tools
        .iter()
        .chain(selected.iter())
        .filter(|tool_id| TOOL_OPTIONS.iter().all(|entry| entry.id != tool_id.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut catalog = TOOL_OPTIONS
        .iter()
        .map(|entry| (entry.id.to_string(), entry.description.to_string(), entry.badge.to_string()))
        .collect::<Vec<_>>();
    catalog.extend(
        extra_tools
            .into_iter()
            .map(|tool_id| (tool_id, "Existing custom tool id.".to_string(), "Custom".to_string())),
    );
    catalog
}

pub(super) async fn select_native_agent_name(ctx: &mut SlashCommandContext<'_>) -> Result<Option<String>> {
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
        ctx.renderer
            .line(MessageStyle::Info, "No native `.vtcode` agent definitions are currently loaded.")?;
        return Ok(None);
    }

    let items = specs
        .iter()
        .map(|spec| {
            ui_list::action(
                spec.name.clone(),
                super::super::agent_subtitle(spec, false),
                Some(super::super::agent_badge(spec)),
                Tone::Accent,
                Some(InlineListSelection::ConfigAction(format!("{}{}", AUTHOR_ACTION_PREFIX, spec.name))),
            )
            .with_search_value(format!("{} {} {}", spec.name, spec.description, spec.source.label()))
        })
        .collect::<Vec<_>>();
    let selected = items.first().and_then(|item| item.selection.clone());
    ctx.handle.show_list_modal(
        "Edit native VT Code agent".to_string(),
        vec!["Select a native `.vtcode` agent definition to edit interactively.".to_string()],
        items,
        selected,
        Some(InlineListSearchConfig {
            label: String::new(),
            placeholder: Some("name, description, source".to_string()),
            fuzzy: false,
        }),
    );

    let Some(selection) = wait_for_list_modal_selection(ctx).await else {
        return Ok(None);
    };
    let InlineListSelection::ConfigAction(action) = selection else {
        return Ok(None);
    };
    Ok(action.strip_prefix(AUTHOR_ACTION_PREFIX).map(ToString::to_string))
}

pub(super) async fn resolve_named_custom_agent(ctx: &SlashCommandContext<'_>, name: &str) -> Result<SubagentSpec> {
    let controller = ctx
        .tool_registry
        .subagent_controller()
        .ok_or_else(|| anyhow!("Subagent controller is not active in this session"))?;
    controller
        .effective_specs()
        .await
        .into_iter()
        .find(|spec| spec.matches_name(name))
        .ok_or_else(|| anyhow!("Unknown agent {name}"))
}

pub(super) fn is_native_vtcode_spec(spec: &SubagentSpec) -> bool {
    matches!(spec.source, SubagentSource::ProjectVtcode | SubagentSource::UserVtcode) && spec.file_path.is_some()
}

pub(super) fn scope_from_source(source: &SubagentSource) -> Result<AgentDefinitionScope> {
    match source {
        SubagentSource::ProjectVtcode => Ok(AgentDefinitionScope::Project),
        SubagentSource::UserVtcode => Ok(AgentDefinitionScope::User),
        _ => bail!("Only native `.vtcode` agents are editable in the guided flow"),
    }
}

pub(super) fn strip_editable_keys(mapping: &mut YamlMapping) {
    for key in EDITABLE_FRONTMATTER_KEYS {
        mapping.remove(*key);
    }
}

pub(super) fn split_markdown_frontmatter(contents: &str) -> Result<(&str, &str)> {
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

pub(super) fn default_agent_tools() -> Vec<String> {
    ordered_tools(
        super::super::DEFAULT_AGENT_TOOL_IDS
            .iter()
            .map(|tool| (*tool).to_string())
            .collect(),
    )
}

pub(super) fn ordered_tools(tools: Vec<String>) -> Vec<String> {
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
        let left_key = known_order.get(left.as_str()).copied().unwrap_or(usize::MAX);
        let right_key = known_order.get(right.as_str()).copied().unwrap_or(usize::MAX);
        left_key
            .cmp(&right_key)
            .then_with(|| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()))
    });
    deduped
}

pub(super) fn workspace_agent_path(workspace_root: &Path, name: &str) -> PathBuf {
    workspace_root.join(".vtcode/agents").join(format!("{name}.md"))
}

pub(super) fn user_agent_path(name: &str) -> Result<PathBuf> {
    VtCodePaths::resolve()
        .and_then(|paths| paths.config_path(format!("agents/{name}.md")))
        .map_err(|error| anyhow!("Cannot resolve user-scope agent path: {error}"))
}

pub(super) fn insert_yaml_string(mapping: &mut YamlMapping, key: &str, value: &str) {
    mapping.insert(key.to_string(), YamlValue::String(value.to_string()));
}

pub(super) fn insert_yaml_bool(mapping: &mut YamlMapping, key: &str, value: bool) {
    mapping.insert(key.to_string(), YamlValue::Bool(value));
}

pub(super) fn insert_yaml_u64(mapping: &mut YamlMapping, key: &str, value: u64) {
    mapping.insert(key.to_string(), YamlValue::Number(serde_json::Number::from(value)));
}

pub(super) fn insert_yaml_string_list(mapping: &mut YamlMapping, key: &str, values: &[String]) {
    mapping.insert(
        key.to_string(),
        YamlValue::Array(values.iter().map(|value| YamlValue::String(value.clone())).collect()),
    );
}

pub(super) fn insert_yaml_permissions(mapping: &mut YamlMapping, permissions_config: &AgentPermissionsConfig) {
    let mut permissions = YamlMapping::new();
    insert_yaml_string(&mut permissions, "default", permission_default_label(permissions_config.default));
    insert_yaml_string_list_if_non_empty(&mut permissions, "allow", &permissions_config.allow);
    insert_yaml_string_list_if_non_empty(&mut permissions, "ask", &permissions_config.ask);
    insert_yaml_string_list_if_non_empty(&mut permissions, "auto", &permissions_config.auto);
    insert_yaml_string_list_if_non_empty(&mut permissions, "deny", &permissions_config.deny);
    mapping.insert("permissions".to_string(), YamlValue::Object(permissions));
}

fn insert_yaml_string_list_if_non_empty(mapping: &mut YamlMapping, key: &str, values: &[String]) {
    if !values.is_empty() {
        insert_yaml_string_list(mapping, key, values);
    }
}

pub(super) fn scope_label(scope: AgentDefinitionScope) -> &'static str {
    match scope {
        AgentDefinitionScope::Project => ".vtcode/agents/<name>.md",
        AgentDefinitionScope::User => "the canonical user config directory/agents/<name>.md",
    }
}

pub(super) fn permission_default_label(default: PermissionDefault) -> &'static str {
    match default {
        PermissionDefault::Ask => "ask",
        PermissionDefault::Allow => "allow",
        PermissionDefault::Auto => "auto",
        PermissionDefault::Deny => "deny",
    }
}

pub(super) fn availability_summary(draft: &NativeAgentDraft) -> &'static str {
    match draft.extra_frontmatter.get("mode").and_then(YamlValue::as_str) {
        Some("primary") => "primary",
        Some("all") => "primary and subagent",
        _ => "subagent",
    }
}

pub(super) fn memory_scope_label(scope: &SubagentMemoryScope) -> &'static str {
    match scope {
        SubagentMemoryScope::User => "user",
        SubagentMemoryScope::Project => "project",
        SubagentMemoryScope::Local => "local",
    }
}

pub(super) fn memory_summary(scope: Option<&SubagentMemoryScope>) -> String {
    scope.map(memory_scope_label).unwrap_or("unset").to_string()
}

pub(super) fn memory_item(title: &str, subtitle: &str, scope: Option<SubagentMemoryScope>) -> InlineListItem {
    ui_list::action(
        title.to_string(),
        subtitle.to_string(),
        None,
        Tone::Neutral,
        Some(InlineListSelection::ConfigAction(memory_action_key(scope.as_ref()))),
    )
    .with_search_value(format!("{title} {subtitle}"))
}

pub(super) fn memory_action_key(scope: Option<&SubagentMemoryScope>) -> String {
    match scope {
        None => "agents:author:memory:unset".to_string(),
        Some(scope) => format!("agents:author:memory:{}", memory_scope_label(scope)),
    }
}

pub(super) fn memory_scope_from_action(action: &str) -> Option<Option<SubagentMemoryScope>> {
    match action.strip_prefix("agents:author:memory:") {
        Some("unset") => Some(None),
        Some("user") => Some(Some(SubagentMemoryScope::User)),
        Some("project") => Some(Some(SubagentMemoryScope::Project)),
        Some("local") => Some(Some(SubagentMemoryScope::Local)),
        _ => None,
    }
}

pub(super) fn normalized_optional_string(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(ToString::to_string)
}

pub(super) fn author_action_item(title: &str, subtitle: &str, badge: Option<&str>, action: &str) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(ToString::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(format!("{AUTHOR_ACTION_PREFIX}{action}"))),
        search_value: Some(format!("{title} {subtitle}")),
        ..Default::default()
    }
}

pub(super) fn tools_summary(tools: &[String]) -> String {
    if tools.is_empty() {
        "No explicit allowlist".to_string()
    } else {
        tools.join(", ")
    }
}

pub(super) fn model_summary(draft: &NativeAgentDraft) -> String {
    let reasoning = draft
        .reasoning_effort
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("unset");
    format!("{} | reasoning {}", draft.model, reasoning)
}

pub(super) fn max_turns_summary(max_turns: Option<usize>) -> String {
    max_turns.map(|value| value.to_string()).unwrap_or_else(|| "unset".to_string())
}

struct ToolOption {
    id: &'static str,
    description: &'static str,
    badge: &'static str,
}

const TOOL_OPTIONS: &[ToolOption] = &[
    ToolOption {
        id: tools::EXEC_COMMAND,
        description: "Run shell commands and scripts.",
        badge: "Exec",
    },
    ToolOption {
        id: tools::WRITE_STDIN,
        description: "Send input to an active shell command session.",
        badge: "Exec",
    },
    ToolOption {
        id: tools::APPLY_PATCH,
        description: "Apply patch-based code changes.",
        badge: "Write",
    },
    ToolOption {
        id: tools::CODE_SEARCH,
        description: "Search code structure and symbol outlines.",
        badge: "Read",
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
