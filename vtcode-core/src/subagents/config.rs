use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_config::core::tools::ToolPolicy;
use vtcode_config::{
    HooksConfig, McpProviderConfig, SubagentMcpServer, SubagentMemoryScope, SubagentSource,
    SubagentSpec,
};

use super::constants::{
    NON_MUTATING_TOOL_PREFIXES, SUBAGENT_MIN_BACKGROUND_MAX_TURNS, SUBAGENT_MIN_MAX_TURNS,
    SUBAGENT_TOOL_NAMES,
};
use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::{ReasoningEffortLevel, SystemPromptMode, ToolDocumentationMode};
use crate::core::threads::build_thread_archive_metadata;
use crate::llm::provider::ToolDefinition;
use crate::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX;
use crate::utils::session_archive::{SessionArchiveMetadata, SessionForkMode};

#[derive(Debug, Clone)]
pub struct ResolvedAgentRuntimeView {
    pub canonical_name: String,
    pub display_name: String,
    pub description: String,
    pub color: Option<String>,
    pub aliases: Vec<String>,
    pub instructions: String,
    pub tools: Option<Vec<String>>,
    pub disallowed_tools: Vec<String>,
    pub permissions: AgentPermissionsConfig,
    pub model: Option<String>,
    pub reasoning_effort: Option<ReasoningEffortLevel>,
    pub hooks: Option<HooksConfig>,
    pub mcp_servers: Vec<SubagentMcpServer>,
    pub skills: Vec<String>,
    pub memory: Option<SubagentMemoryScope>,
    pub read_only: bool,
    pub source: SubagentSource,
    pub file_path: Option<PathBuf>,
    pub tool_policy_overrides: BTreeMap<String, ToolPolicy>,
}

impl ResolvedAgentRuntimeView {
    #[must_use]
    pub fn from_spec(spec: &SubagentSpec) -> Self {
        Self {
            canonical_name: spec.name.clone(),
            display_name: spec.name.clone(),
            description: spec.description.clone(),
            color: spec.color.clone(),
            aliases: spec.aliases.clone(),
            instructions: spec.prompt.clone(),
            tools: spec.tools.clone(),
            disallowed_tools: spec.disallowed_tools.clone(),
            permissions: spec.permissions.clone(),
            model: spec.model.clone(),
            reasoning_effort: spec.reasoning_effort,
            hooks: spec.hooks.clone(),
            mcp_servers: spec.mcp_servers.clone(),
            skills: spec.skills.clone(),
            memory: spec.memory,
            read_only: spec.is_read_only(),
            source: spec.source.clone(),
            file_path: spec.file_path.clone(),
            tool_policy_overrides: spec.tool_policy_overrides.clone(),
        }
    }
}

// ─── Child Config Building ─────────────────────────────────────────────────

pub fn build_child_config(
    parent: &VTCodeConfig,
    spec: &SubagentSpec,
    model: &str,
    max_turns: Option<usize>,
) -> VTCodeConfig {
    build_child_config_from_runtime(
        parent,
        &ResolvedAgentRuntimeView::from_spec(spec),
        model,
        max_turns,
    )
}

fn build_child_config_from_runtime(
    parent: &VTCodeConfig,
    runtime: &ResolvedAgentRuntimeView,
    model: &str,
    max_turns: Option<usize>,
) -> VTCodeConfig {
    let mut child = parent.clone();
    child.agent.default_model = model.to_string();
    child.runtime_agent_permissions = Some(runtime.permissions.clone());
    // Apply a lightweight default profile so a delegated child does not replay
    // the parent bootstrap cost on every turn. This is currently a fixed
    // default; a future enhancement may let a subagent spec opt into a heavier
    // profile via explicit `system_prompt_mode`/`tool_documentation_mode`
    // fields, but today the lightweight profile is always applied.
    apply_subagent_lightweight_profile(&mut child);
    normalize_child_max_turns_config(&mut child, max_turns);

    child.permissions.allow = resolve_child_allowed_tools(parent, runtime);
    child.permissions.deny = resolve_child_denied_tools(parent, runtime);
    merge_child_hooks(&mut child, runtime.hooks.as_ref());
    // Drop parent MCP providers by default; only attach servers explicitly
    // requested by the subagent spec. This prevents multiplying MCP schema
    // tax across every child. (H1: intentional behavioral change — specs that
    // need a parent MCP server must declare it via `mcp_servers`.)
    child.mcp.providers = resolve_child_mcp_providers(parent, runtime);
    child
}

