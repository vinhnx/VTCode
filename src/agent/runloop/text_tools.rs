use serde_json::{Map, Number, Value};
use shell_words::split as split_shell_words;

const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];

const SHELL_CALL_PREFIXES: &[&str] = &["shell", "default_api.shell"];

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    if let Some(result) = detect_shell_call(text) {
        return Some(result);
    }

    for prefix in TEXTUAL_TOOL_PREFIXES {
        let mut search_start = 0usize;
        while let Some(offset) = text[search_start..].find(prefix) {
            let prefix_index = search_start + offset;
            let start = prefix_index + prefix.len();
            let tail = &text[start..];
            let mut name_len = 0usize;
            for ch in tail.chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    name_len += ch.len_utf8();
                } else {
                    break;
                }
            }
            if name_len == 0 {
                search_start += offset + prefix.len();
                continue;
            }

            let name = tail[..name_len].to_string();
            let after_name = &tail[name_len..];
            let Some(paren_offset) = after_name.find('(') else {
                search_start = start;
                continue;
            };

            let args_start = start + name_len + paren_offset + 1;
            let mut depth = 1i32;
            let mut end: Option<usize> = None;
            for (rel_idx, ch) in text[args_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(args_start + rel_idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let args_end = end?;
            let raw_args = &text[args_start..args_end];
            if let Some(args) = parse_textual_arguments(raw_args) {
                return Some((name, args));
            }

            search_start = prefix_index + prefix.len() + name_len;
        }
    }
    None
}

fn detect_shell_call(text: &str) -> Option<(String, Value)> {
    for prefix in SHELL_CALL_PREFIXES {
        let mut search_start = 0usize;
        while let Some(offset) = text[search_start..].find(prefix) {
            let prefix_index = search_start + offset;

            // Ensure the prefix is not part of a longer identifier
            if prefix_index > 0
                && text
                    .chars()
                    .nth(prefix_index - 1)
                    .map(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                    .unwrap_or(false)
            {
                search_start = prefix_index + prefix.len();
                continue;
            }

            let mut tail = &text[prefix_index + prefix.len()..];
            tail = tail.trim_start();
            if !tail.starts_with('(') {
                search_start = prefix_index + prefix.len();
                continue;
            }

            let args_start = prefix_index + prefix.len() + tail[..1].len();
            let mut depth = 1i32;
            let mut end: Option<usize> = None;
            for (rel_idx, ch) in text[args_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(args_start + rel_idx);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let args_end = end?;
            let raw_args = &text[args_start..args_end];
            let parsed = parse_textual_arguments(raw_args)?;
            let normalized = normalize_shell_arguments(parsed)?;
            return Some(("run_terminal_cmd".to_string(), normalized));
        }
    }

    None
}

fn normalize_shell_arguments(value: Value) -> Option<Value> {
    match value {
        Value::String(command) => {
            Some(shell_command_value_from_string(command).map(Value::Object)?)
        }
        Value::Array(items) => Some(shell_command_value_from_array(items).map(Value::Object)?),
        Value::Object(map) => Some(shell_command_value_from_object(map)?),
        _ => None,
    }
}

fn shell_command_value_from_string(command: String) -> Option<Map<String, Value>> {
    let tokens = parse_shell_tokens(&command)?;
    let mut map = Map::new();
    map.insert(
        "command".to_string(),
        Value::Array(tokens.into_iter().map(Value::String).collect()),
    );
    Some(map)
}

fn shell_command_value_from_array(items: Vec<Value>) -> Option<Map<String, Value>> {
    let mut tokens = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::String(text) if !text.trim().is_empty() => {
                tokens.push(Value::String(text));
            }
            Value::String(_) => {}
            _ => return None,
        }
    }

    if tokens.is_empty() {
        return None;
    }

    let mut map = Map::new();
    map.insert("command".to_string(), Value::Array(tokens));
    Some(map)
}

