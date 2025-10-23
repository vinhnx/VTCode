use serde_json::{Map, Number, Value};
use shell_words::split as shell_split;
use std::collections::BTreeMap;
use vtcode_core::config::constants::tools;

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
    } else if matches!(
        normalized.as_str(),
        "run"
            | "runcmd"
            | "runcommand"
            | "terminalrun"
            | "terminalcmd"
            | "terminalcommand"
            | "command"
    ) {
        Some(tools::RUN_TERMINAL_CMD.to_string())
    } else {
        Some(normalized)
    }
}

fn canonicalize_tool_result(name: String, args: Value) -> Option<(String, Value)> {
    let canonical = canonicalize_tool_name(&name)?;
    if is_known_textual_tool(&canonical) {
        Some((canonical, args))
    } else {
        None
    }
}

const TEXTUAL_TOOL_PREFIXES: &[&str] = &["default_api."];
const DIRECT_FUNCTION_ALIASES: &[&str] = &[
    "run_terminal_cmd",
    "run_terminal_command",
    "run",
    "run_cmd",
    "runcommand",
    "terminalrun",
    "terminal_cmd",
    "terminalcommand",
];

fn is_known_textual_tool(name: &str) -> bool {
    matches!(
        name,
        tools::WRITE_FILE
            | tools::EDIT_FILE
            | tools::READ_FILE
            | tools::RUN_TERMINAL_CMD
            | tools::BASH
            | tools::CURL
            | tools::GREP_FILE
            | tools::LIST_FILES
            | tools::UPDATE_PLAN
            | tools::AST_GREP_SEARCH
            | tools::SIMPLE_SEARCH
            | tools::SRGN
            | tools::APPLY_PATCH
            | tools::READ_PTY_SESSION
            | tools::RUN_PTY_CMD
            | tools::SEND_PTY_INPUT
            | tools::RESIZE_PTY_SESSION
            | tools::LIST_PTY_SESSIONS
            | tools::CLOSE_PTY_SESSION
            | tools::CREATE_PTY_SESSION
    )
}

