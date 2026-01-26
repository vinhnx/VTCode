use serde_json::Value;

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
                    "run_pty_cmd"
                } else if header.contains("read") || header.contains("file") {
                    "read_file"
                } else {
                    // Default to pty command if it's a commentary channel but no recipient
                    "run_pty_cmd"
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
        "container.exec" | "exec" | "bash" => "run_pty_cmd",
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
    match tool_name {
        "run_pty_cmd" | "bash" => {
            let mut result = serde_json::Map::new();

            // Preserve other parameters from the original parsed object
            if let Some(map) = parsed.as_object() {
                for (key, value) in map {
                    if key != "cmd" && key != "command" {
                        result.insert(key.to_string(), value.clone());
                    }
                }
            }

            // Handle command parameter - try multiple sources
            if let Some(cmd) = parsed.get("cmd").and_then(|v| v.as_array()) {
                let command: Vec<String> = cmd
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                // Validate non-empty command and executable
                if command.is_empty() || command[0].trim().is_empty() {
                    return Err("command executable cannot be empty".to_string());
                }

                result.insert("command".to_string(), serde_json::json!(command));
                Ok(Value::Object(result))
            } else if let Some(cmd_str) = parsed.get("cmd").and_then(|v| v.as_str()) {
                // Handle string command from 'cmd' parameter
                if cmd_str.trim().is_empty() {
                    return Err("command executable cannot be empty".to_string());
                }

                result.insert("command".to_string(), serde_json::json!([cmd_str]));
                Ok(Value::Object(result))
            } else if let Some(cmd) = parsed.get("command").and_then(|v| v.as_array()) {
                // Fallback: handle 'command' array parameter
                let command: Vec<String> = cmd
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                if command.is_empty() || command[0].trim().is_empty() {
                    return Err("command executable cannot be empty".to_string());
                }

                result.insert("command".to_string(), serde_json::json!(command));
                Ok(Value::Object(result))
            } else if let Some(cmd_str) = parsed.get("command").and_then(|v| v.as_str()) {
                // Fallback: handle 'command' string parameter
                if cmd_str.trim().is_empty() {
                    return Err("command executable cannot be empty".to_string());
                }

                result.insert("command".to_string(), serde_json::json!([cmd_str]));
                Ok(Value::Object(result))
            } else {
                // No command found - return error
                Err("no 'cmd' or 'command' parameter provided".to_string())
            }
        }
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
