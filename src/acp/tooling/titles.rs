use serde_json::Value;

use super::catalog::{SupportedTool, ToolDescriptor};
use super::schemas::{
    TOOL_LIST_FILES_CONTENT_PATTERN_ARG, TOOL_LIST_FILES_NAME_PATTERN_ARG,
    TOOL_LIST_FILES_PATH_ARG, TOOL_READ_FILE_PATH_ARG, TOOL_READ_FILE_URI_ARG,
};

pub(super) fn render_title(
    descriptor: ToolDescriptor,
    function_name: &str,
    args: &Value,
) -> String {
    match descriptor {
        ToolDescriptor::Acp(tool) => match tool {
            SupportedTool::ReadFile => args
                .get(TOOL_READ_FILE_PATH_ARG)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(|path| format!("Read file {}", truncate_middle(path, 80)))
                .or_else(|| {
                    args.get(TOOL_READ_FILE_URI_ARG)
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .map(|uri| format!("Read file {}", truncate_middle(uri, 80)))
                })
                .unwrap_or_else(|| tool.default_title().to_string()),
            SupportedTool::ListFiles => {
                if let Some(path) = args
                    .get(TOOL_LIST_FILES_PATH_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    if path == "." {
                        "List files in workspace root".to_string()
                    } else {
                        format!("List files in {}", truncate_middle(path, 60))
                    }
                } else if let Some(pattern) = args
                    .get(TOOL_LIST_FILES_NAME_PATTERN_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    format!("Find files named {}", truncate_middle(pattern, 40))
                } else if let Some(pattern) = args
                    .get(TOOL_LIST_FILES_CONTENT_PATTERN_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                {
                    format!("Search files for {}", truncate_middle(pattern, 40))
                } else {
                    tool.default_title().to_string()
                }
            }
            SupportedTool::SwitchMode => args
                .get("mode_id")
                .and_then(Value::as_str)
                .map(|mode| format!("Switch to {mode} mode"))
                .unwrap_or_else(|| tool.default_title().to_string()),
        },
        ToolDescriptor::Local => format_local_title(function_name),
    }
}

fn truncate_middle(input: &str, max_len: usize) -> String {
    let total = input.chars().count();
    if total <= max_len {
        return input.to_string();
    }

    if max_len < 3 {
        return input.chars().take(max_len).collect();
    }

    let front_len = max_len / 2;
    let back_len = max_len.saturating_sub(front_len + 1);
    let front: String = input.chars().take(front_len).collect();
    let back: String = input
        .chars()
        .rev()
        .take(back_len)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{front}…{back}")
}

fn format_local_title(name: &str) -> String {
    let formatted = name.replace('_', " ");
    let mut chars = formatted.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => formatted,
    }
}