fn shell_command_value_from_object(mut map: Map<String, Value>) -> Option<Value> {
    let command_value = map
        .remove("command")
        .or_else(|| map.remove("cmd"))
        .or_else(|| map.remove("program"))?;

    let command_entry = match command_value {
        Value::String(command) => shell_command_value_from_string(command)?,
        Value::Array(items) => shell_command_value_from_array(items)?,
        _ => return None,
    };

    let mut normalized = command_entry;

    if let Some(timeout) = map.remove("timeout_secs").or_else(|| map.remove("timeout")) {
        normalized.insert("timeout_secs".to_string(), timeout);
    }

    if let Some(working_dir) = map
        .remove("working_dir")
        .or_else(|| map.remove("workdir"))
        .or_else(|| map.remove("cwd"))
    {
        normalized.insert("working_dir".to_string(), working_dir);
    }

    if let Some(mode) = map.remove("mode") {
        normalized.insert("mode".to_string(), mode);
    }

    if let Some(response_format) = map.remove("response_format") {
        normalized.insert("response_format".to_string(), response_format);
    }

    Some(Value::Object(normalized))
}

fn parse_shell_tokens(command: &str) -> Option<Vec<String>> {
    let tokens = split_shell_words(command).ok()?;
    let filtered: Vec<String> = tokens
        .into_iter()
        .filter(|token| !token.is_empty())
        .collect();
    if filtered.is_empty() {
        return None;
    }
    Some(filtered)
}

fn parse_textual_arguments(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Value::Object(Map::new()));
    }

    if let Some(val) = try_parse_json_value(trimmed) {
        return Some(val);
    }

    parse_key_value_arguments(trimmed)
}

fn try_parse_json_value(input: &str) -> Option<Value> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Some(Value::Object(Map::new()));
    }

    serde_json::from_str(trimmed).ok().or_else(|| {
        if trimmed.contains('\'') {
            let normalized = trimmed.replace('\'', "\"");
            serde_json::from_str(&normalized).ok()
        } else {
            None
        }
    })
}

fn parse_key_value_arguments(input: &str) -> Option<Value> {
    let mut map = Map::new();

    for segment in input.split(',') {
        let pair = segment.trim();
        if pair.is_empty() {
            continue;
        }

        let (key_raw, value_raw) = pair.split_once('=').or_else(|| pair.split_once(':'))?;

        let key = key_raw
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let value = parse_scalar_value(value_raw.trim());
        map.insert(key, value);
    }

    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn parse_scalar_value(input: &str) -> Value {
    if let Some(val) = try_parse_json_value(input) {
        return val;
    }

    let trimmed = input
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string();
    if trimmed.is_empty() {
        return Value::String(trimmed);
    }

    match trimmed.to_ascii_lowercase().as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }

    if let Ok(int_val) = trimmed.parse::<i64>() {
        return Value::Number(Number::from(int_val));
    }

    if let Ok(float_val) = trimmed.parse::<f64>()
        && let Some(num) = Number::from_f64(float_val)
    {
        return Value::Number(num);
    }

    Value::String(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_textual_tool_call_parses_python_style_arguments() {
        let message = "call\nprint(default_api.read_file(path='CLAUDE.md'))";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "read_file");
        assert_eq!(args, serde_json::json!({ "path": "CLAUDE.md" }));
    }

    #[test]
    fn test_detect_textual_tool_call_supports_json_payload() {
        let message =
            "print(default_api.write_file({\"path\": \"notes.md\", \"content\": \"hi\"}))";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "write_file");
        assert_eq!(
            args,
            serde_json::json!({ "path": "notes.md", "content": "hi" })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_handles_boolean_and_numbers() {
        let message =
            "default_api.search_workspace(query='todo', max_results=5, include_archived=false)";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "search_workspace");
        assert_eq!(
            args,
            serde_json::json!({
                "query": "todo",
                "max_results": 5,
                "include_archived": false
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_interprets_shell_string_argument() {
        let message = "shell(\"git diff\")";
        let (name, args) = detect_textual_tool_call(message).expect("should parse shell call");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args, serde_json::json!({ "command": ["git", "diff"] }));
    }

    #[test]
    fn test_detect_textual_tool_call_interprets_shell_array_argument() {
        let message = "shell(['ls', '-a'])";
        let (name, args) = detect_textual_tool_call(message).expect("should parse shell array");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args, serde_json::json!({ "command": ["ls", "-a"] }));
    }

    #[test]
    fn test_detect_textual_tool_call_interprets_shell_object_arguments() {
        let message = "shell(command='npm run build', timeout=30, cwd='app')";
        let (name, args) = detect_textual_tool_call(message).expect("should parse shell object");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["npm", "run", "build"],
                "timeout_secs": 30,
                "working_dir": "app"
            })
        );
    }
}
