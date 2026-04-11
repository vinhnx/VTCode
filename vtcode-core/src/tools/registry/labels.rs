use serde_json::Value;

use crate::config::constants::tools as tool_names;
use crate::tools::mcp::legacy_mcp_tool_name;
use crate::tools::tool_intent;

pub fn tool_action_label(tool_name: &str, args: &Value) -> String {
    let actual_tool_name = normalize_tool_name(tool_name);

    match actual_tool_name {
        name if name == tool_names::EXEC_COMMAND => "Run command".to_string(),
        name if name == tool_names::WRITE_STDIN => "Send command input".to_string(),
        name if name == tool_names::RUN_PTY_CMD => "Run command".to_string(),
        name if name == tool_names::EXECUTE_CODE => "Run code".to_string(),
        name if name == tool_names::GET_ERRORS => "List errors".to_string(),
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
        name if name == tool_names::CREATE_PTY_SESSION => "Create command session".to_string(),
        name if name == tool_names::READ_PTY_SESSION => "Read command session".to_string(),
        name if name == tool_names::LIST_PTY_SESSIONS => "List command sessions".to_string(),
        name if name == tool_names::SEND_PTY_INPUT => "Send command input".to_string(),
        name if name == tool_names::CLOSE_PTY_SESSION => "Close command session".to_string(),
        name if name == tool_names::RESIZE_PTY_SESSION => "Resize command session".to_string(),
        name if name == tool_names::UNIFIED_EXEC => {
            match tool_intent::unified_exec_action(args).unwrap_or("run") {
                "run" => "Run command".to_string(),
                "write" => "Send command input".to_string(),
                "poll" => "Read command session".to_string(),
                "continue" => "Continue command session".to_string(),
                "inspect" => "Inspect command output".to_string(),
                "list" => "List command sessions".to_string(),
                "close" => "Close command session".to_string(),
                "code" => "Run code".to_string(),
                _ => "Exec action".to_string(),
            }
        }
        name if name == tool_names::UNIFIED_SEARCH => {
            let normalized = tool_intent::normalize_unified_search_args(args);
            let workflow = normalized
                .get("workflow")
                .and_then(Value::as_str)
                .unwrap_or("query");

            match tool_intent::unified_search_action(&normalized).unwrap_or("grep") {
                "grep" => "Search text".to_string(),
                "list" => "List files".to_string(),
                "structural" => match workflow {
                    "scan" => "Structural scan".to_string(),
                    "test" => "Structural test".to_string(),
                    _ => "Structural search".to_string(),
                },
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
                "patch" | tool_names::APPLY_PATCH => "Apply patch".to_string(),
                "delete" => "Delete file".to_string(),
                "move" => "Move file".to_string(),
                "copy" => "Copy file".to_string(),
                _ => "File operation".to_string(),
            }
        }
        "fetch" => "Fetch".to_string(),
        _ => humanize_tool_name(actual_tool_name),
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
    fn unified_search_structural_query_uses_default_label() {
        let label = tool_action_label(
            tools::UNIFIED_SEARCH,
            &json!({"action": "structural", "pattern": "fn $NAME() {}"}),
        );

        assert_eq!(label, "Structural search");
    }

    #[test]
    fn unified_search_structural_scan_uses_distinct_label() {
        let label = tool_action_label(
            tools::UNIFIED_SEARCH,
            &json!({"action": "structural", "workflow": "scan"}),
        );

        assert_eq!(label, "Structural scan");
    }

    #[test]
    fn unified_search_structural_test_uses_distinct_label() {
        let label = tool_action_label(
            tools::UNIFIED_SEARCH,
            &json!({"workflow": "test", "config_path": "sgconfig.yml"}),
        );

        assert_eq!(label, "Structural test");
    }
}