/// Forces the minimal system-prompt and tool-documentation modes for a
/// subagent child config. Isolated so the subagent profile contract is
/// testable without building a full runtime.
fn apply_subagent_lightweight_profile(child: &mut VTCodeConfig) {
    child.agent.system_prompt_mode = SystemPromptMode::Minimal;
    child.agent.tool_documentation_mode = ToolDocumentationMode::Minimal;
}

/// Resolves the child's allow-list. When the spec declares tools, the child
/// allow-list is the intersection of the parent allow-list and the declared
/// tools (with subagent-internal tools removed). When the spec declares
/// nothing, the parent allow-list is inherited unchanged.
fn resolve_child_allowed_tools(
    parent: &VTCodeConfig,
    runtime: &ResolvedAgentRuntimeView,
) -> Vec<String> {
    let allowed_tools = runtime.tools.clone().unwrap_or_default();
    if allowed_tools.is_empty() {
        return parent.permissions.allow.clone();
    }
    let filtered: Vec<String> = allowed_tools
        .into_iter()
        .filter(|tool| !SUBAGENT_TOOL_NAMES.iter().any(|blocked| blocked == tool))
        .collect();
    intersect_allowed_tools(&parent.permissions.allow, &filtered)
}

/// Resolves the child's deny-list: the parent deny-list, extended with the
/// spec's disallowed tools and the always-blocked subagent-internal tools.
fn resolve_child_denied_tools(
    parent: &VTCodeConfig,
    runtime: &ResolvedAgentRuntimeView,
) -> Vec<String> {
    let mut denied = parent.permissions.deny.clone();
    denied.extend(runtime.disallowed_tools.clone());
    for tool in SUBAGENT_TOOL_NAMES {
        if !denied.iter().any(|entry| entry == *tool) {
            denied.push((*tool).to_string());
        }
    }
    denied
}

/// Resolves the child's MCP providers. Parent providers are NOT inherited;
/// only servers named or inlined by the spec are attached. This keeps the
/// child bootstrap lean and avoids replaying the parent's MCP schema tax.
fn resolve_child_mcp_providers(
    parent: &VTCodeConfig,
    runtime: &ResolvedAgentRuntimeView,
) -> Vec<McpProviderConfig> {
    let mut providers = Vec::new();
    merge_child_mcp_servers(
        &mut providers,
        &parent.mcp.providers,
        runtime.mcp_servers.as_slice(),
    );
    providers
}

fn normalize_child_max_turns_config(child: &mut VTCodeConfig, max_turns: Option<usize>) {
    if let Some(max_turns) = normalize_child_max_turns(max_turns) {
        child.automation.full_auto.max_turns = max_turns;
    }
}

pub fn normalize_child_max_turns(max_turns: Option<usize>) -> Option<usize> {
    max_turns.map(|value| value.max(SUBAGENT_MIN_MAX_TURNS))
}

pub fn normalize_background_child_max_turns(
    max_turns: Option<usize>,
    background: bool,
) -> Option<usize> {
    let normalized = normalize_child_max_turns(max_turns);
    if background {
        normalized.map(|value| value.max(SUBAGENT_MIN_BACKGROUND_MAX_TURNS))
    } else {
        normalized
    }
}

