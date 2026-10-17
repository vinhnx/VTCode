//! Extraction of balanced JSON blocks embedded in free-form text.
//!
//! LLM responses and tool output frequently wrap a JSON payload in prose or
//! code fences. [`first_json_block`] returns the first balanced `{...}` or
//! `[...]` block, honoring string literals and rejecting mismatched nesting,
//! so callers can hand the slice to `serde_json`.

/// Return the first balanced JSON object or array embedded in `text`.
///
/// String-aware: braces and brackets inside JSON string literals are ignored.
/// Nesting is validated with a stack, so mismatched delimiters such as
/// `{...]` yield `None` instead of a truncated slice.
#[must_use]
pub fn first_json_block(text: &str) -> Option<&str> {
    let (start, opening) = text.char_indices().find(|(_, ch)| matches!(ch, '{' | '['))?;
    let mut stack = vec![opening];
    let mut in_string = false;
    let mut escaped = false;

    for (offset, ch) in text.get(start + opening.len_utf8()..)?.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' => {
                if stack.pop() != Some('{') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return text.get(start..end);
                }
            }
            ']' => {
                if stack.pop() != Some('[') {
                    return None;
                }
                if stack.is_empty() {
                    let end = start + opening.len_utf8() + offset + ch.len_utf8();
                    return text.get(start..end);
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::first_json_block;

    #[test]
    fn finds_object_after_prose_and_fences() {
        let text = "Here you go:\n```json\n{\"a\": 1}\n```";
        assert_eq!(first_json_block(text), Some("{\"a\": 1}"));
    }

    #[test]
    fn finds_array_block() {
        assert_eq!(first_json_block("prefix [1, 2, 3] suffix"), Some("[1, 2, 3]"));
    }

    #[test]
    fn ignores_braces_inside_strings() {
        assert_eq!(first_json_block(r#"{"a": "not } here"}"#), Some(r#"{"a": "not } here"}"#));
    }

    #[test]
    fn handles_mixed_nesting() {
        assert_eq!(first_json_block(r#"x {"a": [1, {"b": 2}]} y"#), Some(r#"{"a": [1, {"b": 2}]}"#));
    }

    #[test]
    fn rejects_mismatched_delimiters() {
        assert_eq!(first_json_block("{\"a\": 1]"), None);
        assert_eq!(first_json_block("[1, 2}"), None);
    }

    #[test]
    fn returns_none_for_unterminated_or_missing_json() {
        assert_eq!(first_json_block("no json here"), None);
        assert_eq!(first_json_block("{\"a\": 1"), None);
        assert_eq!(first_json_block(""), None);
    }
}
