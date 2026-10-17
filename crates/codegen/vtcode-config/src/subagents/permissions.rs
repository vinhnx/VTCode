//! Subagent tool-name normalization and permission-mutation analysis.

use crate::constants::tools;
use crate::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use crate::core::tools::ToolPolicy;

use super::SubagentSpec;

pub(super) fn normalize_subagent_tool_list(tools: Option<Vec<String>>) -> Option<Vec<String>> {
    tools.map(normalize_subagent_tools)
}

pub(super) fn normalize_subagent_tools(tools: Vec<String>) -> Vec<String> {
    let mut normalized: Vec<String> = Vec::with_capacity(tools.len());
    for tool in tools {
        let trimmed = tool.trim();
        let mapped_names = normalize_subagent_tool_name(trimmed);
        if mapped_names.is_empty() {
            if !trimmed.is_empty() && !normalized.iter().any(|existing| existing.eq_ignore_ascii_case(trimmed)) {
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

pub(super) fn normalize_subagent_tool_name(tool: &str) -> &'static [&'static str] {
    let normalized = tool.trim().to_ascii_lowercase();
    let normalized = normalized.strip_suffix("(*)").map(str::trim).unwrap_or(normalized.as_str());

    match normalized {
        "read" | "read_file" | "readfile" | "grep" | "grep_file" | "grepfile" | "glob" | "list" | "list_file"
        | "list_files" | "listfile" | "listfiles" | "list-files" => &[tools::EXEC_COMMAND],
        "code_search" => &[tools::CODE_SEARCH],
        "write" | "edit" | "multiedit" | "multi_edit" | "multi-edit" => &[tools::APPLY_PATCH],
        "bash" | "shell" | "command" | "exec_command" => &[tools::EXEC_COMMAND],
        "patch" | "applypatch" | "apply_patch" => &[tools::APPLY_PATCH],
        "agent" | "task" => &[tools::SPAWN_AGENT],
        "askuserquestion" | "ask_user_question" | "requestuserinput" | "request_user_input" => {
            &[tools::REQUEST_USER_INPUT]
        }
        _ => &[],
    }
}

pub(super) fn is_mutating_tool_name(tool: &str) -> bool {
    tool == "edit"
        || tool == "bash"
        || tool == "shell"
        || tool == "command"
        || tool == "write"
        || tool == tools::EXEC_COMMAND
        || tool == tools::APPLY_PATCH
        || tool == tools::UNIFIED_EXEC
        || tool == tools::EDIT_FILE
        || tool == tools::WRITE_FILE
        || tool == tools::UNIFIED_FILE
        || tool == tools::CREATE_FILE
        || tool == tools::DELETE_FILE
        || tool == tools::MOVE_FILE
        || tool == tools::COPY_FILE
        || tool == tools::SEARCH_REPLACE
}

pub(super) fn permission_rule_allows_mutation(rule: &str) -> bool {
    let tool_name = permission_rule_tool_name(rule);
    let normalized = tool_name.to_ascii_lowercase();
    if is_mutating_tool_name(normalized.as_str()) {
        return true;
    }

    // Read-only aliases (read/grep/glob/list/code_search) collapse onto
    // `exec_command` for routing, but they never mutate. Guard them before the
    // normalized lookup so plugin restrictions retain read-only permissions.
    if is_read_only_tool_alias(normalized.as_str()) {
        return false;
    }

    normalize_subagent_tool_name(tool_name)
        .iter()
        .any(|tool| is_mutating_tool_name(tool))
}

pub(super) fn is_read_only_tool_alias(tool: &str) -> bool {
    matches!(
        tool,
        "read"
            | "read_file"
            | "readfile"
            | "grep"
            | "grep_file"
            | "grepfile"
            | "glob"
            | "list"
            | "list_file"
            | "list_files"
            | "listfile"
            | "listfiles"
            | "list-files"
            | "code_search"
    )
}

pub(super) fn permission_rule_tool_name(rule: &str) -> &str {
    let trimmed = rule.trim();
    if let Some((tool_name, specifier)) = trimmed.split_once('(')
        && specifier.trim_end().ends_with(')')
    {
        return tool_name.trim();
    }
    trimmed
}

/// Check whether a tool name is a legacy internal name that should be expressed
/// as a semantic rule instead (e.g., `"read"` instead of `"read_file"`).
pub(super) fn is_legacy_tool_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "read_file"
            | "write_file"
            | "edit_file"
            | "create_file"
            | "delete_file"
            | "move_file"
            | "copy_file"
            | "search_replace"
            | "file_op"
            | "grep_file"
            | "list_files"
            | "run_pty_cmd"
            | "execute_code"
    )
}

/// Generate warnings for permission rules that use legacy tool names instead of
/// semantic rules.
pub(super) fn warn_legacy_permission_rules(permissions: &AgentPermissionsConfig) -> Vec<String> {
    let mut warnings = Vec::new();
    for rule in permissions
        .allow
        .iter()
        .chain(permissions.ask.iter())
        .chain(permissions.auto.iter())
        .chain(permissions.deny.iter())
    {
        let tool_name = permission_rule_tool_name(rule);
        if is_legacy_tool_name(tool_name) {
            // Normalize the full rule (including path specifier) so the
            // suggestion preserves any path constraints the user specified.
            let normalized = crate::core::permissions::normalize_permission_rule(rule);
            warnings.push(format!(
                "permission rule '{rule}' uses legacy tool name '{tool_name}'; \
                 consider using the semantic rule '{normalized}' for clearer intent"
            ));
        }
    }
    warnings
}

pub(super) fn apply_plugin_restrictions(spec: &mut SubagentSpec) {
    if spec.hooks.take().is_some() {
        spec.warnings.push("plugin subagent hooks are ignored for safety".to_string());
    }
    if !spec.mcp_servers.is_empty() {
        spec.mcp_servers.clear();
        spec.warnings
            .push("plugin subagent mcp_servers are ignored for safety".to_string());
    }
    let default_restricted = matches!(spec.permissions.default, PermissionDefault::Allow | PermissionDefault::Auto);
    if default_restricted {
        spec.permissions.default = PermissionDefault::Ask;
    }

    let allow_len = spec.permissions.allow.len();
    spec.permissions.allow.retain(|rule| !permission_rule_allows_mutation(rule));
    let auto_len = spec.permissions.auto.len();
    spec.permissions.auto.retain(|rule| !permission_rule_allows_mutation(rule));

    if default_restricted || spec.permissions.allow.len() != allow_len || spec.permissions.auto.len() != auto_len {
        spec.warnings
            .push("plugin subagent permission overrides are restricted for safety".to_string());
    }
}
