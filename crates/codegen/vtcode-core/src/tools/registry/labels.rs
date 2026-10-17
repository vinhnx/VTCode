use std::borrow::Cow;

use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::mcp::legacy_mcp_tool_name;
use crate::tools::tool_intent;

pub fn tool_action_label(tool_name: &str, args: &Value) -> Cow<'static, str> {
    if tool_intent::is_command_run_tool_call(tool_name, args) {
        return Cow::Borrowed("Run command");
    }

    let actual_tool_name = normalize_tool_name(tool_name);

    match actual_tool_name {
        name if name == tool_names::EXEC_COMMAND => Cow::Borrowed("Run command"),
        name if name == tool_names::WRITE_STDIN => Cow::Borrowed(write_stdin_action_label(args)),
        name if name == tool_names::RUN_PTY_CMD => Cow::Borrowed("Run command"),
        name if name == tool_names::EXECUTE_CODE => Cow::Borrowed("Run code"),
        name if name == tool_names::GET_ERRORS => Cow::Borrowed("List errors"),
        name if name == tool_names::MCP => Cow::Borrowed("MCP discovery"),
        name if name == tool_names::MCP_CONNECT_SERVER => Cow::Borrowed("Connect MCP server"),
        name if name == tool_names::MCP_DISCONNECT_SERVER => Cow::Borrowed("Disconnect MCP server"),
        name if name == tool_names::LIST_SKILLS => Cow::Borrowed("List skills"),
        name if name == tool_names::LOAD_SKILL => Cow::Borrowed("Load skill"),
        name if name == tool_names::LOAD_SKILL_RESOURCE => Cow::Borrowed("Load skill resource"),
        name if name == tool_names::READ_FILE => Cow::Borrowed("Read file"),
        name if name == tool_names::WRITE_FILE => Cow::Borrowed("Write file"),
        name if name == tool_names::EDIT_FILE => Cow::Borrowed("Edit file"),
        name if name == tool_names::CREATE_FILE => Cow::Borrowed("Create file"),
        name if name == tool_names::DELETE_FILE => Cow::Borrowed("Delete file"),
        name if name == tool_names::APPLY_PATCH => Cow::Borrowed("Apply patch"),
        name if name == tool_names::SEARCH_REPLACE => Cow::Borrowed("Search/replace"),
        name if name == tool_names::CREATE_PTY_SESSION => Cow::Borrowed("Create command session"),
        name if name == tool_names::READ_PTY_SESSION => Cow::Borrowed("Read command session"),
        name if name == tool_names::LIST_PTY_SESSIONS => Cow::Borrowed("List command sessions"),
        name if name == tool_names::SEND_PTY_INPUT => Cow::Borrowed("Send command input"),
        name if name == tool_names::CLOSE_PTY_SESSION => Cow::Borrowed("Close command session"),
        name if name == tool_names::RESIZE_PTY_SESSION => Cow::Borrowed("Resize command session"),
        name if name == tool_names::UNIFIED_EXEC => Cow::Borrowed(unified_exec_action_label(args)),
        name if name == tool_names::CODE_SEARCH => Cow::Borrowed("Search code"),
        name if name == tool_names::UNIFIED_FILE => match tool_intent::file_operation_action(args).unwrap_or("read") {
            "read" => Cow::Borrowed("Read file"),
            "write" => Cow::Borrowed("Write file"),
            "edit" => Cow::Borrowed("Edit file"),
            "patch" | tool_names::APPLY_PATCH => Cow::Borrowed("Apply patch"),
            "delete" => Cow::Borrowed("Delete file"),
            "move" => Cow::Borrowed("Move file"),
            "copy" => Cow::Borrowed("Copy file"),
            _ => Cow::Borrowed("File operation"),
        },
        "fetch" | tool_names::WEB_FETCH | tool_names::FETCH_URL | tool_names::DEFUDDLE_FETCH => Cow::Borrowed("Fetch"),
        _ => Cow::Owned(humanize_tool_name(actual_tool_name)),
    }
}

pub fn unified_exec_action_label(args: &Value) -> &'static str {
    let action = tool_intent::command_session_action(args).unwrap_or("run");
    if action.eq_ignore_ascii_case("run") {
        "Run command"
    } else if action.eq_ignore_ascii_case("write") {
        "Send command input"
    } else if action.eq_ignore_ascii_case("poll") {
        "Read command session"
    } else if action.eq_ignore_ascii_case("wait") {
        "Wait for command session"
    } else if action.eq_ignore_ascii_case("continue") {
        "Continue command session"
    } else if action.eq_ignore_ascii_case("inspect") {
        "Inspect command output"
    } else if action.eq_ignore_ascii_case("list") {
        "List command sessions"
    } else if action.eq_ignore_ascii_case("close") {
        "Close command session"
    } else if action.eq_ignore_ascii_case("code") {
        "Run code"
    } else {
        "Exec action"
    }
}

