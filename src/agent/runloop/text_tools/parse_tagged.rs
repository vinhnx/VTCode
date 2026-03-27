use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::canonical::canonicalize_tool_result;
use crate::agent::runloop::text_tools::parse_args::{
    normalize_command_string, parse_key_value_arguments, parse_scalar_value, split_indexed_key,
};

pub(super) fn parse_tagged_tool_call(text: &str) -> Option<(String, Value)> {
    parse_standard_tagged_tool_call(text).or_else(|| parse_minimax_tool_call(text))
}

fn parse_standard_tagged_tool_call(text: &str) -> Option<(String, Value)> {
    const TOOL_TAG: &str = "<tool_call>";
    const TOOL_TAG_CLOSE: &str = "</tool_call>";
    const ARG_KEY_TAG: &str = "<arg_key>";
    const ARG_VALUE_TAG: &str = "<arg_value>";
    const ARG_KEY_CLOSE: &str = "</arg_key>";
    const ARG_VALUE_CLOSE: &str = "</arg_value>";

    let start = text.find(TOOL_TAG)?;
    let rest_initial = &text[start + TOOL_TAG.len()..];

    // Find the end of the tool name. It ends at:
    // 1. The first '<' (start of next tag)
    // 2. The first '{' (start of JSON arguments)
    // 3. The first whitespace (separator for key=value arguments)
    // 4. The end of the string
    let name_end = rest_initial
        .find(|c: char| c == '<' || c == '{' || c.is_whitespace())
        .unwrap_or(rest_initial.len());

    let name = rest_initial[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let mut rest = &rest_initial[name_end..];
    let mut object = Map::new();
    let mut indexed_values: BTreeMap<String, BTreeMap<usize, Value>> = BTreeMap::new();

    // First, try standard <arg_key>/<arg_value> parsing
    let mut found_arg_tags = false;
    while let Some(key_index) = rest.find(ARG_KEY_TAG) {
        found_arg_tags = true;
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

    // If no arg tags found, try fallback parsing for malformed output
    // e.g., <tool_call>list_files<tool_call>{"path": "/tmp"} or <tool_call>read_file path="/tmp"
    if !found_arg_tags && object.is_empty() {
        let after_name = &rest_initial[name_end..];
        // Determine the content boundary (next <tool_call>, </tool_call>, or end)
        let content_end = after_name
            .find(TOOL_TAG)
            .or_else(|| after_name.find(TOOL_TAG_CLOSE))
            .unwrap_or(after_name.len());
        let content = after_name[..content_end].trim();

        if !content.is_empty() {
            // Try parsing as JSON first
            if let Some(json_start) = content.find('{') {
                let json_content = &content[json_start..];
                // Find matching closing brace
                let mut depth = 0;
                let mut json_end = None;
                for (idx, ch) in json_content.char_indices() {
                    match ch {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                json_end = Some(idx + 1);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(end) = json_end
                    && let Ok(parsed) = serde_json::from_str::<Value>(&json_content[..end])
                    && let Some(obj) = parsed.as_object()
                {
                    for (k, v) in obj {
                        object.insert(k.clone(), v.clone());
                    }
                }
            }

            // If JSON parsing didn't work, try key=value or key:value pairs
            if object.is_empty()
                && let Some(parsed) = parse_key_value_arguments(content)
                && let Some(obj) = parsed.as_object()
            {
                for (k, v) in obj {
                    object.insert(k.clone(), v.clone());
                }
            }
        }
    }

    for (base, entries) in indexed_values {
        let offset = if entries.contains_key(&0) {
            0usize
        } else {
            entries.keys().next().cloned().unwrap_or(0)
        };

        let mut ordered: Vec<Value> = Vec::new();
        for (index, value) in entries {
            let normalized = index.saturating_sub(offset);
            if normalized >= ordered.len() {
                ordered.resize(normalized + 1, Value::Null);
            }
            ordered[normalized] = value;
        }

        while matches!(ordered.last(), Some(Value::Null)) {
            ordered.pop();
        }

        object.insert(base, Value::Array(ordered));
    }

    if let Some(Value::String(command)) = object.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        object.insert("command".to_string(), Value::Array(array));
    }

    canonicalize_tool_result(name.to_string(), Value::Object(object))
}

fn parse_minimax_tool_call(text: &str) -> Option<(String, Value)> {
    const INVOKE_TAG: &str = "<invoke name=\"";
    const INVOKE_CLOSE: &str = "</invoke>";
    const PARAMETER_TAG: &str = "<parameter name=\"";
    const PARAMETER_CLOSE: &str = "</parameter>";

    let invoke_start = text.find(INVOKE_TAG)?;
    let invoke_rest = &text[invoke_start + INVOKE_TAG.len()..];
    let name_end = invoke_rest.find('"')?;
    let name = invoke_rest[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let after_name = &invoke_rest[name_end + 1..];
    let body_start = after_name.find('>')?;
    let after_invoke_tag = &after_name[body_start + 1..];
    let invoke_body_end = after_invoke_tag
        .find(INVOKE_CLOSE)
        .unwrap_or(after_invoke_tag.len());
    let mut rest = &after_invoke_tag[..invoke_body_end];

    let mut object = Map::new();
    let mut indexed_values: BTreeMap<String, BTreeMap<usize, Value>> = BTreeMap::new();

    while let Some(parameter_start) = rest.find(PARAMETER_TAG) {
        rest = &rest[parameter_start + PARAMETER_TAG.len()..];

        let parameter_name_end = match rest.find('"') {
            Some(index) => index,
            None => break,
        };
        let parameter_name = rest[..parameter_name_end].trim();
        if parameter_name.is_empty() {
            break;
        }

        let after_parameter_name = &rest[parameter_name_end + 1..];
        let value_start = match after_parameter_name.find('>') {
            Some(index) => index,
            None => break,
        };
        rest = &after_parameter_name[value_start + 1..];

        let value_end = rest.find(PARAMETER_CLOSE).unwrap_or(rest.len());
        let value = parse_scalar_value(rest[..value_end].trim());

        if let Some((base, index)) = split_indexed_key(parameter_name) {
            indexed_values
                .entry(base.to_string())
                .or_default()
                .insert(index, value);
        } else {
            object.insert(parameter_name.to_string(), value);
        }

        if value_end >= rest.len() {
            break;
        }
        rest = &rest[value_end + PARAMETER_CLOSE.len()..];
    }

    if object.is_empty() && indexed_values.is_empty() {
        return None;
    }

    for (base, entries) in indexed_values {
        let offset = if entries.contains_key(&0) {
            0usize
        } else {
            entries.keys().next().cloned().unwrap_or(0)
        };

        let mut ordered: Vec<Value> = Vec::new();
        for (index, value) in entries {
            let normalized = index.saturating_sub(offset);
            if normalized >= ordered.len() {
                ordered.resize(normalized + 1, Value::Null);
            }
            ordered[normalized] = value;
        }

        while matches!(ordered.last(), Some(Value::Null)) {
            ordered.pop();
        }

        object.insert(base, Value::Array(ordered));
    }

    if let Some(Value::String(command)) = object.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        object.insert("command".to_string(), Value::Array(array));
    }

    canonicalize_tool_result(name, Value::Object(object))
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