pub fn prepare_child_runtime_config(
    parent: &VTCodeConfig,
    spec: &SubagentSpec,
    parent_model: &str,
    parent_provider: &str,
    parent_reasoning_effort: ReasoningEffortLevel,
    max_turns: Option<usize>,
    model_override: Option<&str>,
    reasoning_override: Option<&str>,
    resolve_model: impl FnOnce(
        &VTCodeConfig,
        &str,
        &str,
        Option<&str>,
        Option<&str>,
        &str,
    ) -> Result<ModelId>,
) -> Result<(ModelId, ReasoningEffortLevel, VTCodeConfig)> {
    let runtime = ResolvedAgentRuntimeView::from_spec(spec);
    let resolved_model = resolve_model(
        parent,
        parent_model,
        parent_provider,
        model_override,
        runtime.model.as_deref(),
        runtime.canonical_name.as_str(),
    )?;
    let mut child_cfg =
        build_child_config_from_runtime(parent, &runtime, &resolved_model.as_str(), max_turns);
    let child_reasoning_effort = reasoning_override
        .and_then(ReasoningEffortLevel::parse)
        .or(runtime.reasoning_effort)
        .unwrap_or(parent_reasoning_effort);
    child_cfg.agent.default_model = resolved_model.to_string();
    child_cfg.agent.reasoning_effort = child_reasoning_effort;
    Ok((resolved_model, child_reasoning_effort, child_cfg))
}

fn intersect_allowed_tools(parent_allowed: &[String], spec_allowed: &[String]) -> Vec<String> {
    if parent_allowed.is_empty() {
        return spec_allowed.to_vec();
    }

    parent_allowed
        .iter()
        .filter(|rule| parent_rule_matches_spec_tools(rule, spec_allowed))
        .cloned()
        .collect()
}

fn parent_rule_matches_spec_tools(rule: &str, spec_allowed: &[String]) -> bool {
    let rule = rule.trim();
    if rule.is_empty() {
        return false;
    }

    let prefix = rule
        .split_once('(')
        .map_or(rule, |(prefix, _)| prefix)
        .trim();
    match prefix.to_ascii_lowercase().as_str() {
        "read" => spec_allowed
            .iter()
            .any(|tool| tool_supports_read_permission(tool)),
        "edit" => spec_allowed
            .iter()
            .any(|tool| tool_supports_edit_permission(tool)),
        "write" => spec_allowed
            .iter()
            .any(|tool| tool_supports_write_permission(tool)),
        "bash" => spec_allowed
            .iter()
            .any(|tool| tool_supports_bash_permission(tool)),
        "webfetch" => spec_allowed
            .iter()
            .any(|tool| tool_supports_web_fetch_permission(tool)),
        _ if rule.starts_with(MCP_QUALIFIED_TOOL_PREFIX) => spec_allowed
            .iter()
            .any(|tool| canonical_mcp_rule_matches_tool(rule, tool)),
        _ if rule.contains(['(', ')']) => false,
        _ => spec_allowed
            .iter()
            .any(|tool| tool.trim().eq_ignore_ascii_case(rule)),
    }
}

#[must_use]
fn tool_supports_read_permission(tool: &str) -> bool {
    matches!(
        tool.trim(),
        tools::CODE_SEARCH
            | tools::EXEC_COMMAND
            | tools::READ_FILE
            | tools::GREP_FILE
            | tools::LIST_FILES
            | tools::UNIFIED_SEARCH
            | tools::UNIFIED_FILE
    )
}

#[must_use]
fn tool_supports_edit_permission(tool: &str) -> bool {
    matches!(
        tool.trim(),
        tools::EDIT_FILE
            | tools::APPLY_PATCH
            | tools::SEARCH_REPLACE
            | tools::FILE_OP
            | tools::UNIFIED_FILE
    )
}

