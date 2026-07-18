use serde_json::{Map, Number, Value};
use shell_words::split as shell_split;

/// Shared delimiter matcher for all parsers.
/// Returns the index of the matching close delimiter in `text`.
/// `start` is the index of the open delimiter.
/// Tracks both the specific delimiter pair AND total nesting depth across all delimiter types
/// to detect excessively deep mixed nesting.
pub(super) fn find_matching_delimiter(
    text: &str,
    start: usize,
    open: char,
    close: char,
    max_depth: usize,
) -> Option<usize> {
    let mut target_depth = 0usize;
    let mut total_depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (relative, ch) in text[start..].char_indices() {
        if let Some(delimiter) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == delimiter {
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            continue;
        }

        // Track all opening delimiters for total depth
        if matches!(ch, '(' | '{' | '[') {
            total_depth += 1;
            if total_depth > max_depth {
                tracing::warn!(
                    total_nesting_depth = total_depth,
                    max_nesting_depth = max_depth,
                    open = %open,
                    close = %close,
                    "Rejected delimiter matching due to excessive mixed nesting"
                );
                return None;
            }
            if ch == open {
                target_depth += 1;
            }
        } else if matches!(ch, ')' | '}' | ']') {
            total_depth = total_depth.saturating_sub(1);
            if ch == close {
                if target_depth == 0 {
                    tracing::debug!(
                        close = %close,
                        "Rejected delimiter matching due to unmatched closing delimiter"
                    );
                    return None;
                }
                target_depth -= 1;
                if target_depth == 0 {
                    return Some(start + relative);
                }
            }
        }
    }

    None
}

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

    // Try standard JSON parsing first
    if let Ok(val) = serde_json::from_str(trimmed) {
        return Some(val);
    }

    // If input contains single quotes and no double quotes inside strings,
    // attempt single-quoted JSON parsing (minimal tokenizer)
    if trimmed.contains('\'') && !has_double_quotes_in_strings(trimmed) {
        let normalized = convert_single_quoted_json(trimmed);
        if let Ok(val) = serde_json::from_str(&normalized) {
            return Some(val);
        }
    }

    None
}

fn has_double_quotes_in_strings(input: &str) -> bool {
    let mut in_single = false;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '\'' {
            in_single = !in_single;
        } else if ch == '"' && in_single {
            return true;
        }
    }
    false
}

fn convert_single_quoted_json(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_single = false;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            result.push(ch);
            escaped = true;
            continue;
        }
        if ch == '\'' {
            result.push('"');
            in_single = !in_single;
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn parse_key_value_arguments(input: &str) -> Option<Value> {
    let mut map = Map::new();

    // Use quote-aware splitting instead of naive split(',') to handle values like "a,b"
    for segment in split_function_arguments(input) {
        let pair = segment.trim().trim_end_matches(';').trim();
        if pair.is_empty() {
            continue;
        }

        let (key_raw, value_raw) = pair.split_once('=').or_else(|| pair.split_once(':'))?;

        let key = key_raw.trim().trim_matches('"').trim_matches('\'').to_string();

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
    if fallback.is_empty() { None } else { Some(fallback) }
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
