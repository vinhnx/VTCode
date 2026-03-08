use serde_json::Value;
use vtcode_core::config::constants::tools;

use crate::agent::runloop::text_tools::canonical::{
    apply_unified_exec_defaults, unified_exec_defaults_for_name,
};

pub(super) fn parse_channel_tool_call(text: &str) -> Option<(String, Value)> {
    // Harmony format: <|start|>{header}<|message|>{content}<|end|>
    // We look for a message that is a tool call (ends with <|call|> or has commentary channel)

    for segment in text.split("<|start|>") {
        let trimmed_segment = segment.trim();
        if trimmed_segment.is_empty() {
            continue;
        }

        let channel_idx = segment.find("<|channel|>");
        let message_idx = segment.find("<|message|>");

        if let (Some(c_idx), Some(m_idx)) = (channel_idx, message_idx)
            && m_idx > c_idx
        {
            let header = &segment[..m_idx];

            // Check if this is a commentary channel or has a recipient
            if header.contains("commentary") || header.contains("to=") {
                let stop_idx = segment
                    .find("<|call|>")
                    .or_else(|| segment.find("<|end|>"))
                    .or_else(|| segment.find("<|return|>"))
                    .unwrap_or(segment.len());

                let content_raw = segment[m_idx + "<|message|>".len()..stop_idx].trim();

                // Parse tool name from header
                let tool_name = if let Some(to_pos) = header.find("to=") {
                    let after_to = &header[to_pos + 3..];
                    let tool_ref = after_to
                        .split(|c: char| c.is_whitespace() || c == '<')
                        .next()
                        .unwrap_or("");
                    parse_tool_name_from_reference(tool_ref)
                } else if header.contains("container.exec") || header.contains("exec") {
                    "unified_exec"
                } else if header.contains("read") || header.contains("file") {
                    "read_file"
                } else {
                    // Default to command execution if it's a commentary channel but no recipient
                    "unified_exec"
                };

                // Parse JSON from content
                if let Ok(parsed) = serde_json::from_str::<Value>(content_raw) {
                    // Convert to expected format
                    if let Ok(args) = convert_harmony_args_to_tool_format(tool_name, parsed) {
                        return Some((tool_name.to_string(), args));
                    }
                }
            }
        }
    }

    None
}

pub(super) fn parse_tool_name_from_reference(tool_ref: &str) -> &str {
    match tool_ref {
        "repo_browser.list_files" | "list_files" => "list_files",
        "repo_browser.read_file" | "read_file" => "read_file",
        "repo_browser.write_file" | "write_file" => "write_file",
        "container.exec" | "exec" | "bash" | "exec_command" => tools::UNIFIED_EXEC,
        "grep" => "grep_file",
        _ => {
            // Try to extract the function name after the last dot
            if let Some(dot_pos) = tool_ref.rfind('.') {
                &tool_ref[dot_pos + 1..]
            } else {
                tool_ref
            }
        }
    }
}

pub(super) fn convert_harmony_args_to_tool_format(
    tool_name: &str,
    parsed: Value,
) -> Result<Value, String> {
    if let Some(defaults) = unified_exec_defaults_for_name(tool_name) {
        let mut result = serde_json::Map::new();
        apply_unified_exec_defaults(&mut result, defaults);

        // Preserve other parameters from the original parsed object
        if let Some(map) = parsed.as_object() {
            for (key, value) in map {
                if key != "cmd" && key != "command" && key != "action" {
                    result.insert(key.to_string(), value.clone());
                }
            }
        }

        if matches!(defaults.action, "list" | "close" | "poll" | "write")
            && parsed.get("cmd").is_none()
            && parsed.get("command").is_none()
        {
            return Ok(Value::Object(result));
        }

        let command = normalized_harmony_command(&parsed)?
            .ok_or_else(|| "no 'cmd' or 'command' parameter provided".to_string())?;
        result.insert("command".to_string(), command);
        Ok(Value::Object(result))
    } else {
        match tool_name {
            "list_files" => {
                // Convert harmony list_files format to vtcode format
                let mut args = serde_json::Map::new();

                if let Some(path) = parsed.get("path") {
                    args.insert("path".to_string(), path.clone());
                }

                if let Some(recursive) = parsed.get("recursive") {
                    args.insert("recursive".to_string(), recursive.clone());
                }

                Ok(Value::Object(args))
            }
            _ => Ok(parsed),
        }
    }
}

fn normalized_harmony_command(parsed: &Value) -> Result<Option<Value>, String> {
    parsed
        .get("cmd")
        .or_else(|| parsed.get("command"))
        .map(normalize_harmony_command_value)
        .transpose()
}

fn normalize_harmony_command_value(command: &Value) -> Result<Value, String> {
    match command {
        Value::String(command) => {
            if command.trim().is_empty() {
                Err("command executable cannot be empty".to_string())
            } else {
                Ok(Value::String(command.clone()))
            }
        }
        Value::Array(values) => {
            let command = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| "command array must contain only strings".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?;

            if command
                .first()
                .map(|part| part.trim().is_empty())
                .unwrap_or(true)
            {
                Err("command executable cannot be empty".to_string())
            } else {
                Ok(serde_json::json!(command))
            }
        }
        _ => Err("command must be a string or array of strings".to_string()),
    }
}
