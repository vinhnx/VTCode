use serde_json::Value;

use crate::agent::runloop::text_tools::parse_args::{
    find_matching_delimiter, parse_textual_arguments,
};
use crate::agent::runloop::text_tools::parse_channel::parse_tool_name_from_reference;
use crate::agent::runloop::text_tools::parser::{ParsedToolCall, TextualToolParser};

const MAX_BRACKETED_NESTING_DEPTH: usize = 256;

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
        if let Some(idx) =
            find_matching_delimiter(after_name, 0, '{', '}', MAX_BRACKETED_NESTING_DEPTH)
        {
            let json_str = &after_name[..idx + 1];
            if let Ok(args) = serde_json::from_str::<Value>(json_str) {
                return Some((tool_name, args));
            }
        }
    } else if after_name.starts_with('(') {
        // Try to parse as function arguments
        if let Some(idx) =
            find_matching_delimiter(after_name, 0, '(', ')', MAX_BRACKETED_NESTING_DEPTH)
        {
            let args_str = &after_name[1..idx];
            if let Some(args) = parse_textual_arguments(args_str) {
                return Some((tool_name, args));
            }
        }
    }

    None
}

/// Parser for bracketed tool call format.
pub(crate) struct BracketedToolParser;

impl TextualToolParser for BracketedToolParser {
    fn name(&self) -> &'static str {
        "bracketed"
    }

    fn try_parse(&self, text: &str) -> Option<ParsedToolCall> {
        let result = parse_bracketed_tool_call(text);
        if result.is_none() {
            tracing::debug!(
                parser = "bracketed",
                reason = "no matching [tool: ...] pattern",
                "Rejected textual tool call"
            );
        }
        result.map(|(name, args)| ParsedToolCall { name, args })
    }
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
