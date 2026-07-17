use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::canonical::canonicalize_tool_name;
use crate::agent::runloop::text_tools::parse_args::{
    normalize_command_string, parse_scalar_value, split_function_arguments, split_top_level_entries,
};
use crate::agent::runloop::text_tools::parser::{ParseResult, ParsedToolCall, TextualToolParser};
use crate::agent::runloop::text_tools::validate::normalize_and_validate_tool_args;

/// A code-fence span together with the parsed tool call it contains.
pub(super) type RustStructToolCallSpan = ((usize, usize), (String, Value, Vec<Value>));

pub(super) fn parse_rust_struct_tool_call(text: &str) -> Option<(String, Value, Vec<Value>)> {
    find_rust_struct_tool_call_spans(text)
        .into_iter()
        .next()
        .map(|(_span, result)| result)
}

/// Finds all code-fence spans that contain a parseable Rust-struct-style tool
/// call.
pub(super) fn find_rust_struct_tool_call_spans(text: &str) -> Vec<RustStructToolCallSpan> {
    let mut results = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find("```") {
        let fence_start = search_start + relative_start;
        let after_open = &text[fence_start + 3..];
        let Some(newline) = after_open.find('\n') else {
            break;
        };
        let block_start_in_text = fence_start + 3 + newline + 1;
        let after_newline = &after_open[newline + 1..];

        let Some(end_relative) = after_newline.find("```") else {
            break;
        };
        let block = &after_newline[..end_relative];
        let fence_end = block_start_in_text + end_relative + 3;

        if let Some(result) = parse_structured_block(block) {
            results.push(((fence_start, fence_end), result));
        }

        search_start = fence_end;
    }
    results
}

fn parse_structured_block(block: &str) -> Option<(String, Value, Vec<Value>)> {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(result) = parse_function_call_block(trimmed) {
        return validate_structured_result(result);
    }

    // Struct-style blocks (e.g. `run_pty_cmd { command: "ls" }`) are parsed
    // without schema validation to preserve the original behavior where only
    // function-call-shaped blocks had required-parameter checks.
    parse_struct_style_block(trimmed)
}

fn parse_struct_style_block(trimmed: &str) -> Option<(String, Value, Vec<Value>)> {
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
        raw_name.trim().trim_end_matches([':', '=']).trim().to_string()
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

    Some((name, Value::Object(object), Vec::new()))
}

fn validate_structured_result(
    (name, mut args, positional): (String, Value, Vec<Value>),
) -> Option<(String, Value, Vec<Value>)> {
    let canonical = canonicalize_tool_name(&name)?;
    if !normalize_and_validate_tool_args(&canonical, &mut args, positional) {
        return None;
    }
    Some((name, args, Vec::new()))
}

fn parse_function_call_block(block: &str) -> Option<(String, Value, Vec<Value>)> {
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

    // Reject function-call-shaped blocks whose name does not canonicalize to a
    // known tool. This prevents non-tool code like `printf!("hi")` from being
    // accepted as a tool call while still allowing aliases such as `run(...)`.
    canonicalize_tool_name(name)?;

    let mut object = Map::new();
    let mut positional: Vec<Value> = Vec::new();
    for entry in split_function_arguments(args_body) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        if let Some((key_raw, value_raw)) = entry.split_once('=').or_else(|| entry.split_once(':'))
        {
            let key = key_raw.trim().trim_matches('"').trim_matches('\'').to_string();
            let value = parse_scalar_value(value_raw.trim());
            object.insert(key, value);
        } else {
            positional.push(parse_scalar_value(entry));
        }
    }

    Some((name.to_string(), Value::Object(object), positional))
}

/// Parser for Rust struct-style tool calls in code fences.
pub(crate) struct StructuredToolParser;

impl TextualToolParser for StructuredToolParser {
    fn name(&self) -> &'static str {
        "structured"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
        match parse_rust_struct_tool_call(text) {
            Some((name, args, _positional)) => ParseResult::Success(ParsedToolCall { name, args }),
            None => {
                tracing::debug!(
                    parser = "structured",
                    reason = "no matching Rust struct pattern",
                    "Rejected textual tool call"
                );
                ParseResult::Reject("no matching Rust struct pattern")
            }
        }
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        find_rust_struct_tool_call_spans(text)
            .into_iter()
            .map(|(span, _result)| span)
            .collect()
    }
}
