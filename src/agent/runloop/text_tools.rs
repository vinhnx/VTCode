use serde_json::{Map, Number, Value};
use shell_words::split as split_shell_words;
use std::borrow::Cow;

const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];

const SHELL_CALL_PREFIXES: &[&str] = &["shell", "default_api.shell"];

const DIRECT_TOOL_NAMES: &[&str] = &[
    "run_terminal_cmd",
    "default_api.run_terminal_cmd",
    "bash",
    "default_api.bash",
];

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    let mut segments: Vec<Cow<'_, str>> = vec![Cow::Borrowed(text)];

    if let Some(stripped) = strip_code_fence(text) {
        segments.push(Cow::Owned(stripped));
    }

    for segment in segments.iter() {
        let segment = segment.as_ref();

        if let Some(result) = detect_shell_call(segment) {
            return Some(result);
        }

        if let Some(result) = detect_json_tool_call(segment) {
            return Some(result);
        }

        if let Some(result) = detect_direct_tool_call(segment) {
            return Some(result);
        }

        if let Some(result) = detect_prefixed_tool_call(segment) {
            return Some(result);
        }
    }

    None
}

fn detect_prefixed_tool_call(text: &str) -> Option<(String, Value)> {
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
            let Some(paren_index) = find_open_paren(after_name) else {
                search_start = start;
                continue;
            };

            let open_index = start + name_len + paren_index;
            let Some((args_start, args_end)) = locate_argument_span(text, open_index) else {
                search_start = open_index + 1;
                continue;
            };

            let raw_args = &text[args_start..args_end];
            if let Some(args) = parse_textual_arguments(raw_args) {
                return Some((name, args));
            }

            search_start = prefix_index + prefix.len() + name_len;
        }
    }
    None
}

fn detect_direct_tool_call(text: &str) -> Option<(String, Value)> {
    for name in DIRECT_TOOL_NAMES {
        let mut search_start = 0usize;
        while let Some(offset) = text[search_start..].find(name) {
            let index = search_start + offset;

            if index > 0 && is_identifier_char(text.chars().nth(index - 1)) {
                search_start = index + name.len();
                continue;
            }

            let after_name = &text[index + name.len()..];
            let Some(paren_index) = find_open_paren(after_name) else {
                search_start = index + name.len();
                continue;
            };

            let open_index = index + name.len() + paren_index;
            let Some((args_start, args_end)) = locate_argument_span(text, open_index) else {
                search_start = open_index + 1;
                continue;
            };

            let raw_args = &text[args_start..args_end];
            let normalized_name = normalize_tool_name(name);

            if let Some(args) = parse_textual_arguments(raw_args) {
                if let Value::Array(_) = args {
                    if let Some(mapped) =
                        parse_direct_tool_positional_arguments(&normalized_name, raw_args)
                    {
                        return Some((normalized_name, mapped));
                    }
                } else {
                    return Some((normalized_name, args));
                }
            }

            if let Some(args) = parse_direct_tool_positional_arguments(&normalized_name, raw_args) {
                return Some((normalized_name, args));
            }

            search_start = index + name.len();
        }
    }

    None
}

fn detect_json_tool_call(text: &str) -> Option<(String, Value)> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let value = try_parse_json_value(trimmed)?;
    match value {
        Value::Object(map) => extract_tool_from_object(map),
        _ => None,
    }
}

fn extract_tool_from_object(mut map: Map<String, Value>) -> Option<(String, Value)> {
    if let Some(Value::Object(mut function)) = map.remove("function") {
        if let Some(name_value) = function.remove("name") {
            let name = name_value.as_str()?.to_string();
            let base_args = function
                .remove("arguments")
                .or_else(|| function.remove("params"))
                .unwrap_or(Value::Object(Map::new()));
            let args = finalize_json_arguments(&name, base_args, vec![map, function]);
            return Some((normalize_tool_name(&name), args));
        }
    }

    if let Some(tool_call_value) = map.remove("tool_call") {
        return extract_tool_from_tool_call(tool_call_value, map);
    }

    if let Some(tool_value) = map.remove("tool") {
        return extract_tool_from_value(tool_value, map);
    }

    if let Some(name_value) = map.remove("name") {
        if let Some(name) = name_value.as_str() {
            let base_args = map
                .remove("params")
                .or_else(|| map.remove("arguments"))
                .unwrap_or(Value::Object(Map::new()));
            let args = finalize_json_arguments(name, base_args, vec![map]);
            return Some((normalize_tool_name(name), args));
        }
    }

    None
}