#[must_use]
fn tool_supports_write_permission(tool: &str) -> bool {
    matches!(
        tool.trim(),
        tools::WRITE_FILE
            | tools::CREATE_FILE
            | tools::DELETE_FILE
            | tools::MOVE_FILE
            | tools::COPY_FILE
            | tools::UNIFIED_FILE
    )
}

#[must_use]
fn tool_supports_bash_permission(tool: &str) -> bool {
    matches!(
        tool.trim(),
        tools::UNIFIED_EXEC
            | tools::SHELL
            | tools::EXEC_COMMAND
            | tools::WRITE_STDIN
            | tools::RUN_PTY_CMD
            | tools::EXEC_PTY_CMD
            | tools::CREATE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::CLOSE_PTY_SESSION
            | tools::SEND_PTY_INPUT
            | tools::READ_PTY_SESSION
            | tools::RESIZE_PTY_SESSION
            | tools::EXECUTE_CODE
    )
}

#[must_use]
fn tool_supports_web_fetch_permission(tool: &str) -> bool {
    matches!(
        tool.trim(),
        tools::WEB_FETCH | tools::FETCH_URL | tools::UNIFIED_SEARCH
    )
}

#[must_use]
fn canonical_mcp_rule_matches_tool(rule: &str, tool: &str) -> bool {
    let Some(rule) = rule.trim().strip_prefix(MCP_QUALIFIED_TOOL_PREFIX) else {
        return false;
    };
    let Some(tool) = tool.trim().strip_prefix(MCP_QUALIFIED_TOOL_PREFIX) else {
        return false;
    };

    match rule.split_once("__") {
        Some((server, "*")) => tool.starts_with(&format!("{server}__")),
        Some(_) => tool == rule,
        None => tool == rule || tool.starts_with(&format!("{rule}__")),
    }
}

// ─── Hook & MCP Merging ─────────────────────────────────────────────────────

fn merge_child_hooks(child: &mut VTCodeConfig, hooks: Option<&HooksConfig>) {
    let Some(hooks) = hooks else {
        return;
    };

    child.hooks.lifecycle.quiet_success_output |= hooks.lifecycle.quiet_success_output;
    child
        .hooks
        .lifecycle
        .session_start
        .extend(hooks.lifecycle.session_start.clone());
    child
        .hooks
        .lifecycle
        .session_end
        .extend(hooks.lifecycle.session_end.clone());
    child
        .hooks
        .lifecycle
        .user_prompt_submit
        .extend(hooks.lifecycle.user_prompt_submit.clone());
    child
        .hooks
        .lifecycle
        .pre_tool_use
        .extend(hooks.lifecycle.pre_tool_use.clone());
    child
        .hooks
        .lifecycle
        .post_tool_use
        .extend(hooks.lifecycle.post_tool_use.clone());
    child
        .hooks
        .lifecycle
        .permission_request
        .extend(hooks.lifecycle.permission_request.clone());
    child
        .hooks
        .lifecycle
        .pre_compact
        .extend(hooks.lifecycle.pre_compact.clone());
    // Unified stop hook merging: stop + task_completion + task_completed
    child.hooks.lifecycle.stop.extend(
        hooks
            .lifecycle
            .stop
            .clone()
            .into_iter()
            .chain(hooks.lifecycle.task_completion.clone())
            .chain(hooks.lifecycle.task_completed.clone()),
    );
    child
        .hooks
        .lifecycle
        .notification
        .extend(hooks.lifecycle.notification.clone());
}

fn merge_child_mcp_servers(
    providers: &mut Vec<McpProviderConfig>,
    parent_providers: &[McpProviderConfig],
    servers: &[SubagentMcpServer],
) {
    for server in servers {
        match server {
            SubagentMcpServer::Named(name) => {
                if providers.iter().any(|provider| provider.name == *name) {
                    continue;
                }
                if let Some(parent_provider) = parent_providers
                    .iter()
                    .find(|provider| provider.name == *name)
                {
                    providers.push(parent_provider.clone());
                }
            }
            SubagentMcpServer::Inline(definition) => {
                for (name, value) in definition {
                    let provider = inline_mcp_provider(name, value);
                    if let Some(provider) = provider {
                        providers.retain(|existing| existing.name != provider.name);
                        providers.push(provider);
                    }
                }
            }
        }
    }
}

