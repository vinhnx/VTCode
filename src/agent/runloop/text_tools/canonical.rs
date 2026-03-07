use serde_json::Value;
use vtcode_core::config::constants::tools;

pub(super) const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];
pub(super) const DIRECT_FUNCTION_ALIASES: &[&str] = &[
    "run",
    "run_cmd",
    "runcommand",
    "terminalrun",
    "terminal_cmd",
    "terminalcommand",
];

pub(super) fn canonicalize_tool_name(raw: &str) -> Option<String> {
    let normalized = canonicalize_normalized_name(raw)?;
    if normalized.is_empty() {
        None
    } else if default_unified_exec_action(&normalized).is_some() {
        Some(tools::UNIFIED_EXEC.to_string())
    } else {
        Some(normalized)
    }
}

pub(super) fn canonicalize_tool_result(name: String, mut args: Value) -> Option<(String, Value)> {
    let normalized = canonicalize_normalized_name(&name)?;
    let canonical = canonicalize_tool_name(&name)?;
    if canonical == tools::UNIFIED_EXEC
        && let Some(action) = default_unified_exec_action(&normalized)
        && let Some(payload) = args.as_object_mut()
        && !payload.contains_key("action")
    {
        payload.insert("action".to_string(), Value::String(action.to_string()));
    }
    if is_known_textual_tool(&canonical) {
        Some((canonical, args))
    } else {
        None
    }
}

pub(super) fn is_known_textual_tool(name: &str) -> bool {
    matches!(
        name,
        tools::WRITE_FILE
            | tools::EDIT_FILE
            | tools::READ_FILE
            | tools::UNIFIED_EXEC
            | tools::RUN_PTY_CMD
            | "grep_file"
            | "list_files"
            | tools::APPLY_PATCH
            | tools::READ_PTY_SESSION
            | tools::SEND_PTY_INPUT
            | tools::RESIZE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::CLOSE_PTY_SESSION
            | tools::CREATE_PTY_SESSION
    )
}

fn canonicalize_normalized_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));

    let mut normalized = String::with_capacity(trimmed.len());
    let mut last_was_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if ch == '_' {
            normalized.push('_');
            last_was_separator = false;
        } else if matches!(ch, ' ' | '\t' | '\n' | '-' | ':' | '.')
            && !last_was_separator
            && !normalized.is_empty()
        {
            normalized.push('_');
            last_was_separator = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn default_unified_exec_action(normalized: &str) -> Option<&'static str> {
    if matches!(
        normalized,
        "run"
            | "runcmd"
            | "runcommand"
            | "terminalrun"
            | "terminalcmd"
            | "terminalcommand"
            | "command"
            | "run_pty_cmd"
            | "shell"
            | "bash"
            | "container_exec"
            | "exec"
            | "exec_command"
    ) {
        Some("run")
    } else {
        None
    }
}
