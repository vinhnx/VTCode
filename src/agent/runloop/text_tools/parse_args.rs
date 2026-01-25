use serde_json::{Map, Number, Value};
use shell_words::split as shell_split;

pub(super) fn parse_textual_arguments(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Value::Object(Map::new()));
    }

    if let Some(val) = try_parse_json_value(trimmed) {
        return Some(val);
    }

    parse_key_value_arguments(trimmed)
}

pub(super) fn try_parse_json_value(input: &str) -> Option<Value> {
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

pub(super) fn parse_key_value_arguments(input: &str) -> Option<Value> {
    let mut map = Map::new();

    for segment in input.split(',') {
        let pair = segment.trim().trim_end_matches(';').trim();
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
        return None;
    }

    if let Some(Value::String(command)) = map.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        map.insert("command".to_string(), Value::Array(array));
    }

    Some(Value::Object(map))
}

pub(super) fn normalize_command_string(command: &str) -> Option<Vec<Value>> {
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

pub(super) fn parse_scalar_value(input: &str) -> Value {
    if let Some(val) = try_parse_json_value(input) {
        return val;
    }

    let trimmed = input.trim();
    let trimmed = trimmed.trim_end_matches(&[',', ';'][..]);
    let trimmed = trimmed.trim();
    let trimmed = trimmed.trim_matches('"').trim_matches('\'').trim();
    let trimmed = trimmed.to_string();
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

pub(super) fn split_top_level_entries(body: &str) -> Vec<String> {
    fn push_entry(entries: &mut Vec<String>, current: &mut String) {
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            entries.push(trimmed.trim_end_matches([',', ';']).trim().to_string());
        }
        current.clear();
    }

    let mut entries = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;

    for ch in body.chars() {
        match ch {
            '{' | '[' => {
                depth += 1;
                current.push(ch);
            }
            '}' | ']' => {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(ch);
            }
            ',' if depth == 0 => {
                push_entry(&mut entries, &mut current);
            }
            '\n' | '\r' => {
                if depth == 0 {
                    push_entry(&mut entries, &mut current);
                }
            }
            _ => current.push(ch),
        }
    }

    push_entry(&mut entries, &mut current);

    entries
}

pub(super) fn split_function_arguments(body: &str) -> Vec<String> {
    fn push_arg(entries: &mut Vec<String>, current: &mut String) {
        let trimmed = current.trim().trim_end_matches([',', ';']).trim();
        if !trimmed.is_empty() {
            entries.push(trimmed.to_string());
        }
        current.clear();
    }

    let mut entries = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    let mut string_delim: Option<char> = None;
    let mut chars = body.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '"' | '\'' => {
                current.push(ch);
                if let Some(delim) = string_delim {
                    if delim == ch {
                        string_delim = None;
                    }
                } else {
                    string_delim = Some(ch);
                }
            }
            '(' | '{' | '[' if string_delim.is_none() => {
                depth += 1;
                current.push(ch);
            }
            ')' | '}' | ']' if string_delim.is_none() => {
                if depth > 0 {
                    depth -= 1;
                }
                current.push(ch);
            }
            ',' if string_delim.is_none() && depth == 0 => {
                push_arg(&mut entries, &mut current);
            }
            _ => current.push(ch),
        }
    }

    push_arg(&mut entries, &mut current);
    entries
}

pub(super) fn split_indexed_key(key: &str) -> Option<(&str, usize)> {
    let (base, index_str) = key.rsplit_once('.')?;
    let index = index_str.parse().ok()?;
    Some((base, index))
}