pub(crate) fn detect_textual_tool_call(text: &str) -> Option<(String, Value)> {
    // Try gpt-oss channel format first
    if let Some((name, args)) = parse_channel_tool_call(text) {
        if let Some(result) = canonicalize_tool_result(name, args) {
            return Some(result);
        }
    }

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

    if let Some((name, args)) = parse_yaml_tool_call(text) {
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

    if let Some(result) = detect_direct_function_alias(text) {
        return Some(result);
    }
    None
}

fn detect_direct_function_alias(text: &str) -> Option<(String, Value)> {
    let lowered = text.to_ascii_lowercase();

    for alias in DIRECT_FUNCTION_ALIASES {
        let alias_lower = alias.to_ascii_lowercase();
        let mut search_start = 0usize;

        while let Some(offset) = lowered[search_start..].find(&alias_lower) {
            let start = search_start + offset;
            let end = start + alias_lower.len();

            if start > 0 {
                if let Some(prev) = lowered[..start].chars().rev().next() {
                    if prev.is_ascii_alphanumeric() || prev == '_' {
                        search_start = end;
                        continue;
                    }
                }
            }

            let mut paren_index: Option<usize> = None;
            let mut iter = text[end..].char_indices();
            while let Some((relative, ch)) = iter.next() {
                if ch.is_whitespace() {
                    continue;
                }
                if ch == '(' {
                    paren_index = Some(end + relative);
                }
                break;
            }

            let Some(paren_pos) = paren_index else {
                search_start = end;
                continue;
            };

            let args_start = paren_pos + 1;
            let mut depth = 1i32;
            let mut args_end: Option<usize> = None;
            for (relative, ch) in text[args_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            args_end = Some(args_start + relative);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let Some(end_pos) = args_end else {
                search_start = end;
                continue;
            };

            let raw_args = &text[args_start..end_pos];
            if let Some(args) = parse_textual_arguments(raw_args)
                && let Some(canonical) = canonicalize_tool_name(alias)
            {
                return Some((canonical, args));
            }

            search_start = end;
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
        return None;
    }

    if let Some(Value::String(command)) = map.get("command").cloned()
        && let Some(array) = normalize_command_string(&command)
    {
        map.insert("command".to_string(), Value::Array(array));
    }

    Some(Value::Object(map))
}

fn parse_channel_tool_call(text: &str) -> Option<(String, Value)> {
    // Format: <|start|>assistant<|channel|>commentary to=container.exec code<|constrain|>json<|message|>{"cmd":...}<|call|>
    // Also supports: <|start|>assistant<|channel|>commentary to=container.exec code<|message|>{"cmd":...}<|call|>
    let channel_start = text.find("<|channel|>")?;
    let message_start = text.find("<|message|>")?;
    let call_end = text.find("<|call|>")?;

    if message_start <= channel_start || call_end <= message_start {
        return None;
    }

    // Extract the channel commentary to find tool name
    // Handle both formats: with and without <|constrain|> tag
    let channel_end =
        if let Some(constrain_pos) = text[channel_start..message_start].find("<|constrain|>") {
            channel_start + "<|channel|>".len() + constrain_pos
        } else {
            message_start
        };

    let channel_text = &text[channel_start + "<|channel|>".len()..channel_end];

    // Parse tool name from channel commentary
    let tool_name = if let Some(to_pos) = channel_text.find("to=") {
        let after_to = &channel_text[to_pos + 3..];
        if let Some(space_pos) = after_to.find(' ') {
            let tool_ref = &after_to[..space_pos];
            parse_tool_name_from_reference(tool_ref)
        } else {
            parse_tool_name_from_reference(after_to.trim())
        }
    } else if channel_text.contains("container.exec") || channel_text.contains("exec") {
        "run_terminal_cmd"
    } else if channel_text.contains("read") || channel_text.contains("file") {
        "read_file"
    } else {
        // Default to terminal command
        "run_terminal_cmd"
    };

    // Extract JSON from message
    let json_text = &text[message_start + "<|message|>".len()..call_end].trim();

    // Parse the JSON
    let parsed: Value = serde_json::from_str(json_text).ok()?;

    // Convert to expected format based on tool name and arguments
    let args = convert_harmony_args_to_tool_format(tool_name, parsed);

    Some((tool_name.to_string(), args))
}

fn parse_tool_name_from_reference(tool_ref: &str) -> &str {
    match tool_ref {
        "repo_browser.list_files" | "list_files" => "list_files",
        "repo_browser.read_file" | "read_file" => "read_file",
        "repo_browser.write_file" | "write_file" => "write_file",
        "container.exec" | "exec" => "run_terminal_cmd",
        "bash" => "bash",
        "curl" => "curl",
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

fn convert_harmony_args_to_tool_format(tool_name: &str, parsed: Value) -> Value {
    match tool_name {
        "run_terminal_cmd" | "bash" => {
            if let Some(cmd) = parsed.get("cmd").and_then(|v| v.as_array()) {
                let command: Vec<String> = cmd
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();

                serde_json::json!({
                    "command": command
                })
            } else if let Some(cmd_str) = parsed.get("cmd").and_then(|v| v.as_str()) {
                serde_json::json!({
                    "command": [cmd_str]
                })
            } else {
                parsed
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

            Value::Object(args)
        }
        _ => parsed,
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

    if let Some(result) = parse_function_call_block(trimmed) {
        return Some(result);
    }

    let brace_index = trimmed.find('{')?;
    let raw_name = trimmed[..brace_index]
        .lines()
        .last()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    // Check if the name contains an assignment like "run_terminal_cmd args=" or "run_terminal_cmd args ="
    let name = if let Some(pos) = raw_name.find(" args=") {
        // Extract just the function name part before " args="
        raw_name[..pos].trim().to_string()
    } else if let Some(pos) = raw_name.find(" args =") {
        // Extract just the function name part before " args ="
        raw_name[..pos].trim().to_string()
    } else {
        // Normal case - trim end of colons and equals
        raw_name
            .trim()
            .trim_end_matches([':', '='])
            .trim()
            .to_string()
    };

    if name.is_empty() {
        return None;
    }

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

fn parse_yaml_tool_call(text: &str) -> Option<(String, Value)> {
    for segment in text.split("```") {
        if segment.trim().is_empty() {
            continue;
        }
        if let Some(result) = parse_yaml_tool_block(segment) {
            return Some(result);
        }
    }
    parse_yaml_tool_block(text)
}

fn parse_function_call_block(block: &str) -> Option<(String, Value)> {
    let trimmed = block.trim();
    if !trimmed.contains('(') {
        return None;
    }

    let mut open_index = None;
    for (idx, ch) in trimmed.char_indices() {
        if ch == '(' {
            open_index = Some(idx);
            break;
        }
    }
    let open_index = open_index?;
    let name = trimmed[..open_index].trim();
    if name.is_empty() {
        return None;
    }

    let mut depth = 0i32;
    let mut close_index = None;
    for (offset, ch) in trimmed[open_index..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close_index = Some(open_index + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close_index = close_index?;
    let args_body = trimmed[open_index + 1..close_index].trim();

    let canonical = canonicalize_tool_name(name);
    if canonical.is_none() {
        return None;
    }

    let mut object = Map::new();
    let mut positional: Vec<Value> = Vec::new();
    for entry in split_function_arguments(args_body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        if let Some((key_raw, value_raw)) = entry.split_once('=').or_else(|| entry.split_once(':'))
        {
            let key = key_raw
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            let value = parse_scalar_value(value_raw.trim());
            object.insert(key, value);
        } else {
            positional.push(parse_scalar_value(entry));
        }
    }

    if !positional.is_empty() {
        match canonical.as_deref() {
            Some(tools::RUN_TERMINAL_CMD) | Some(tools::BASH) => {
                if !object.contains_key("command") {
                    let mut positional_parts = Vec::new();
                    let mut all_strings = true;
                    for value in &positional {
                        if let Value::String(part) = value {
                            positional_parts.push(part.clone());
                        } else {
                            all_strings = false;
                            break;
                        }
                    }

                    if all_strings && !positional_parts.is_empty() {
                        if positional_parts.len() == 1 {
                            let command = &positional_parts[0];
                            if let Some(array) = normalize_command_string(command) {
                                object.insert("command".to_string(), Value::Array(array));
                            } else {
                                object
                                    .insert("command".to_string(), Value::String(command.clone()));
                            }
                        } else {
                            let array = positional_parts
                                .into_iter()
                                .map(Value::String)
                                .collect::<Vec<_>>();
                            object.insert("command".to_string(), Value::Array(array));
                        }
                    } else if let Some(Value::String(command)) = positional.first() {
                        if let Some(array) = normalize_command_string(command) {
                            object.insert("command".to_string(), Value::Array(array));
                        } else {
                            object.insert("command".to_string(), Value::String(command.clone()));
                        }
                    }
                }
            }
            _ => {
                // We don't have a reliable mapping for positional arguments on other tools.
                return None;
            }
        }
    }

    Some((name.to_string(), Value::Object(object)))
}

fn parse_yaml_tool_block(block: &str) -> Option<(String, Value)> {
    let mut lines = block.lines().map(|line| line.trim_end()).peekable();
    let mut name = None;
    const LANGUAGE_HINTS: &[&str] = &[
        "rust",
        "bash",
        "shell",
        "python",
        "json",
        "yaml",
        "toml",
        "javascript",
        "typescript",
        "markdown",
        "text",
    ];

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }
        if LANGUAGE_HINTS.contains(&trimmed.to_ascii_lowercase().as_str()) {
            continue;
        }
        name = Some(trimmed.trim_end_matches(':').to_string());
        break;
    }

    let name = name?;
    if name.is_empty() {
        return None;
    }

    let mut object = Map::new();
    let mut pending_key: Option<String> = None;
    let mut multiline_buffer: Vec<String> = Vec::new();
    let mut multiline_indent: Option<usize> = None;

    while let Some(line) = lines.next() {
        let raw = line;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            if pending_key.is_some() {
                multiline_buffer.push(String::new());
            }
            continue;
        }

        if pending_key.is_some() && (raw.starts_with(' ') || raw.starts_with('\t')) {
            let indent = raw.chars().take_while(|c| c.is_whitespace()).count();
            let content = if let Some(expected) = multiline_indent {
                if indent >= expected {
                    raw[expected..].to_string()
                } else {
                    raw.trim_start().to_string()
                }
            } else {
                multiline_indent = Some(indent);
                raw[indent..].to_string()
            };
            multiline_buffer.push(content);
            continue;
        }

        if let Some(key) = pending_key.take() {
            let joined = multiline_buffer.join("\n");
            object.insert(key, Value::String(joined));
            multiline_buffer.clear();
            multiline_indent = None;
        }

        if let Some((key_raw, value_raw)) = trimmed.split_once(':') {
            let key = key_raw.trim();
            let value = value_raw.trim();
            if key.is_empty() {
                continue;
            }
            if value == "|" {
                pending_key = Some(key.to_string());
                continue;
            }
            let parsed = parse_scalar_value(value);
            object.insert(key.to_string(), parsed);
        }
    }

    if let Some(key) = pending_key.take() {
        let joined = multiline_buffer.join("\n");
        object.insert(key, Value::String(joined));
    }

    if object.is_empty() {
        None
    } else {
        Some((name, Value::Object(object)))
    }
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

fn split_function_arguments(body: &str) -> Vec<String> {
    fn push_arg(entries: &mut Vec<String>, current: &mut String) {
        let trimmed = current
            .trim()
            .trim_end_matches(|ch| ch == ',' || ch == ';')
            .trim();
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
            '\"' | '\'' => {
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
    fn test_detect_textual_tool_call_parses_function_style_block() {
        let message =
            "```rust\nrun_terminal_cmd(\"ls -a\", workdir=WORKSPACE_DIR, max_lines=100)\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args["command"], serde_json::json!(["ls", "-a"]));
        assert_eq!(args["workdir"], serde_json::json!("WORKSPACE_DIR"));
        assert_eq!(args["max_lines"], serde_json::json!(100));
    }

    #[test]
    fn test_detect_textual_tool_call_skips_non_tool_function_blocks() {
        let message = "```rust\nprintf!(\"hi\");\n```\n```rust\nrun_terminal_cmd {\n    command: \"pwd\"\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args["command"], serde_json::json!(["pwd"]));
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
    fn test_detect_rust_struct_tool_call_maps_run_alias() {
        let message = "```rust\nrun {\n    command: \"ls\",\n    args: [\"-a\"]\n}\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["ls"],
                "args": ["-a"]
            })
        );
    }

    #[test]
    fn test_detect_textual_function_maps_run_alias() {
        let message = "run(command: \"npm\", args: [\"test\"])";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(
            args,
            serde_json::json!({
                "command": ["npm"],
                "args": ["test"]
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
    fn test_detect_yaml_tool_call_with_multiline_content() {
        let message = "```rust\nwrite_file\npath: /tmp/hello.txt\ncontent: |\n  Line one\n  Line two\nmode: overwrite\n```";
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "write_file");
        assert_eq!(args["path"], serde_json::json!("/tmp/hello.txt"));
        assert_eq!(args["mode"], serde_json::json!("overwrite"));
        assert_eq!(args["content"], serde_json::json!("Line one\nLine two"));
    }

    #[test]
    fn test_detect_yaml_tool_call_ignores_language_hint_lines() {
        let message = "Rust block\n\n```yaml\nwrite_file\npath: /tmp/hello.txt\ncontent: hi\nmode: overwrite\n```";
        let (name, _) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, "write_file");
    }

    #[test]
    fn test_detect_yaml_tool_call_matches_complex_message() {
        let message = r#"Planned steps:
- Ensure directory exists

I'll create a hello world file named hellovinhnx.md in the workspace root.

```rust
write_file
path: /Users/example/workspace/hellovinhnx.md
content: Hello, VinhNX!\n\nThis is a simple hello world file created for you.\nIt demonstrates basic file creation in the VT Code workspace.
mode: overwrite
```
"#;
        let (name, args) = detect_textual_tool_call(message).expect("should parse");
        assert_eq!(name, tools::WRITE_FILE);
        assert_eq!(
            args["path"],
            serde_json::json!("/Users/example/workspace/hellovinhnx.md")
        );
        assert_eq!(args["mode"], serde_json::json!("overwrite"));
        assert_eq!(
            args["content"],
            serde_json::json!(
                "Hello, VinhNX!\\n\\nThis is a simple hello world file created for you.\\nIt demonstrates basic file creation in the VT Code workspace."
            )
        );
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

    #[test]
    fn test_parse_harmony_channel_tool_call_with_constrain() {
        let message = "<|start|>assistant<|channel|>commentary to=repo_browser.list_files <|constrain|>json<|message|>{\"path\":\"\", \"recursive\":\"true\"}<|call|>";
        let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
        assert_eq!(name, "list_files");
        assert_eq!(args["path"], serde_json::json!(""));
        assert_eq!(args["recursive"], serde_json::json!("true"));
    }

    #[test]
    fn test_parse_harmony_channel_tool_call_without_constrain() {
        let message = "<|start|>assistant<|channel|>commentary to=container.exec<|message|>{\"cmd\":[\"ls\", \"-la\"]}<|call|>";
        let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
        assert_eq!(name, "run_terminal_cmd");
        assert_eq!(args["command"], serde_json::json!(["ls", "-la"]));
    }

    #[test]
    fn test_parse_harmony_channel_tool_call_with_string_cmd() {
        let message =
            "<|start|>assistant<|channel|>commentary to=bash<|message|>{\"cmd\":\"pwd\"}<|call|>";
        let (name, args) = detect_textual_tool_call(message).expect("should parse harmony format");
        assert_eq!(name, "bash");
        assert_eq!(args["command"], serde_json::json!(["pwd"]));
    }
}
