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
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));

    // Pre-allocate string with estimated capacity (same as input length)
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
    } else if matches!(
        normalized.as_str(),
        "run"
            | "runcmd"
            | "runcommand"
            | "terminalrun"
            | "terminalcmd"
            | "terminalcommand"
            | "command"
    ) {
        Some(tools::RUN_PTY_CMD.to_string())
    } else {
        Some(normalized)
    }
}

pub(super) fn canonicalize_tool_result(name: String, args: Value) -> Option<(String, Value)> {
    let canonical = canonicalize_tool_name(&name)?;
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
            | tools::RUN_PTY_CMD
            | tools::GREP_FILE
            | tools::LIST_FILES
            | tools::APPLY_PATCH
            | tools::READ_PTY_SESSION
            | tools::SEND_PTY_INPUT
            | tools::RESIZE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::CLOSE_PTY_SESSION
            | tools::CREATE_PTY_SESSION
    )
}
