use serde_json::Value;

use crate::config::constants::tools as tool_names;

pub fn tool_action_label(tool_name: &str, args: &Value) -> String {
    let actual_tool_name = normalize_tool_name(tool_name);

    match actual_tool_name {
        name if name == tool_names::RUN_PTY_CMD => "Run command".to_string(),
        name if name == tool_names::EXECUTE_CODE => "Run code".to_string(),
        name if name == tool_names::LIST_FILES => "List files".to_string(),
        name if name == tool_names::GREP_FILE => "Search text".to_string(),
        name if name == tool_names::CODE_INTELLIGENCE => "Code intelligence".to_string(),
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
        name if name == tool_names::CREATE_PTY_SESSION => "Create session".to_string(),
        name if name == tool_names::READ_PTY_SESSION => "Read session".to_string(),
        name if name == tool_names::LIST_PTY_SESSIONS => "List sessions".to_string(),
        name if name == tool_names::SEND_PTY_INPUT => "Send input".to_string(),
        name if name == tool_names::CLOSE_PTY_SESSION => "Close session".to_string(),
        name if name == tool_names::RESIZE_PTY_SESSION => "Resize session".to_string(),
        name if name == tool_names::UNIFIED_EXEC => match unified_exec_action(args).as_str() {
            "run" => "Run command".to_string(),
            "write" => "Send input".to_string(),
            "poll" => "Read session".to_string(),
            "list" => "List sessions".to_string(),
            "close" => "Close session".to_string(),
            "code" => "Run code".to_string(),
            _ => "Exec action".to_string(),
        },
        name if name == tool_names::UNIFIED_SEARCH => match unified_search_action(args).as_str() {
            "grep" => "Search text".to_string(),
            "list" => "List files".to_string(),
            "intelligence" => "Code intelligence".to_string(),
            "tools" => "List tools".to_string(),
            "errors" => "List errors".to_string(),
            "agent" => "Show agent info".to_string(),
            "web" => "Fetch".to_string(),
            "skill" => "Load skill".to_string(),
            _ => "Search".to_string(),
        },
        name if name == tool_names::UNIFIED_FILE => match unified_file_action(args).as_str() {
            "read" => "Read file".to_string(),
            "write" => "Write file".to_string(),
            "edit" => "Edit file".to_string(),
            "patch" | "apply_patch" => "Apply patch".to_string(),
            "delete" => "Delete file".to_string(),
            "move" => "Move file".to_string(),
            "copy" => "Copy file".to_string(),
            _ => "File operation".to_string(),
        },
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

fn unified_exec_action(args: &Value) -> String {
    args.get("action")
        .and_then(Value::as_str)
        .unwrap_or("run")
        .to_string()
}

fn unified_search_action(args: &Value) -> String {
    if let Some(action) = args.get("action").and_then(Value::as_str) {
        return action.to_string();
    }
    if args.get("pattern").is_some() || args.get("query").is_some() {
        return "grep".to_string();
    }
    if args.get("operation").is_some() {
        return "intelligence".to_string();
    }
    if args.get("url").is_some() {
        return "web".to_string();
    }
    if args.get("sub_action").is_some() {
        return "skill".to_string();
    }
    if args.get("scope").is_some() {
        return "errors".to_string();
    }
    if args.get("path").is_some() {
        return "list".to_string();
    }
    "grep".to_string()
}

fn unified_file_action(args: &Value) -> String {
    args.get("action")
        .and_then(Value::as_str)
        .or_else(|| {
            if args.get("old_str").is_some() {
                Some("edit")
            } else if args.get("patch").is_some() {
                Some("patch")
            } else if args.get("content").is_some() {
                Some("write")
            } else if args.get("destination").is_some() {
                Some("move")
            } else {
                Some("read")
            }
        })
        .unwrap_or("read")
        .to_string()
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
