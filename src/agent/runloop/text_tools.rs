use serde_json::{Map, Number, Value};
use shell_words::split as shell_split;
use std::collections::BTreeMap;

const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    if let Some(parsed) = parse_tagged_tool_call(text) {
        return Some(parsed);
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

fn parse_tagged_tool_call(text: &str) -> Option<(String, Value)> {
    const TOOL_TAG: &str = "<tool_call>";
    const ARG_KEY_TAG: &str = "<arg_key>";
    const ARG_VALUE_TAG: &str = "<arg_value>";
    const ARG_KEY_CLOSE: &str = "</arg_key>";
    const ARG_VALUE_CLOSE: &str = "</arg_value>";

    let start = text.find(TOOL_TAG)?;
    let mut rest = &text[start + TOOL_TAG.len()..];
    let (name, after_name) = read_tag_text(rest);
    if name.is_empty() {
        return None;
    }

    let mut object = Map::new();
    let mut indexed_values: BTreeMap<String, BTreeMap<usize, Value>> = BTreeMap::new();
    rest = after_name;

    while let Some(key_index) = rest.find(ARG_KEY_TAG) {
        rest = &rest[key_index + ARG_KEY_TAG.len()..];
        let (raw_key, mut after_key) = read_tag_text(rest);
        if raw_key.is_empty() {
            rest = after_key;
            continue;
        }
        if after_key.starts_with(ARG_KEY_CLOSE) {
            after_key = &after_key[ARG_KEY_CLOSE.len()..];
        }

        rest = after_key;
        let Some(value_index) = rest.find(ARG_VALUE_TAG) else {
            break;
        };
        rest = &rest[value_index + ARG_VALUE_TAG.len()..];
        let (raw_value, mut after_value) = read_tag_text(rest);
        if after_value.starts_with(ARG_VALUE_CLOSE) {
            after_value = &after_value[ARG_VALUE_CLOSE.len()..];
        }
        rest = after_value;

        let key = raw_key.trim();
        let value = parse_scalar_value(raw_value.trim());
        if let Some((base, index)) = split_indexed_key(key) {
            indexed_values
                .entry(base.to_string())
                .or_default()
                .insert(index, value);
        } else {
            object.insert(key.to_string(), value);
        }
    }

    for (base, entries) in indexed_values {
        let mut ordered = Vec::new();
        for (index, value) in entries {
            while ordered.len() < index {
                ordered.push(Value::Null);
            }
            ordered.push(value);
        }
        object.insert(base, Value::Array(ordered));
    }

    if let Some(Value::String(command)) = object.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        object.insert("command".to_string(), Value::Array(array));
    }

    Some((name.to_string(), Value::Object(object)))
}

fn read_tag_text(input: &str) -> (String, &str) {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return (String::new(), "");
    }

    if let Some(idx) = trimmed.find('<') {
        let (value, rest) = trimmed.split_at(idx);
        (
            value.trim().to_string(),
            rest.trim_start_matches(['\n', '\r']),
        )
    } else {
        (trimmed.trim().to_string(), "")
    }
}

fn split_indexed_key(key: &str) -> Option<(&str, usize)> {
    let (base, index_str) = key.rsplit_once('.')?;
    let index = index_str.parse().ok()?;
    Some((base, index))
}

fn normalize_command_string(command: &str) -> Option<Vec<Value>> {
    if command.trim().is_empty() {
        return None;
    }

    if let Ok(parts) = shell_split(command)
        && !parts.is_empty()
    {
        return Some(parts.into_iter().map(Value::String).collect());
    }

    let fallback: Vec<Value> = command
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .map(|segment| Value::String(segment.to_string()))
        .collect();
    if fallback.is_empty() {
        None
    } else {
        Some(fallback)
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
    fn test_detect_tagged_tool_call_parses_basic_command() {
        let message =
            "<tool_call>run_terminal_cmd\n<arg_key>command\n<arg_value>ls -a\n</tool_call>";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["ls", "-a"]
            })
        );
    }

    #[test]
    fn test_detect_tagged_tool_call_respects_indexed_arguments() {
        let message = "<tool_call>run_terminal_cmd\n<arg_key>command.0\n<arg_value>python\n<arg_key>command.1\n<arg_value>-c\n<arg_key>command.2\n<arg_value>print('hi')\n</tool_call>";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["python", "-c", "print('hi')"]
            })
        );
    }
}