fn inline_mcp_provider(name: &str, value: &serde_json::Value) -> Option<McpProviderConfig> {
    let object = value.as_object()?;
    let mut payload = serde_json::Map::with_capacity(object.len().saturating_add(1));
    payload.insert(
        "name".to_string(),
        serde_json::Value::String(name.to_string()),
    );
    for (key, value) in object {
        if key == "type" {
            continue;
        }
        payload.insert(key.clone(), value.clone());
    }
    if payload.contains_key("command") && !payload.contains_key("args") {
        payload.insert("args".to_string(), serde_json::Value::Array(Vec::new()));
    }
    serde_json::from_value(serde_json::Value::Object(payload)).ok()
}

// ─── Instructions Composition ───────────────────────────────────────────────

const FINAL_RESPONSE_CONTRACT: &str = "Return your final response using this exact Markdown contract:\n\n\
## Summary\n\
- [Concise outcome]\n\n\
## Facts\n\
- [Grounded fact]\n\n\
## Touched Files\n\
- [Relative path]\n\n\
## Verification\n\
- [Check performed or still needed]\n\n\
## Open Questions\n\
- [Any unresolved question]\n\n\
Use `- None` for empty sections. Keep it concise and grounded in the work you actually performed.";

const READ_ONLY_TOOL_REMINDER: &str = "Tool reminder: stay inside the exposed read-only tool set for this child. \
Use `code_search` for workspace discovery, structural search, and outline work. Use `list_skills`, `load_skill`, and \
`load_skill_resource` when the task requires repository skills. If these tools are insufficient, report the blocker \
instead of retrying denied calls.";

const READ_ONLY_PLANNING_WORKFLOW_REMINDER: &str = "This delegated agent already runs with a read-only tool surface. \
Do not try to enter or exit planning workflow, do not call hidden mutating tools, and do not retry the same denied tool \
call; adjust strategy or report the blocker instead.";

const WRITE_TOOL_REMINDER: &str = "Tool reminder: use `exec_command` with targeted commands for workspace discovery \
and file reading. Use advanced `code_search` for structural or outline work. When `exec_command` returns a live session, \
continue or poll it with `write_stdin`. Use `exec_command` with `git diff --name-only` or `git diff --stat` when reviewing \
current changes.";

const WRITE_SYNTHESIS_REMINDER: &str = "CRITICAL: After reading files to gather context, you MUST synthesize \
your findings and begin implementation. Do not continue reading additional files. The harness enforces a hard \
read-only budget -- exceeding it terminates your session with no output. \
If you catch yourself reading the same file with different offsets, STOP immediately and write what you have.";

pub fn compose_subagent_instructions(
    spec: &SubagentSpec,
    memory_appendix: Option<String>,
) -> String {
    compose_subagent_runtime_instructions(
        &ResolvedAgentRuntimeView::from_spec(spec),
        memory_appendix,
    )
}

fn compose_subagent_runtime_instructions(
    runtime: &ResolvedAgentRuntimeView,
    memory_appendix: Option<String>,
) -> String {
    let mut sections = Vec::new();
    if !runtime.instructions.trim().is_empty() {
        sections.push(runtime.instructions.trim().to_string());
    }
    sections.push(FINAL_RESPONSE_CONTRACT.to_string());

    if is_runtime_read_only(runtime) {
        sections.push(READ_ONLY_TOOL_REMINDER.to_string());
        sections.push(READ_ONLY_PLANNING_WORKFLOW_REMINDER.to_string());
    } else {
        sections.push(WRITE_TOOL_REMINDER.to_string());
        sections.push(WRITE_SYNTHESIS_REMINDER.to_string());
    }

    if !runtime.skills.is_empty() {
        sections.push(format!(
            "Preloaded skill names: {}. Use their established repository conventions.",
            runtime.skills.join(", ")
        ));
    }
    if let Some(memory_appendix) = memory_appendix
        && !memory_appendix.trim().is_empty()
    {
        sections.push(memory_appendix);
    }
    sections.join("\n\n")
}

