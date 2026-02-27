use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::tool_intent;

pub fn tool_action_label(tool_name: &str, args: &Value) -> String {
    let actual_tool_name = normalize_tool_name(tool_name);

    match actual_tool_name {
        name if name == tool_names::RUN_PTY_CMD => "Run command (PTY)".to_string(),
        name if name == tool_names::EXECUTE_CODE => "Run code".to_string(),
        name if name == tool_names::LIST_FILES => "List files".to_string(),
        name if name == tool_names::GREP_FILE => "Search text".to_string(),
        name if name == tool_names::SEARCH_TOOLS => "List tools".to_string(),
        name if name == tool_names::GET_ERRORS => "List errors".to_string(),
        name if name == tool_names::AGENT_INFO => "Show agent info".to_string(),
        name if name == tool_names::LIST_SKILLS => "List skills".to_string(),
        name if name == tool_names::LOAD_SKILL => "Load skill".to_string(),
        name if name == tool_names::LOAD_SKILL_RESOURCE => "Load skill resource".to_string(),
        name if name == tool_names::READ_FILE => "Read file".to_string(),
        name if name == tool_names::WRITE_FILE => "Write file".to_string(),
        name if name == tool_names::EDIT_FILE => "Edit file".to_string(),
        name if name == tool_names::CREATE_FILE => "Create file".to_string(),
        name if name == tool_names::DELETE_FILE => "Delete file".to_string(),
        name if name == tool_names::APPLY_PATCH => "Apply patch".to_string(),
        name if name == tool_names::SEARCH_REPLACE => "Search/replace".to_string(),
        name if name == tool_names::CREATE_PTY_SESSION => "Create PTY session".to_string(),
        name if name == tool_names::READ_PTY_SESSION => "Read PTY session".to_string(),
        name if name == tool_names::LIST_PTY_SESSIONS => "List PTY sessions".to_string(),
        name if name == tool_names::SEND_PTY_INPUT => "Send PTY input".to_string(),
        name if name == tool_names::CLOSE_PTY_SESSION => "Close PTY session".to_string(),
        name if name == tool_names::RESIZE_PTY_SESSION => "Resize PTY session".to_string(),
        name if name == tool_names::UNIFIED_EXEC => {
            match tool_intent::unified_exec_action(args).unwrap_or("run") {
                "run" => "Run command (PTY)".to_string(),
                "write" => "Send PTY input".to_string(),
                "poll" => "Read PTY session".to_string(),
                "continue" => "Continue PTY session".to_string(),
                "inspect" => "Inspect PTY output".to_string(),
                "list" => "List PTY sessions".to_string(),
                "close" => "Close PTY session".to_string(),
                "code" => "Run code".to_string(),
                _ => "Exec action".to_string(),
            }
        }
        name if name == tool_names::UNIFIED_SEARCH => {
            match tool_intent::unified_search_action(args).unwrap_or("grep") {
                "grep" => "Search text".to_string(),
                "list" => "List files".to_string(),
                "tools" => "List tools".to_string(),
                "errors" => "List errors".to_string(),
                "agent" => "Show agent info".to_string(),
                "web" => "Fetch".to_string(),
                "skill" => "Load skill".to_string(),
                _ => "Search".to_string(),
            }
        }
        name if name == tool_names::UNIFIED_FILE => {
            match tool_intent::unified_file_action(args).unwrap_or("read") {
                "read" => "Read file".to_string(),
                "write" => "Write file".to_string(),
                "edit" => "Edit file".to_string(),
                "patch" | "apply_patch" => "Apply patch".to_string(),
                "delete" => "Delete file".to_string(),
                "move" => "Move file".to_string(),
                "copy" => "Copy file".to_string(),
                _ => "File operation".to_string(),
            }
        }
        "fetch" | "web_fetch" => "Fetch".to_string(),
        _ => humanize_tool_name(actual_tool_name),
    }
}

fn normalize_tool_name(tool_name: &str) -> &str {
    if let Some(stripped) = tool_name.strip_prefix("mcp_") {
        return stripped;
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
    let first = chars.next().unwrap();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}
