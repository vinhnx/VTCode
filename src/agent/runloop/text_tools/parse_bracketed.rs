use serde_json::Value;

use crate::agent::runloop::text_tools::parse_args::parse_textual_arguments;
use crate::agent::runloop::text_tools::parse_channel::parse_tool_name_from_reference;

const MAX_BRACKETED_NESTING_DEPTH: usize = 256;

fn find_matching_delimiter(input: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if let Some(delimiter) = in_string {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == delimiter {
                in_string = None;
            }
            continue;
        }

        if ch == '"' || ch == '\'' {
            in_string = Some(ch);
            continue;
        }

        if ch == open {
            depth += 1;
            if depth > MAX_BRACKETED_NESTING_DEPTH {
                tracing::warn!(
                    nesting_depth = depth,
                    max_nesting_depth = MAX_BRACKETED_NESTING_DEPTH,
                    open = %open,
                    close = %close,
                    "Rejected bracketed tool call due to excessive nesting"
                );
                return None;
            }
            continue;
        }

        if ch == close {
            if depth == 0 {
                tracing::debug!(
                    close = %close,
                    "Rejected bracketed tool call due to unmatched closing delimiter"
                );
                return None;
            }
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }

    None
}

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
        if let Some(idx) = find_matching_delimiter(after_name, '{', '}') {
            let json_str = &after_name[..idx + 1];
            if let Ok(args) = serde_json::from_str::<Value>(json_str) {
                return Some((tool_name, args));
            }
        }
    } else if after_name.starts_with('(') {
        // Try to parse as function arguments
        if let Some(idx) = find_matching_delimiter(after_name, '(', ')') {
            let args_str = &after_name[1..idx];
            if let Some(args) = parse_textual_arguments(args_str) {
                return Some((tool_name, args));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{MAX_BRACKETED_NESTING_DEPTH, parse_bracketed_tool_call};
    use serde_json::Value;

    fn nested_json(depth: usize) -> String {
        let mut payload = String::new();
        for _ in 0..depth {
            payload.push_str("{\"a\":");
        }
        payload.push_str("\"x\"");
        for _ in 0..depth {
            payload.push('}');
        }
        payload
    }

    #[test]
    fn parses_bracketed_json_within_depth_limit() {
        let payload = nested_json(8);
        let message = format!("[tool: read_file] {payload}");
        let parsed = parse_bracketed_tool_call(&message);
        assert!(parsed.is_some());
        let (name, args) = parsed.unwrap();
        assert_eq!(name, "read_file");
        assert!(args.is_object());
    }

    #[test]
    fn rejects_bracketed_json_beyond_depth_limit() {
        let payload = nested_json(MAX_BRACKETED_NESTING_DEPTH + 1);
        let message = format!("[tool: read_file] {payload}");
        assert!(parse_bracketed_tool_call(&message).is_none());
    }

    #[test]
    fn parses_bracketed_json_with_closing_delimiters_inside_strings() {
        let message =
            r#"[tool: read_file] {"path":"docs/notes})].md","note":"escaped quote: \"}\""}"#;
        let parsed = parse_bracketed_tool_call(message).expect("should parse");
        assert_eq!(parsed.0, "read_file");
        assert_eq!(
            parsed.1["path"],
            Value::String("docs/notes})].md".to_string())
        );
    }

    #[test]
    fn parses_bracketed_function_args_with_closing_delimiters_inside_strings() {
        let message = "[tool: read_file] (path='docs/notes})].md')";
        let parsed = parse_bracketed_tool_call(message).expect("should parse");
        assert_eq!(parsed.0, "read_file");
        assert_eq!(
            parsed.1["path"],
            Value::String("docs/notes})].md".to_string())
        );
    }
}