fn is_runtime_read_only(runtime: &ResolvedAgentRuntimeView) -> bool {
    runtime.read_only
}

pub fn build_subagent_archive_metadata(
    workspace_root: &Path,
    model: &str,
    provider: &str,
    theme: &str,
    reasoning_effort: &str,
    parent_session_id: &str,
    forked: bool,
) -> SessionArchiveMetadata {
    build_thread_archive_metadata(workspace_root, model, provider, theme, reasoning_effort)
        .with_parent_session_id(parent_session_id.to_string())
        .with_fork_mode(if forked {
            SessionForkMode::FullCopy
        } else {
            SessionForkMode::Summarized
        })
}

// ─── Tool Filtering ─────────────────────────────────────────────────────────

pub fn filter_child_tools(
    spec: &SubagentSpec,
    definitions: Vec<ToolDefinition>,
    read_only: bool,
) -> Vec<ToolDefinition> {
    let allowed = spec.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect::<Vec<_>>()
    });
    let denied = spec
        .disallowed_tools
        .iter()
        .map(|tool| tool.to_ascii_lowercase())
        .collect::<Vec<_>>();

    definitions
        .into_iter()
        .filter(|tool| {
            let name = tool.function_name().to_ascii_lowercase();
            if SUBAGENT_TOOL_NAMES.iter().any(|blocked| *blocked == name) {
                return false;
            }
            if denied.iter().any(|entry| entry == &name) {
                return false;
            }
            if let Some(allowed) = allowed.as_ref()
                && !allowed.iter().any(|entry| entry == &name)
            {
                return false;
            }
            if read_only {
                return NON_MUTATING_TOOL_PREFIXES
                    .iter()
                    .any(|candidate| *candidate == name);
            }
            true
        })
        .collect()
}
#[cfg(test)]
mod slice4_tests {
    use super::{
        READ_ONLY_TOOL_REMINDER, WRITE_TOOL_REMINDER, build_child_config, filter_child_tools,
    };
    use crate::config::VTCodeConfig;
    use crate::config::constants::tools;
    use crate::llm::provider::ToolDefinition;

    fn definition(name: &str) -> ToolDefinition {
        ToolDefinition::function(
            name.to_string(),
            name.to_string(),
            serde_json::json!({"type": "object"}),
        )
    }

    #[test]
    fn explorer_keeps_public_read_tools_through_intersection_and_filtering() {
        let mut parent = VTCodeConfig::default();
        parent.permissions.allow = vec!["Read".to_string()];
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");

        let child = build_child_config(&parent, &spec, "small", None);
        assert_eq!(child.permissions.allow, vec!["Read".to_string()]);

        let filtered = filter_child_tools(
            &spec,
            vec![
                definition(tools::CODE_SEARCH),
                definition(tools::EXEC_COMMAND),
                definition(tools::APPLY_PATCH),
                definition(tools::WRITE_STDIN),
            ],
            spec.is_read_only(),
        );
        let names = filtered
            .iter()
            .map(ToolDefinition::function_name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec![tools::CODE_SEARCH]);
    }

    #[test]
    fn read_only_tool_reminder_names_only_exposed_read_only_tools() {
        for tool in [
            "code_search",
            "list_skills",
            "load_skill",
            "load_skill_resource",
        ] {
            assert!(READ_ONLY_TOOL_REMINDER.contains(&format!("`{tool}`")));
        }
        assert!(!READ_ONLY_TOOL_REMINDER.contains("`exec_command`"));
        assert!(!READ_ONLY_TOOL_REMINDER.contains("`write_stdin`"));
        assert!(!READ_ONLY_TOOL_REMINDER.contains("search_dispatch"));
        assert!(!READ_ONLY_TOOL_REMINDER.contains("command_session"));
    }

