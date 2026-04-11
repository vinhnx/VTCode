use crate::config::constants::models;
use crate::config::constants::tools;
use serde_json::Value;

pub(super) fn normalized_harmony_model(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_provider = trimmed.rsplit('/').next().unwrap_or(trimmed);
    let without_annotation = without_provider
        .split('@')
        .next()
        .unwrap_or(without_provider);
    let without_variant = without_annotation
        .split(':')
        .next()
        .unwrap_or(without_annotation);

    without_variant.to_ascii_lowercase()
}

pub(super) fn uses_harmony(model: &str) -> bool {
    let normalized = normalized_harmony_model(model);
    if normalized.is_empty() {
        return false;
    }

    models::openai::HARMONY_MODELS
        .iter()
        .any(|candidate| *candidate == normalized)
}

pub(super) fn parse_harmony_tool_name(recipient: &str) -> String {
    // Handle harmony format namespace mappings (e.g., "repo_browser.list_files" -> "list_files")
    // Direct tool name aliases are handled by canonical_tool_name() in the registry
    match recipient {
        "repo_browser.list_files" => tools::LIST_FILES.to_string(),
        "repo_browser.read_file" => tools::READ_FILE.to_string(),
        "repo_browser.write_file" => tools::WRITE_FILE.to_string(),
        "container.exec" | "bash" => tools::UNIFIED_EXEC.to_string(),
        "grep" => tools::GREP_FILE.to_string(),
        _ => {
            // Try to extract the function name after the last dot
            if let Some(dot_pos) = recipient.rfind('.') {
                recipient[dot_pos + 1..].to_string()
            } else {
                recipient.to_string()
            }
        }
    }
}

pub(super) fn parse_harmony_tool_call_from_text(text: &str) -> Option<(String, Value)> {
    let mut found_segment = false;
    for segment in text.split("<|start|>") {
        if segment.trim().is_empty() {
            continue;
        }
        found_segment = true;
        if let Some(parsed) = parse_harmony_tool_call_segment(segment) {
            return Some(parsed);
        }
    }

    if !found_segment {
        return parse_harmony_tool_call_segment(text);
    }

    None
}

pub(super) fn normalize_harmony_tool_arguments(raw: &str) -> Option<String> {
    let arguments = parse_harmony_arguments(raw)?;
    serde_json::to_string(&arguments).ok()
}

fn parse_harmony_tool_call_segment(text: &str) -> Option<(String, Value)> {
    let to_pos = text.find("to=")?;
    let after_to = &text[to_pos + 3..];
    let tool_ref = after_to
        .split(|c: char| c.is_whitespace() || c == '<')
        .next()
        .unwrap_or("");
    let tool_name = parse_harmony_tool_name(tool_ref);
    if tool_name.is_empty() {
        return None;
    }

    let content = if let Some(message_pos) = text.find("<|message|>") {
        let after_message = &text[message_pos + "<|message|>".len()..];
        let stop_idx = after_message
            .find("<|call|>")
            .or_else(|| after_message.find("<|end|>"))
            .or_else(|| after_message.find("<|return|>"))
            .unwrap_or(after_message.len());
        after_message[..stop_idx].trim()
    } else {
        after_to[tool_ref.len()..].trim()
    };

    let args = parse_harmony_arguments(content)?;
    Some((tool_name, args))
}

fn parse_harmony_arguments(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(Value::Object(serde_json::Map::new()));
    }

    serde_json::from_str(trimmed).ok().or_else(|| {
        if trimmed.contains('\'') {
            serde_json::from_str::<Value>(&trimmed.replace('\'', "\"")).ok()
        } else {
            None
        }
    })
}
