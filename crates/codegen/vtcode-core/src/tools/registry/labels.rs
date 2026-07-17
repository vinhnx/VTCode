use std::borrow::Cow;

use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::mcp::legacy_mcp_tool_name;
use crate::tools::tool_intent;

pub fn tool_action_label(tool_name: &str, args: &Value) -> Cow<'static, str> {
    let actual_tool_name = normalize_tool_name(tool_name);

    match actual_tool_name {
        name if name == tool_names::EXEC_COMMAND => Cow::Borrowed("Run command"),
        name if name == tool_names::WRITE_STDIN => Cow::Borrowed("Send command input"),
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
        name if name == tool_names::UNIFIED_EXEC => {
            match tool_intent::command_session_action(args).unwrap_or("run") {
                "run" => Cow::Borrowed("Run command"),
                "write" => Cow::Borrowed("Send command input"),
                "poll" => Cow::Borrowed("Read command session"),
                "continue" => Cow::Borrowed("Continue command session"),
                "inspect" => Cow::Borrowed("Inspect command output"),
                "list" => Cow::Borrowed("List command sessions"),
                "close" => Cow::Borrowed("Close command session"),
                "code" => Cow::Borrowed("Run code"),
                _ => Cow::Borrowed("Exec action"),
            }
        }
        name if name == tool_names::CODE_SEARCH => Cow::Borrowed("Search code"),
        name if name == tool_names::UNIFIED_FILE => {
            match tool_intent::file_operation_action(args).unwrap_or("read") {
                "read" => Cow::Borrowed("Read file"),
                "write" => Cow::Borrowed("Write file"),
                "edit" => Cow::Borrowed("Edit file"),
                "patch" | tool_names::APPLY_PATCH => Cow::Borrowed("Apply patch"),
                "delete" => Cow::Borrowed("Delete file"),
                "move" => Cow::Borrowed("Move file"),
                "copy" => Cow::Borrowed("Copy file"),
                _ => Cow::Borrowed("File operation"),
            }
        }
        "fetch" => Cow::Borrowed("Fetch"),
        _ => Cow::Owned(humanize_tool_name(actual_tool_name)),
    }
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
        assert_eq!(
            tool_action_label(tools::CODE_SEARCH, &json!({"query": "Widget"})),
            "Search code"
        );
    }
}
