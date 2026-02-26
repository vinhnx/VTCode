use crate::config::constants::models;
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
        "repo_browser.list_files" => "list_files".to_string(),
        "repo_browser.read_file" => "read_file".to_string(),
        "repo_browser.write_file" => "write_file".to_string(),
        "container.exec" => "run_pty_cmd".to_string(),
        "bash" => "bash".to_string(),
        "grep" => "grep_file".to_string(),
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
    // Look for harmony format: to=tool_name followed by JSON
    if let Some(to_pos) = text.find("to=") {
        let after_to = &text[to_pos + 3..];
        if let Some(space_pos) = after_to.find(' ') {
            let tool_ref = &after_to[..space_pos];
            let tool_name = parse_harmony_tool_name(tool_ref);

            // Look for JSON in the remaining text
            let remaining = &after_to[space_pos..];
            if let Some(json_start) = remaining.find('{')
                && let Some(json_end) = remaining.rfind('}')
            {
                let json_text = &remaining[json_start..=json_end];
                if let Ok(args) = serde_json::from_str(json_text) {
                    return Some((tool_name, args));
                }
            }
        }
    }
    None
}