fn extract_tool_from_value(
    tool_value: Value,
    mut remainder: Map<String, Value>,
) -> Option<(String, Value)> {
    match tool_value {
        Value::String(name) => {
            let base_args = remainder
                .remove("params")
                .or_else(|| remainder.remove("arguments"))
                .unwrap_or(Value::Object(Map::new()));
            let args = finalize_json_arguments(&name, base_args, vec![remainder]);
            Some((normalize_tool_name(&name), args))
        }
        Value::Object(mut inner) => {
            if let Some(name_value) = inner
                .remove("name")
                .or_else(|| inner.remove("tool"))
                .or_else(|| inner.remove("id"))
            {
                if let Some(name) = name_value.as_str() {
                    let base_args = inner
                        .remove("arguments")
                        .or_else(|| inner.remove("params"))
                        .unwrap_or(Value::Object(Map::new()));
                    let args = finalize_json_arguments(name, base_args, vec![remainder, inner]);
                    return Some((normalize_tool_name(name), args));
                }
            }
            None
        }
        _ => None,
    }
}

fn extract_tool_from_tool_call(
    tool_call_value: Value,
    mut remainder: Map<String, Value>,
) -> Option<(String, Value)> {
    match tool_call_value {
        Value::Object(mut inner) => {
            let name_value = inner
                .remove("name")
                .or_else(|| inner.remove("tool"))
                .or_else(|| inner.remove("id"))?;
            let name = name_value.as_str()?.to_string();
            let base_args = inner
                .remove("params")
                .or_else(|| inner.remove("arguments"))
                .unwrap_or(Value::Object(Map::new()));

            if !inner.is_empty() {
                remainder.extend(inner);
            }

            let mut extras = Vec::new();
            if !remainder.is_empty() {
                extras.push(remainder);
            }

            let args = finalize_json_arguments(&name, base_args, extras);
            Some((normalize_tool_name(&name), args))
        }
        Value::Array(items) => {
            for item in items {
                if let Some(result) = extract_tool_from_tool_call(item, remainder.clone()) {
                    return Some(result);
                }
            }
            None
        }
        Value::String(name) => {
            let mut extras = Vec::new();
            if !remainder.is_empty() {
                extras.push(remainder);
            }
            let args = finalize_json_arguments(&name, Value::Object(Map::new()), extras);
            Some((normalize_tool_name(&name), args))
        }
        _ => None,
    }
}

fn finalize_json_arguments(
    tool_name: &str,
    base_args: Value,
    extras: Vec<Map<String, Value>>,
) -> Value {
    match base_args {
        Value::Object(mut obj) => {
            let canonicalize_terminal = is_terminal_tool(tool_name);
            if canonicalize_terminal {
                normalize_terminal_argument_keys(&mut obj);
            }
            merge_extra_maps(&mut obj, extras, canonicalize_terminal);
            Value::Object(obj)
        }
        Value::Array(items) => {
            let mut obj = Map::new();
            let key = if is_terminal_tool(tool_name) {
                "command"
            } else {
                "args"
            };
            obj.insert(key.to_string(), Value::Array(items));
            let canonicalize_terminal = is_terminal_tool(tool_name);
            merge_extra_maps(&mut obj, extras, canonicalize_terminal);
            Value::Object(obj)
        }
        Value::Null => {
            let mut obj = Map::new();
            let canonicalize_terminal = is_terminal_tool(tool_name);
            merge_extra_maps(&mut obj, extras, canonicalize_terminal);
            Value::Object(obj)
        }
        other => {
            let mut obj = Map::new();
            let key = if is_terminal_tool(tool_name) {
                "command"
            } else {
                "value"
            };
            obj.insert(key.to_string(), other);
            let canonicalize_terminal = is_terminal_tool(tool_name);
            merge_extra_maps(&mut obj, extras, canonicalize_terminal);
            Value::Object(obj)
        }
    }
}