pub fn write_stdin_action_label(args: &Value) -> &'static str {
    if let Some(action) = args.get("action").and_then(Value::as_str) {
        if action.eq_ignore_ascii_case("wait") {
            return "Wait for command session";
        }
        if action.eq_ignore_ascii_case("poll") {
            return "Read command session";
        }
        if action.eq_ignore_ascii_case("inspect") {
            return "Inspect command output";
        }
        if action.eq_ignore_ascii_case("close") {
            return "Close command session";
        }
        if action.eq_ignore_ascii_case("terminate") {
            return "Terminate command session";
        }
        if action.eq_ignore_ascii_case("write") {
            // Match `write_stdin_dispatch`: explicit write with empty/missing
            // input polls instead of sending, so label it as a read.
            return if has_session_input(args) {
                "Send command input"
            } else {
                "Read command session"
            };
        }
    }
    if has_session_input(args) {
        "Send command input"
    } else {
        // A session_id-only follow-up polls output; calling it "Send" misleads
        // (screenshot 2026-10-02: pure waits rendered as sends).
        "Read command session"
    }
}

fn has_session_input(args: &Value) -> bool {
    ["chars", "input", "text"]
        .iter()
        .any(|key| args.get(key).and_then(Value::as_str).is_some_and(|s| !s.is_empty()))
}

fn normalize_tool_name(tool_name: &str) -> &str {
    if let Some(stripped) = legacy_mcp_tool_name(tool_name) {
        return stripped;
    }
    if tool_name.starts_with("mcp__") {
        return tool_name.split("__").last().unwrap_or(tool_name);
    }
    if tool_name.starts_with("mcp::") {
        return tool_name.split("::").last().unwrap_or(tool_name);
    }
    tool_name
}

fn humanize_tool_name(name: &str) -> String {
    let replaced = name.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

#[cfg(test)]
mod tests {
    use super::tool_action_label;
    use crate::config::constants::tools;
    use serde_json::json;

    #[test]
    fn code_search_uses_stable_label() {
        assert_eq!(tool_action_label(tools::CODE_SEARCH, &json!({"query": "Widget"})), "Search code");
    }

    #[test]
    fn write_stdin_distinguishes_wait_from_send() {
        assert_eq!(
            tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-wait-1", "action": "wait"})),
            "Wait for command session"
        );
        assert_eq!(
            tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-send-2", "chars": "y\n"})),
            "Send command input"
        );
        assert_eq!(tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-poll-3"})), "Read command session");
    }

    #[test]
    fn unified_exec_wait_is_case_insensitive() {
        for action in ["wait", "Wait", "WAIT", "WaIt"] {
            assert_eq!(
                tool_action_label(tools::UNIFIED_EXEC, &json!({"session_id": "run-abc", "action": action})),
                "Wait for command session",
                "action {action} must map case-insensitively"
            );
        }
    }

    #[test]
    fn write_stdin_explicit_action_wins_over_chars() {
        let chars = "y\n";
        assert_eq!(
            tool_action_label(
                tools::WRITE_STDIN,
                &json!({"session_id": "run-explicit-wait", "action": "wait", "chars": chars})
            ),
            "Wait for command session"
        );
        assert_eq!(
            tool_action_label(
                tools::WRITE_STDIN,
                &json!({"session_id": "run-explicit-write", "action": "write", "chars": chars})
            ),
            "Send command input"
        );
    }

    #[test]
    fn write_stdin_empty_vs_whitespace_chars_boundary() {
        assert_eq!(
            tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-empty", "chars": ""})),
            "Read command session"
        );
        assert_eq!(
            tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-space", "chars": "   "})),
            "Send command input",
            "whitespace-only chars are still stdin bytes"
        );
    }

    #[test]
    fn write_stdin_explicit_write_without_input_reads_as_poll() {
        // Matches `write_stdin_dispatch`: explicit write with empty/missing
        // input executes as Poll, so the label must not claim a send.
        assert_eq!(
            tool_action_label(
                tools::WRITE_STDIN,
                &json!({"session_id": "run-write-empty", "action": "write", "chars": ""})
            ),
            "Read command session"
        );
        assert_eq!(
            tool_action_label(tools::WRITE_STDIN, &json!({"session_id": "run-write-none", "action": "write"})),
            "Read command session"
        );
    }
}
