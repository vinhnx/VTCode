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
pub(super) struct UnifiedExecDefaults {
    pub(super) action: &'static str,
    pub(super) force_tty: Option<bool>,
}

pub(super) fn canonicalize_tool_name(raw: &str) -> Option<String> {
    let normalized = canonicalize_normalized_name(raw)?;
    if normalized.is_empty() {
        None
    } else if unified_exec_defaults_from_normalized_name(&normalized).is_some() {
        Some(tools::UNIFIED_EXEC.to_string())
    } else {
        Some(normalized)
    }
}

pub(super) fn canonicalize_tool_result(name: String, mut args: Value) -> Option<(String, Value)> {
    let normalized = canonicalize_normalized_name(&name)?;
    let canonical = canonicalize_tool_name(&name)?;
    if canonical == tools::UNIFIED_EXEC
        && let Some(defaults) = unified_exec_defaults_from_normalized_name(&normalized)
        && let Some(payload) = args.as_object_mut()
    {
        apply_unified_exec_defaults(payload, defaults);
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
            | tools::UNIFIED_FILE
            | tools::UNIFIED_EXEC
            | tools::UNIFIED_SEARCH
            | "grep_file"
            | "list_files"
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

pub(super) fn unified_exec_defaults_for_name(raw: &str) -> Option<UnifiedExecDefaults> {
    canonicalize_normalized_name(raw)
        .and_then(|normalized| unified_exec_defaults_from_normalized_name(normalized.as_str()))
}

pub(super) fn apply_unified_exec_defaults(
    payload: &mut serde_json::Map<String, Value>,
    defaults: UnifiedExecDefaults,
) {
    payload
        .entry("action".to_string())
        .or_insert_with(|| Value::String(defaults.action.to_string()));
    if let Some(tty) = defaults.force_tty {
        payload
            .entry("tty".to_string())
            .or_insert_with(|| Value::Bool(tty));
    }
}

fn unified_exec_defaults_from_normalized_name(normalized: &str) -> Option<UnifiedExecDefaults> {
    match normalized {
        tools::UNIFIED_EXEC => Some(UnifiedExecDefaults {
            action: "run",
            force_tty: None,
        }),
        "run" | "runcmd" | "runcommand" | "terminalrun" | "terminalcmd" | "terminalcommand"
        | "command" | "shell" | "bash" | "container_exec" | "exec" | "exec_command" => {
            Some(UnifiedExecDefaults {
                action: "run",
                force_tty: None,
            })
        }
        "run_pty_cmd" | "create_pty_session" => Some(UnifiedExecDefaults {
            action: "run",
            force_tty: Some(true),
        }),
        "send_pty_input" => Some(UnifiedExecDefaults {
            action: "write",
            force_tty: None,
        }),
        "read_pty_session" => Some(UnifiedExecDefaults {
            action: "poll",
            force_tty: None,
        }),
        "list_pty_sessions" => Some(UnifiedExecDefaults {
            action: "list",
            force_tty: None,
        }),
        "close_pty_session" => Some(UnifiedExecDefaults {
            action: "close",
            force_tty: None,
        }),
        _ => None,
    }
}