    #[test]
    fn writable_tool_reminder_names_public_search_and_execution_tools() {
        assert!(WRITE_TOOL_REMINDER.contains("`exec_command`"));
        assert!(WRITE_TOOL_REMINDER.contains("`write_stdin`"));
        assert!(WRITE_TOOL_REMINDER.contains("`code_search`"));
        assert!(!WRITE_TOOL_REMINDER.contains("search_dispatch"));
        assert!(!WRITE_TOOL_REMINDER.contains("command_session"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::models;
    use vtcode_config::core::permissions::PermissionDefault;
    use vtcode_config::{
        AgentMode, IsolationMode, McpProviderConfig, SubagentMcpServer, SubagentSource,
        SubagentSpec,
    };

    fn test_subagent_spec() -> SubagentSpec {
        SubagentSpec {
            name: "test-agent".to_string(),
            description: "Test agent".to_string(),
            prompt: "Do the thing".to_string(),
            tools: None,
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
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

    #[test]
    fn child_config_uses_lightweight_default_profile() {
        let mut parent = VTCodeConfig::default();
        parent.agent.system_prompt_mode = SystemPromptMode::Specialized;
        parent.agent.tool_documentation_mode = ToolDocumentationMode::Full;
        parent.mcp.providers.push(McpProviderConfig::default());

        let spec = test_subagent_spec();
        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

        assert_eq!(
            child.agent.system_prompt_mode,
            SystemPromptMode::Minimal,
            "subagent should default to Minimal system prompt mode"
        );
        assert_eq!(
            child.agent.tool_documentation_mode,
            ToolDocumentationMode::Minimal,
            "subagent should default to Minimal tool documentation mode"
        );
        assert!(
            child.mcp.providers.is_empty(),
            "subagent should not inherit parent MCP providers unless explicitly requested"
        );
    }

    #[test]
    fn child_config_attaches_explicit_mcp_servers() {
        let mut parent = VTCodeConfig::default();
        parent.mcp.providers.push(McpProviderConfig::default());

        let mut spec = test_subagent_spec();
        parent.mcp.providers[0].name = "context7".to_string();
        spec.mcp_servers = vec![SubagentMcpServer::Named("context7".to_string())];

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);

        assert_eq!(child.mcp.providers.len(), 1);
        assert_eq!(child.mcp.providers[0].name, "context7");
    }

    /// Guard-rail test for the extracted MCP-resolution helper: it must
    /// isolatedly drop every parent provider unless the spec names it.
    #[test]
    fn resolve_child_mcp_providers_drops_unnamed_parent_servers() {
        let mut parent = VTCodeConfig::default();
        parent.mcp.providers.push(McpProviderConfig::default());

        let spec = test_subagent_spec();
        let runtime = ResolvedAgentRuntimeView::from_spec(&spec);

        let providers = resolve_child_mcp_providers(&parent, &runtime);
        assert!(
            providers.is_empty(),
            "parent MCP providers must not leak into the child unless explicitly named"
        );
    }

    #[test]
    fn resolve_child_mcp_providers_keeps_named_parent_server() {
        let mut parent = VTCodeConfig::default();
        parent.mcp.providers.push(McpProviderConfig::default());
        parent.mcp.providers[0].name = "context7".to_string();

        let mut spec = test_subagent_spec();
        spec.mcp_servers = vec![SubagentMcpServer::Named("context7".to_string())];
        let runtime = ResolvedAgentRuntimeView::from_spec(&spec);

        let providers = resolve_child_mcp_providers(&parent, &runtime);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "context7");
    }
}
