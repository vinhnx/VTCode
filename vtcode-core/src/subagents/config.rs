use anyhow::Result;
use std::path::Path;
use vtcode_config::{
    HooksConfig, McpProviderConfig, PermissionMode, SubagentMcpServer, SubagentSpec,
};

use super::constants::{NON_MUTATING_TOOL_PREFIXES, SUBAGENT_MIN_MAX_TURNS, SUBAGENT_TOOL_NAMES};
use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::ReasoningEffortLevel;
use crate::core::threads::build_thread_archive_metadata;
use crate::llm::provider::ToolDefinition;
use crate::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX;
use crate::utils::session_archive::{SessionArchiveMetadata, SessionForkMode};

// ─── Child Config Building ─────────────────────────────────────────────────

pub fn build_child_config(
    parent: &VTCodeConfig,
    spec: &SubagentSpec,
    model: &str,
    max_turns: Option<usize>,
) -> VTCodeConfig {
    let mut child = parent.clone();
    child.agent.default_model = model.to_string();
    if let Some(mode) = spec.permission_mode {
        child.permissions.default_mode =
            clamp_permission_mode(parent.permissions.default_mode, mode);
    }
    if let Some(max_turns) = normalize_child_max_turns(max_turns) {
        child.automation.full_auto.max_turns = max_turns;
    }

    let mut allowed_tools = spec.tools.clone().unwrap_or_default();
    if !allowed_tools.is_empty() {
        allowed_tools.retain(|tool| !SUBAGENT_TOOL_NAMES.iter().any(|blocked| blocked == tool));
        child.permissions.allow =
            intersect_allowed_tools(&parent.permissions.allow, &allowed_tools);
    }

    let mut disallowed_tools = parent.permissions.deny.clone();
    disallowed_tools.extend(spec.disallowed_tools.clone());
    for tool in SUBAGENT_TOOL_NAMES {
        if !disallowed_tools.iter().any(|entry| entry == tool) {
            disallowed_tools.push((*tool).to_string());
        }
    }
    child.permissions.deny = disallowed_tools;
    merge_child_hooks(&mut child, spec.hooks.as_ref());
    merge_child_mcp_servers(&mut child, spec.mcp_servers.as_slice());
    child
}

pub fn normalize_child_max_turns(max_turns: Option<usize>) -> Option<usize> {
    max_turns.map(|value| value.max(SUBAGENT_MIN_MAX_TURNS))
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
    let resolved_model = resolve_model(
        parent,
        parent_model,
        parent_provider,
        model_override,
        spec.model.as_deref(),
        spec.name.as_str(),
    )?;
    let mut child_cfg = build_child_config(parent, spec, resolved_model.as_str(), max_turns);
    let child_reasoning_effort = reasoning_override
        .and_then(ReasoningEffortLevel::parse)
        .or_else(|| {
            spec.reasoning_effort
                .as_deref()
                .and_then(ReasoningEffortLevel::parse)
        })
        .unwrap_or(parent_reasoning_effort);
    child_cfg.agent.default_model = resolved_model.to_string();
    child_cfg.agent.reasoning_effort = child_reasoning_effort;
    Ok((resolved_model, child_reasoning_effort, child_cfg))
}

// ─── Permission Handling ────────────────────────────────────────────────────

fn clamp_permission_mode(parent: PermissionMode, requested: PermissionMode) -> PermissionMode {
    if matches!(
        parent,
        PermissionMode::Auto | PermissionMode::BypassPermissions
    ) {
        return parent;
    }
    if permission_rank(requested) <= permission_rank(parent) {
        requested
    } else {
        parent
    }
}

fn permission_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::DontAsk => 0,
        PermissionMode::Plan => 1,
        PermissionMode::Default => 2,
        PermissionMode::AcceptEdits => 3,
        PermissionMode::Auto => 4,
        PermissionMode::BypassPermissions => 5,
    }
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
        tools::READ_FILE
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

fn merge_child_mcp_servers(child: &mut VTCodeConfig, servers: &[SubagentMcpServer]) {
    for server in servers {
        match server {
            SubagentMcpServer::Named(name) => {
                if child
                    .mcp
                    .providers
                    .iter()
                    .any(|provider| provider.name == *name)
                {
                    continue;
                }
            }
            SubagentMcpServer::Inline(definition) => {
                for (name, value) in definition {
                    let provider = inline_mcp_provider(name, value);
                    if let Some(provider) = provider {
                        child
                            .mcp
                            .providers
                            .retain(|existing| existing.name != provider.name);
                        child.mcp.providers.push(provider);
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
        payload.insert(key.clone(), value.clone());
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
Do not guess hidden or legacy helpers such as `list_files`, `read_file`, `unified_file`, or `unified_exec` when they \
are not visible. For workspace discovery here, prefer `unified_search`; if that is insufficient, report the blocker \
instead of retrying denied calls.";

const READ_ONLY_PLAN_MODE_REMINDER: &str = "This delegated agent already runs with a read-only tool surface. \
Do not try to enter or exit plan mode, do not call hidden mutating tools, and do not retry the same denied tool \
call; adjust strategy or report the blocker instead.";

const WRITE_TOOL_REMINDER: &str = "Tool reminder: `list_files` on the workspace root (`.`) is blocked, and \
`list_files` already uses search internally. Do not pair `list_files` with `unified_search` in the same batch. \
Use a specific subdirectory, `unified_search` for workspace-wide discovery, or `unified_exec` with \
`git diff --name-only` / `git diff --stat` when reviewing current changes.";

pub fn compose_subagent_instructions(
    spec: &SubagentSpec,
    memory_appendix: Option<String>,
) -> String {
    let mut sections = Vec::new();
    if !spec.prompt.trim().is_empty() {
        sections.push(spec.prompt.trim().to_string());
    }
    sections.push(FINAL_RESPONSE_CONTRACT.to_string());

    if spec.is_read_only() {
        sections.push(READ_ONLY_TOOL_REMINDER.to_string());
        sections.push(READ_ONLY_PLAN_MODE_REMINDER.to_string());
    } else {
        sections.push(WRITE_TOOL_REMINDER.to_string());
    }

    if !spec.skills.is_empty() {
        sections.push(format!(
            "Preloaded skill names: {}. Use their established repository conventions.",
            spec.skills.join(", ")
        ));
    }
    if let Some(memory_appendix) = memory_appendix
        && !memory_appendix.trim().is_empty()
    {
        sections.push(memory_appendix);
    }
    sections.join("\n\n")
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
