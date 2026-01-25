use serde_json::Value;

use crate::agent::runloop::text_tools::parse_args::parse_textual_arguments;
use crate::agent::runloop::text_tools::parse_channel::parse_tool_name_from_reference;

pub(super) fn parse_bracketed_tool_call(text: &str) -> Option<(String, Value)> {
    let start_tag = "[tool: ";
    let start_idx = text.find(start_tag)?;
    let rest = &text[start_idx + start_tag.len()..];

    let end_bracket_idx = rest.find(']')?;
    let header = rest[..end_bracket_idx].trim();

    // Extract tool name from header (supporting to= and functions. prefix)
    let tool_name = if let Some(to_pos) = header.find("to=") {
        let after_to = &header[to_pos + 3..];
        let tool_ref = after_to
            .split(|c: char| c.is_whitespace() || c == ':' || c == '<')
            .next()
            .unwrap_or("");
        parse_tool_name_from_reference(tool_ref).to_string()
    } else {
        parse_tool_name_from_reference(header).to_string()
    };

    let after_name = &rest[end_bracket_idx + 1..].trim_start();

    if after_name.starts_with('{') {
        // Try to parse as JSON
        let mut depth = 0;
        let mut end_idx = None;
        for (idx, ch) in after_name.char_indices() {
            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(idx);
                    break;
                }
            }
        }
        if let Some(idx) = end_idx {
            let json_str = &after_name[..idx + 1];
            if let Ok(args) = serde_json::from_str::<Value>(json_str) {
                return Some((tool_name, args));
            }
        }
    } else if after_name.starts_with('(') {
        // Try to parse as function arguments
        let mut depth = 0;
        let mut end_idx = None;
        for (idx, ch) in after_name.char_indices() {
            if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(idx);
                    break;
                }
            }
        }
        if let Some(idx) = end_idx {
            let args_str = &after_name[1..idx];
            if let Some(args) = parse_textual_arguments(args_str) {
                return Some((tool_name, args));
            }
        }
    }

    None
}
