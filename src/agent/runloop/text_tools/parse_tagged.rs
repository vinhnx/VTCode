use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::parse_args::{
    find_matching_delimiter, normalize_command_string, parse_key_value_arguments,
    parse_scalar_value, split_indexed_key,
};
use crate::agent::runloop::text_tools::parser::{ParseResult, ParsedToolCall, TextualToolParser};

const MAX_TAGGED_NESTING_DEPTH: usize = 256;

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
                // Use shared delimiter matcher to find matching closing brace
                if let Some(json_end) =
                    find_matching_delimiter(json_content, 0, '{', '}', MAX_TAGGED_NESTING_DEPTH)
                {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&json_content[..json_end + 1])
                        && let Some(obj) = parsed.as_object()
                    {
                        for (k, v) in obj {
                            object.insert(k.clone(), v.clone());
                        }
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

    Some((name.to_string(), Value::Object(object)))
}

fn parse_minimax_tool_call(text: &str) -> Option<(String, Value)> {
    const INVOKE_TAG: &str = "<invoke name=\"";
    const INVOKE_CLOSE: &str = "</invoke>";
    const PARAMETER_TAG: &str = "<parameter name=\"";
    const PARAMETER_CLOSE: &str = "</parameter>";

    // Strip the ]<]minimax[>[ noise prefix if present
    let cleaned_text = text.replace("]<]minimax[>[", "");
    let working_text = cleaned_text.as_str();

    let invoke_start = working_text.find(INVOKE_TAG)?;
    let invoke_rest = &working_text[invoke_start + INVOKE_TAG.len()..];
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

    // First, try parsing <parameter name="..."> tags (old format)
    let mut found_parameter_tags = false;
    let original_rest = rest;
    while let Some(parameter_start) = rest.find(PARAMETER_TAG) {
        found_parameter_tags = true;
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

    // If no <parameter name="..."> tags were found, try parsing child elements (new format)
    if !found_parameter_tags {
        rest = original_rest.trim();
        // Parse direct child elements as parameters
        while !rest.is_empty() {
            // Skip whitespace
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }

            // Check if this starts with '<'
            if !rest.starts_with('<') {
                break;
            }

            // Check if this is a closing tag - if so, we're done
            if rest.starts_with("</") {
                break;
            }

            // Extract the tag name
            rest = &rest[1..]; // Skip the '<'
            let tag_name_end = rest.find(['>', ' ', '\t', '\n']).unwrap_or(rest.len());
            let tag_name = &rest[..tag_name_end].trim();

            // Skip attributes if any and find the '>'
            let content_start = match rest.find('>') {
                Some(pos) => pos,
                None => break,
            };
            rest = &rest[content_start + 1..];

            // Find the matching closing tag
            let close_tag = format!("</{tag_name}>");
            let content_end = match rest.find(&close_tag) {
                Some(pos) => pos,
                None => break,
            };
            let content = rest[..content_end].trim();

            // Parse the value - special handling for "command" to preserve quotes
            let value = if *tag_name == "command" {
                // For command, normalize it to an array but don't strip quotes via parse_scalar_value
                if let Some(array) = normalize_command_string(content) {
                    Value::Array(array)
                } else {
                    Value::String(content.to_string())
                }
            } else {
                parse_scalar_value(content)
            };

            // Add to object or indexed_values
            if let Some((base, index)) = split_indexed_key(tag_name) {
                indexed_values
                    .entry(base.to_string())
                    .or_default()
                    .insert(index, value);
            } else {
                object.insert(tag_name.to_string(), value);
            }

            // Move past the closing tag
            rest = &rest[content_end + close_tag.len()..];
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

    Some((name, Value::Object(object)))
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

/// Collects XML-style tagged tool-call regions for stripping.
pub(super) fn collect_tagged_regions(text: &str, regions: &mut Vec<(usize, usize)>) {
    collect_enclosed_regions(text, "<tool_call>", "</tool_call>", regions);
    collect_enclosed_regions(text, "<invoke name=\"", "</invoke>", regions);
}

fn collect_enclosed_regions(
    text: &str,
    open_marker: &str,
    close_marker: &str,
    regions: &mut Vec<(usize, usize)>,
) {
    let mut search_start = 0usize;
    while let Some(relative_start) = text[search_start..].find(open_marker) {
        let start = search_start + relative_start;
        let content_start = start + open_marker.len();
        let end = text[content_start..]
            .find(close_marker)
            .map(|idx| content_start + idx + close_marker.len())
            .unwrap_or(text.len());
        if start < end && end <= text.len() {
            regions.push((start, end));
        }
        search_start = end.max(content_start);
    }
}

/// Parser for XML-style tagged tool calls.
pub(crate) struct TaggedToolParser;

impl TextualToolParser for TaggedToolParser {
    fn name(&self) -> &'static str {
        "tagged"
    }

    fn try_parse(&self, text: &str) -> ParseResult {
        match parse_tagged_tool_call(text) {
            Some((name, args)) => ParseResult::Success(ParsedToolCall { name, args }),
            None => {
                tracing::debug!(
                    parser = "tagged",
                    reason = "no matching <tool_call> or <invoke> pattern",
                    "Rejected textual tool call"
                );
                ParseResult::Reject("no matching <tool_call> or <invoke> pattern")
            }
        }
    }

    fn find_consumed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut regions = Vec::new();
        collect_tagged_regions(text, &mut regions);
        regions
    }
}
