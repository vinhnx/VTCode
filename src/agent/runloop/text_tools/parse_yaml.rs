use serde_json::{Map, Value};

use crate::agent::runloop::text_tools::parse_args::parse_scalar_value;

pub(super) fn parse_yaml_tool_call(text: &str) -> Option<(String, Value)> {
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
        "swift",
        "go",
        "java",
        "cpp",
        "c",
        "php",
        "html",
        "css",
        "sql",
        "csharp",
    ];

    for line in lines.by_ref() {
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

    for line in lines {
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