fn merge_extra_maps(
    target: &mut Map<String, Value>,
    extras: Vec<Map<String, Value>>,
    canonicalize_terminal: bool,
) {
    for mut extra in extras {
        if canonicalize_terminal {
            normalize_terminal_argument_keys(&mut extra);
        }
        for (key, value) in extra.into_iter() {
            target.entry(key).or_insert(value);
        }
    }
}

fn normalize_terminal_argument_keys(map: &mut Map<String, Value>) {
    if let Some(value) = map.remove("workdir") {
        map.entry("working_dir".to_string()).or_insert(value);
    }
    if let Some(value) = map.remove("cwd") {
        map.entry("working_dir".to_string()).or_insert(value);
    }
    if let Some(value) = map.remove("timeout") {
        map.entry("timeout_secs".to_string()).or_insert(value);
    }
}

fn strip_code_fence(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let start_idx = trimmed.find("```")?;
    let after_start = &trimmed[start_idx + 3..];
    let newline_offset = after_start.find('\n')?;
    let body_start = start_idx + 3 + newline_offset + 1;
    let remaining = &trimmed[body_start..];
    let end_rel = remaining.rfind("```")?;
    let body_end = body_start + end_rel;
    Some(trimmed[body_start..body_end].trim().to_string())
}

fn find_open_paren(text: &str) -> Option<usize> {
    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            continue;
        }
        if ch == '(' {
            return Some(idx);
        }
        break;
    }
    None
}

fn locate_argument_span(text: &str, open_index: usize) -> Option<(usize, usize)> {
    let mut depth = 1i32;
    let mut end: Option<usize> = None;
    for (rel_idx, ch) in text[open_index + 1..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(open_index + 1 + rel_idx);
                    break;
                }
            }
            _ => {}
        }
    }

    end.map(|end_index| (open_index + 1, end_index))
}

fn is_identifier_char(ch: Option<char>) -> bool {
    matches!(ch, Some(c) if c.is_ascii_alphanumeric() || c == '_')
}

fn normalize_tool_name(name: &str) -> String {
    name.trim_start_matches("default_api.").to_string()
}

fn is_terminal_tool(name: &str) -> bool {
    matches!(
        name.trim_start_matches("default_api."),
        "run_terminal_cmd" | "bash"
    )
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

fn parse_direct_tool_positional_arguments(name: &str, raw_args: &str) -> Option<Value> {
    let positional = parse_positional_argument_list(raw_args)?;

    match name {
        "run_terminal_cmd" => build_run_terminal_cmd_arguments(positional),
        "bash" => build_bash_arguments(positional),
        _ => None,
    }
}

fn build_run_terminal_cmd_arguments(positional: Vec<Value>) -> Option<Value> {
    if positional.is_empty() {
        return None;
    }

    if let Some(Value::Object(map)) = positional.first() {
        // Allow callers to pass the canonical object form directly.
        return Some(Value::Object(map.clone()));
    }

    let mut command_tokens = match positional[0].clone() {
        Value::String(command) => parse_shell_tokens(&command)?,
        Value::Array(items) => {
            let mut tokens = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::String(token) => tokens.push(token),
                    _ => return None,
                }
            }
            if tokens.is_empty() {
                return None;
            }
            tokens
        }
        _ => return None,
    };

    if command_tokens.is_empty() {
        return None;
    }

    let mut map = Map::new();
    let command_values = command_tokens
        .drain(..)
        .map(Value::String)
        .collect::<Vec<Value>>();
    map.insert("command".to_string(), Value::Array(command_values));

    if let Some(value) = positional.get(1).filter(|value| !value.is_null()) {
        map.insert("working_dir".to_string(), value.clone());
    }

    if let Some(value) = positional.get(2).filter(|value| !value.is_null()) {
        map.insert("timeout_secs".to_string(), value.clone());
    }

    if let Some(value) = positional.get(3).filter(|value| !value.is_null()) {
        map.insert("mode".to_string(), value.clone());
    }

    if let Some(value) = positional.get(4).filter(|value| !value.is_null()) {
        map.insert("response_format".to_string(), value.clone());
    }

    Some(Value::Object(map))
}

