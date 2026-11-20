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

// Tool-name hygiene is shared infrastructure (`vtcode_commons::text_fence`).
pub(crate) use vtcode_commons::text_fence::{is_clean_tool_name, is_dispatchable_tool_name};

/// Map well-known shell aliases used by gateway models onto `exec_command`.
///
/// Returns `None` when the name is not a shell alias (leave it unchanged).
/// Intentionally does **not** map PTY-family names (`run_pty_cmd`,
/// `create_pty_session`, …) — those keep their own tool identity.
pub(crate) fn canonicalize_shell_tool_alias(raw: &str) -> Option<String> {
    let normalized = canonicalize_normalized_name(raw)?;
    matches!(
        normalized.as_str(),
        "run"
            | "runcmd"
            | "runcommand"
            | "terminalrun"
            | "terminalcmd"
            | "terminalcommand"
            | "command"
            | tools::SHELL
            | tools::BASH_ALIAS
            | "container_exec"
            | tools::EXEC_ALIAS
            | tools::EXEC_COMMAND
    )
    .then(|| tools::EXEC_COMMAND.to_string())
}

#[derive(Clone, Copy)]
pub(super) struct ExecCommandDefaults {
    pub(super) action: &'static str,
    pub(super) force_tty: Option<bool>,
}

/// Map an already-normalized name to its canonical dispatch name.
///
/// Reuses the normalized allocation in the common (non-exec) case. Callers must
/// normalize first via `canonicalize_normalized_name`, which also guarantees
/// non-empty input.
fn canonicalize_from_normalized_name(normalized: String) -> String {
    if exec_command_defaults_from_normalized_name(&normalized).is_some() {
        tools::EXEC_COMMAND.to_owned()
    } else {
        normalized
    }
}

pub(super) fn canonicalize_tool_name(raw: &str) -> Option<String> {
    canonicalize_normalized_name(raw).map(canonicalize_from_normalized_name)
}

/// Canonicalize a tool call and optionally validate against known tools.
///
/// When `validate` is true, the result is checked against the known-tool allowlist.
/// When `validate` is false, only canonicalisation and exec_command defaults are applied.
pub(super) fn canonicalize_tool_result(name: String, mut args: Value, validate: bool) -> Option<(String, Value)> {
    // Normalize once and reuse the result: `canonicalize_tool_name` used to be
    // called here, which re-normalized the very same string a second time.
    let normalized = canonicalize_normalized_name(&name)?;
    let defaults = exec_command_defaults_from_normalized_name(&normalized);
    let canonical = canonicalize_from_normalized_name(normalized);
    if let Some(defaults) = defaults
        && let Some(payload) = args.as_object_mut()
    {
        apply_exec_command_defaults(payload, defaults);
    }
    if validate && !is_known_textual_tool(&canonical) {
        return None;
    }
    Some((canonical, args))
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
            | tools::CREATE_FILE
            | tools::DELETE_FILE
            | tools::SEARCH_REPLACE
            | tools::UNIFIED_FILE
            | tools::TASK_TRACKER
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
        "run"
        | "runcmd"
        | "runcommand"
        | "terminalrun"
        | "terminalcmd"
        | "terminalcommand"
        | "command"
        | tools::SHELL
        | tools::BASH_ALIAS
        | "container_exec"
        | tools::EXEC_ALIAS
        | tools::EXEC_COMMAND => Some(ExecCommandDefaults { action: "run", force_tty: None }),
        tools::RUN_PTY_CMD | tools::CREATE_PTY_SESSION => {
            Some(ExecCommandDefaults { action: "run", force_tty: Some(true) })
        }
        tools::SEND_PTY_INPUT => Some(ExecCommandDefaults { action: "write", force_tty: None }),
        tools::READ_PTY_SESSION => Some(ExecCommandDefaults { action: "poll", force_tty: None }),
        tools::LIST_PTY_SESSIONS => Some(ExecCommandDefaults { action: "list", force_tty: None }),
        tools::CLOSE_PTY_SESSION => Some(ExecCommandDefaults { action: "close", force_tty: None }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_name_canonicalization_preserves_behavior() {
        // Shell aliases fold to exec_command; ordinary names keep normalized
        // form; empty input stays a miss.
        assert_eq!(canonicalize_tool_name("run").as_deref(), Some(tools::EXEC_COMMAND));
        assert_eq!(canonicalize_tool_name("  RUN  ").as_deref(), Some(tools::EXEC_COMMAND));
        assert_eq!(canonicalize_tool_name("Write File").as_deref(), Some("write_file"));
        assert_eq!(canonicalize_tool_name("default_api.read_file").as_deref(), Some("default_api_read_file"));
        assert_eq!(canonicalize_tool_name("   "), None);
    }

    #[test]
    fn tool_result_normalizes_once_and_applies_exec_defaults() {
        // A shell alias is normalized to exec_command and gets the default
        // action, exactly as before the single-normalization refactor.
        let (name, args) = canonicalize_tool_result("run".to_owned(), json!({}), false).expect("exec result");
        assert_eq!(name, tools::EXEC_COMMAND);
        assert_eq!(args["action"], "run");

        // An explicit action is preserved (`entry().or_insert_with`).
        let (_, args) =
            canonicalize_tool_result("run".to_owned(), json!({"action": "poll"}), false).expect("exec result");
        assert_eq!(args["action"], "poll");

        // Non-exec tools are canonicalized but receive no defaults.
        let (name, args) = canonicalize_tool_result("Write File".to_owned(), json!({}), false).expect("write result");
        assert_eq!(name, "write_file");
        assert!(args.get("action").is_none());
    }

    #[test]
    fn tool_result_validation_still_gates_unknown_names() {
        assert!(canonicalize_tool_result("totally_unknown".to_owned(), json!({}), true).is_none());
        let (name, _) =
            canonicalize_tool_result("totally_unknown".to_owned(), json!({}), false).expect("unvalidated result");
        assert_eq!(name, "totally_unknown");
    }
}
