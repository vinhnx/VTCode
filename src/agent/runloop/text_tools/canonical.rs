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

#[derive(Clone, Copy)]
pub(super) struct ExecCommandDefaults {
    pub(super) action: &'static str,
    pub(super) force_tty: Option<bool>,
}

pub(super) fn canonicalize_tool_name(raw: &str) -> Option<String> {
    let normalized = canonicalize_normalized_name(raw)?;
    if normalized.is_empty() {
        None
    } else if exec_command_defaults_from_normalized_name(&normalized).is_some() {
        Some(tools::EXEC_COMMAND.to_string())
    } else {
        Some(normalized)
    }
}

/// Canonicalize a tool call and optionally validate against known tools.
///
/// When `validate` is true, the result is checked against the known-tool allowlist.
/// When `validate` is false, only canonicalisation and exec_command defaults are applied.
pub(super) fn canonicalize_tool_result(name: String, mut args: Value, validate: bool) -> Option<(String, Value)> {
    let normalized = canonicalize_normalized_name(&name)?;
    let canonical = canonicalize_tool_name(&name)?;
    if canonical == tools::EXEC_COMMAND
        && let Some(defaults) = exec_command_defaults_from_normalized_name(&normalized)
        && let Some(payload) = args.as_object_mut()
    {
        apply_exec_command_defaults(payload, defaults);
    }
    if validate {
        if is_known_textual_tool(&canonical) {
            Some((canonical, args))
        } else {
            None
        }
    } else {
        Some((canonical, args))
    }
}

pub(crate) fn is_known_textual_tool(name: &str) -> bool {
    matches!(
        name,
        tools::WRITE_FILE
            | tools::EDIT_FILE
            | tools::READ_FILE
            | tools::EXEC_COMMAND
            | tools::WRITE_STDIN
            | tools::CODE_SEARCH
            | tools::GREP_FILE
            | tools::LIST_FILES
            | tools::APPLY_PATCH
            | tools::RESIZE_PTY_SESSION
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
        } else if matches!(ch, ' ' | '\t' | '\n' | '-' | ':' | '.') && !last_was_separator && !normalized.is_empty() {
            normalized.push('_');
            last_was_separator = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_string();
    if normalized.is_empty() { None } else { Some(normalized) }
}

pub(super) fn exec_command_defaults_for_name(raw: &str) -> Option<ExecCommandDefaults> {
    canonicalize_normalized_name(raw)
        .and_then(|normalized| exec_command_defaults_from_normalized_name(normalized.as_str()))
}

pub(super) fn apply_exec_command_defaults(payload: &mut serde_json::Map<String, Value>, defaults: ExecCommandDefaults) {
    payload
        .entry("action".to_string())
        .or_insert_with(|| Value::String(defaults.action.to_string()));
    if let Some(tty) = defaults.force_tty {
        payload.entry("tty".to_string()).or_insert_with(|| Value::Bool(tty));
    }
}

fn exec_command_defaults_from_normalized_name(normalized: &str) -> Option<ExecCommandDefaults> {
    match normalized {
        "run" | "runcmd" | "runcommand" | "terminalrun" | "terminalcmd" | "terminalcommand" | "command" | "shell"
        | "bash" | "container_exec" | "exec" | "exec_command" => {
            Some(ExecCommandDefaults { action: "run", force_tty: None })
        }
        "run_pty_cmd" | "create_pty_session" => Some(ExecCommandDefaults { action: "run", force_tty: Some(true) }),
        "send_pty_input" => Some(ExecCommandDefaults { action: "write", force_tty: None }),
        "read_pty_session" => Some(ExecCommandDefaults { action: "poll", force_tty: None }),
        "list_pty_sessions" => Some(ExecCommandDefaults { action: "list", force_tty: None }),
        "close_pty_session" => Some(ExecCommandDefaults { action: "close", force_tty: None }),
        _ => None,
    }
}