fn build_bash_arguments(positional: Vec<Value>) -> Option<Value> {
    if positional.is_empty() {
        return None;
    }

    if let Some(Value::Object(map)) = positional.first() {
        return Some(Value::Object(map.clone()));
    }

    let mut map = Map::new();
    match positional[0].clone() {
        Value::String(command) => {
            map.insert("command".to_string(), Value::String(command));
        }
        Value::Array(items) => {
            map.insert("args".to_string(), Value::Array(items));
        }
        _ => return None,
    }

    if let Some(value) = positional.get(1).filter(|value| !value.is_null()) {
        map.insert("timeout_secs".to_string(), value.clone());
    }

    if let Some(value) = positional.get(2).filter(|value| !value.is_null()) {
        map.insert("working_dir".to_string(), value.clone());
    }

    Some(Value::Object(map))
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

    if let Some(values) = parse_positional_argument_list(trimmed) {
        return Some(Value::Array(values));
    }

    parse_key_value_arguments(trimmed)
}

fn parse_positional_argument_list(input: &str) -> Option<Vec<Value>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut escape = false;
    let mut string_delim = '\0';
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut brace_depth = 0usize;

    for ch in input.chars() {
        if in_string {
            current.push(ch);
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '\'' | '"' => {
                in_string = true;
                string_delim = ch;
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                current.push(ch);
            }
            '[' => {
                bracket_depth += 1;
                current.push(ch);
            }
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                current.push(ch);
            }
            '{' => {
                brace_depth += 1;
                current.push(ch);
            }
            '}' => {
                brace_depth = brace_depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    args.push(parse_positional_value(trimmed)?);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    let trimmed = current.trim();
    if !trimmed.is_empty() {
        args.push(parse_positional_value(trimmed)?);
    }

    if args.is_empty() { None } else { Some(args) }
}

fn parse_positional_value(segment: &str) -> Option<Value> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(value) = try_parse_json_value(trimmed) {
        return Some(value);
    }

    if trimmed.eq_ignore_ascii_case("none") {
        return Some(Value::Null);
    }

    if let Some(stripped) = trimmed
        .strip_prefix("Some(")
        .and_then(|inner| inner.strip_suffix(')'))
    {
        return parse_positional_value(stripped);
    }

    if let Some(nested) = trimmed
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .and_then(parse_positional_argument_list)
    {
        return Some(Value::Array(nested));
    }

    Some(parse_scalar_value(trimmed))
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
        "null" | "none" => return Value::Null,
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

    #[test]
    fn test_detect_textual_tool_call_parses_json_tool_object() {
        let message = r#"{"tool":"run_terminal_cmd","params":{"command":"pwd","timeout_secs":5}}"#;
        let (name, args) = detect_textual_tool_call(message).expect("should parse json tool");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": "pwd",
                "timeout_secs": 5
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_parses_code_fenced_json_tool() {
        let message = "```json\n{\n  \"tool\": \"run_terminal_cmd\",\n  \"params\": {\n    \"command\": [\"ls\", \"-a\"],\n    \"working_dir\": \"app\"\n  }\n}\n```";
        let (name, args) =
            detect_textual_tool_call(message).expect("should parse code fenced json");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["ls", "-a"],
                "working_dir": "app"
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_handles_direct_run_terminal_cmd_invocation() {
        let message = "```shell\nrun_terminal_cmd(command=\"git diff\", timeout=20)\n```";
        let (name, args) =
            detect_textual_tool_call(message).expect("should parse direct invocation");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": "git diff",
                "timeout": 20
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_parses_positional_run_terminal_cmd_arguments() {
        let message = "run_terminal_cmd(\"pwd\", None, 1000, \"terminal\")";
        let (name, args) =
            detect_textual_tool_call(message).expect("should parse positional arguments");

        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["pwd"],
                "timeout_secs": 1000,
                "mode": "terminal"
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_parses_default_prefixed_positional_command() {
        let message = "default_api.run_terminal_cmd(\"git diff\", \"/tmp\")";
        let (name, args) =
            detect_textual_tool_call(message).expect("should parse prefixed positional call");

        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["git", "diff"],
                "working_dir": "/tmp"
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_parses_tool_call_object() {
        let message = r#"```json
{
  "tool_call": {
    "name": "run_terminal_cmd",
    "params": {
      "command": ["pwd"],
      "workdir": "src"
    }
  }
}
```"#;

        let (name, args) =
            detect_textual_tool_call(message).expect("should parse tool_call wrapper");

        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["pwd"],
                "working_dir": "src"
            })
        );
    }
}
