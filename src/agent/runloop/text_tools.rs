use serde_json::{Map, Number, Value};
use shell_words::split as shell_split;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeFenceBlock {
    pub language: Option<String>,
    pub lines: Vec<String>,
}

pub(crate) fn extract_code_fence_blocks(text: &str) -> Vec<CodeFenceBlock> {
    let mut blocks = Vec::new();
    let mut current_language: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for raw_line in text.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some(rest) = trimmed_start.strip_prefix("```") {
            let rest_clean = rest.trim_matches('\r');
            let rest_trimmed = rest_clean.trim();
            if current_language.is_some() {
                if rest_trimmed.is_empty() {
                    let language = current_language.take().and_then(|lang| {
                        let cleaned = lang.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
                        let cleaned = cleaned.trim();
                        if cleaned.is_empty() {
                            None
                        } else {
                            Some(cleaned.to_string())
                        }
                    });
                    let block_lines = std::mem::take(&mut current_lines);
                    blocks.push(CodeFenceBlock {
                        language,
                        lines: block_lines,
                    });
                    continue;
                }
            } else {
                let token = rest_trimmed.split_whitespace().next().unwrap_or_default();
                let normalized = token
                    .trim_matches(|ch| matches!(ch, '"' | '\'' | '`'))
                    .trim();
                current_language = Some(normalized.to_ascii_lowercase());
                current_lines.clear();
                continue;
            }
        }

        if current_language.is_some() {
            current_lines.push(raw_line.trim_end_matches('\r').to_string());
        }
    }

    blocks
}

fn canonicalize_tool_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let trimmed = trimmed.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
    let mut normalized = String::new();
    let mut last_was_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if ch == '_' {
            normalized.push('_');
            last_was_separator = false;
        } else if matches!(ch, ' ' | '\t' | '\n' | '-' | ':' | '.')
            && !last_was_separator
            && !normalized.is_empty()
        {
            normalized.push('_');
            last_was_separator = true;
        }
    }

    let normalized = normalized.trim_matches('_').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn canonicalize_tool_result(name: String, args: Value) -> Option<(String, Value)> {
    canonicalize_tool_name(&name).map(|canonical| (canonical, args))
}

const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    if let Some((name, args)) = parse_tagged_tool_call(text) {
        if let Some(result) = canonicalize_tool_result(name, args) {
            return Some(result);
        }
    }

    if let Some((name, args)) = parse_rust_struct_tool_call(text) {
        if let Some(result) = canonicalize_tool_result(name, args) {
            return Some(result);
        }
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
            if let Some(args) = parse_textual_arguments(raw_args)
                && let Some(canonical) = canonicalize_tool_name(&name)
            {
                return Some((canonical, args));
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

fn parse_rust_struct_tool_call(text: &str) -> Option<(String, Value)> {
    let mut search = text;
    while let Some(start) = search.find("```") {
        let mut rest = &search[start + 3..];
        if let Some(newline) = rest.find('\n') {
            rest = &rest[newline + 1..];
        } else {
            return None;
        }

        let end = rest.find("```")?;
        let (block, after) = rest.split_at(end);
        search = &after[3..];

        if let Some((name, args)) = parse_structured_block(block) {
            return Some((name, args));
        }
    }
    None
}

fn parse_structured_block(block: &str) -> Option<(String, Value)> {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        return None;
    }

    let brace_index = trimmed.find('{')?;
    let raw_name = trimmed[..brace_index]
        .lines()
        .last()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let name = raw_name.trim().trim_end_matches([':', '=']).trim();
    if name.is_empty() {
        return None;
    }
    let name = name.to_string();

    let rest = trimmed[brace_index + 1..].trim_start();
    let mut depth = 1i32;
    let mut body_end = None;
    for (idx, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }

    let end_index = body_end?;
    let body = &rest[..end_index];

    let entries = split_top_level_entries(body);
    let mut object = Map::new();
    for entry in entries {
        if let Some((key, value)) = entry.split_once(':').or_else(|| entry.split_once('=')) {
            let key = key.trim().trim_matches('"').trim_matches('\'').to_string();
            if key.is_empty() {
                continue;
            }
            let parsed = parse_scalar_value(value.trim());
            object.insert(key, parsed);
        }
    }

    if object.is_empty() {
        return None;
    }

    if let Some(Value::String(command)) = object.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        object.insert("command".to_string(), Value::Array(array));
    }

    Some((name, Value::Object(object)))
}

fn split_top_level_entries(body: &str) -> Vec<String> {
    fn push_entry(entries: &mut Vec<String>, current: &mut String) {
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            entries.push(
                trimmed
                    .trim_end_matches(|ch| ch == ',' || ch == ';')
                    .trim()
                    .to_string(),
            );
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

    #[test]
    fn test_detect_tagged_tool_call_handles_one_based_indexes() {
        let message = "<tool_call>run_terminal_cmd\n<arg_key>command.1\n<arg_value>ls\n<arg_key>command.2\n<arg_value>-a\n</tool_call>";
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
    fn test_detect_rust_struct_tool_call_parses_command_block() {
        let message = "Here you go:\n```rust\nrun_terminal_cmd {\n    command: \"ls -a\",\n    workdir: \"/tmp\",\n    timeout: 5.0\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["ls", "-a"],
                "workdir": "/tmp",
                "timeout": 5.0
            })
        );
    }

    #[test]
    fn test_detect_rust_struct_tool_call_handles_trailing_commas() {
        let message = "```rust\nrun_terminal_cmd {\n    command: \"git status\",\n    workdir: \".\",\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["git", "status"],
                "workdir": "."
            })
        );
    }

    #[test]
    fn test_detect_rust_struct_tool_call_handles_semicolons() {
        let message =
            "```rust\nrun_terminal_cmd {\n    command = \"pwd\";\n    workdir = \"/tmp\";\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["pwd"],
                "workdir": "/tmp"
            })
        );
    }

    #[test]
    fn test_detect_structured_tool_call_handles_args_assignment() {
        let message = "```bash\nrun_terminal_cmd args={\n    command: \"ls -a\";\n    workdir: \"/tmp\";\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["ls", "-a"],
                "workdir": "/tmp"
            })
        );
    }

    #[test]
    fn test_detect_textual_tool_call_canonicalizes_name_variants() {
        let message = "```rust\nRun Terminal Cmd {\n    command = \"pwd\";\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args, serde_json::json!({ "command": ["pwd"] }));
    }

    #[test]
    fn test_extract_code_fence_blocks_collects_languages() {
        let message = "```bash\nTZ=Asia/Tokyo date +\"%Y-%m-%d %H:%M:%S %Z\"\n```\n```rust\nrun_terminal_cmd {\n    command: \"ls -a\"\n}\n```";
        let blocks = extract_code_fence_blocks(message);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language.as_deref(), Some("bash"));
        assert_eq!(
            blocks[0].lines,
            vec!["TZ=Asia/Tokyo date +\"%Y-%m-%d %H:%M:%S %Z\""]
        );
        assert_eq!(blocks[1].language.as_deref(), Some("rust"));
        assert_eq!(
            blocks[1].lines,
            vec!["run_terminal_cmd {", "    command: \"ls -a\"", "}"]
        );
    }
}
