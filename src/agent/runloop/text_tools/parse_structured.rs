use serde_json::{Map, Value};
use vtcode_core::config::constants::tools;

use crate::agent::runloop::text_tools::canonical::canonicalize_tool_name;
use crate::agent::runloop::text_tools::parse_args::{
    normalize_command_string, parse_scalar_value, split_function_arguments, split_top_level_entries,
};

pub(super) fn parse_rust_struct_tool_call(text: &str) -> Option<(String, Value)> {
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

    // Check if the name contains an assignment like "run_pty_cmd args=" or "run_pty_cmd args ="
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
    canonical.as_ref()?;

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

    // Validate required parameters based on tool type
    match canonical.as_deref() {
        Some(tools::RUN_PTY_CMD) => {
            if !positional.is_empty() && !object.contains_key("command") {
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
                            object.insert("command".to_string(), Value::String(command.clone()));
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
            // Validate that command is present and not empty
            if !object.contains_key("command") {
                return None;
            }
        }
        Some(tools::GREP_FILE) => {
            // For grep_file, ensure pattern is present
            if !positional.is_empty()
                && !object.contains_key("pattern")
                && let Value::String(pattern) = &positional[0]
            {
                object.insert("pattern".to_string(), Value::String(pattern.clone()));
            }
            // Validate that pattern is required
            if !object.contains_key("pattern") {
                return None;
            }
        }
        Some(tools::READ_FILE | tools::WRITE_FILE | tools::EDIT_FILE) => {
            // These tools require a 'path' parameter
            if !positional.is_empty()
                && !object.contains_key("path")
                && let Value::String(path) = &positional[0]
            {
                object.insert("path".to_string(), Value::String(path.clone()));
            }
            if !object.contains_key("path") {
                return None;
            }
        }
        _ => {
            // For other tools, only reject if there are positional args but no handler
            if !positional.is_empty() {
                return None;
            }
        }
    }

    Some((name.to_string(), Value::Object(object)))
}
