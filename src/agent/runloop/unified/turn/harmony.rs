/// All known harmony control-token markers. Used by [`contains_harmony_marker`]
/// to detect harmony-formatted content and by `stream_sanitization` for
/// incomplete-block detection.
pub(crate) const HARMONY_MARKERS: &[&str] = &[
    "<|start|>",
    "<|channel|>",
    "<|message|>",
    "<|call|>",
    "<|return|>",
    "<|end|>",
];

/// Harmony end-of-block tags, used for matching the end of a `<|start|>` block.
/// This is the single source of truth — `stream_sanitization` imports this
/// instead of maintaining a duplicate list.
pub(crate) const HARMONY_END_TAGS: &[&str] = &["<|end|>", "<|call|>", "<|return|>"];

/// Returns `true` if `text` contains any harmony control-token marker.
///
/// This is the single source of truth for harmony detection — both the stream
/// renderer (`stream_sanitization`) and the non-streamed response processor
/// (`response_processing`) use this function instead of inline checks.
#[inline]
pub(crate) fn contains_harmony_marker(text: &str) -> bool {
    text.contains("<|") || HARMONY_MARKERS.iter().any(|marker| text.contains(marker))
}

pub(crate) fn strip_harmony_syntax(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut current = text;

    while let Some(start_pos) = current.find("<|start|>") {
        // Add text before <|start|>
        result.push_str(&current[..start_pos]);

        let rest = &current[start_pos + "<|start|>".len()..];
        if let Some(msg_pos) = rest.find("<|message|>") {
            let after_msg = &rest[msg_pos + "<|message|>".len()..];

            // Find the end of this message
            let mut earliest_end = None;
            for tag in HARMONY_END_TAGS {
                if let Some(pos) = after_msg.find(tag)
                    && earliest_end.is_none_or(|(p, _)| pos < p)
                {
                    earliest_end = Some((pos, tag));
                }
            }

            if let Some((end_pos, tag)) = earliest_end {
                // Check if this is a "final" channel message. If so, keep the content.
                // Otherwise (analysis, commentary), skip it.
                let header = &rest[..msg_pos];
                if header.contains("final") {
                    result.push_str(&after_msg[..end_pos]);
                }

                current = &after_msg[end_pos + tag.len()..];
            } else {
                // No end tag found, just skip the rest of the header and keep the rest of the content
                result.push_str(after_msg);
                current = "";
            }
        } else {
            // No <|message|> found, skip <|start|>
            current = rest;
        }
    }

    result.push_str(current);

    // Optimization: Single-pass cleanup of remaining tags using in-place filtering
    // This avoids multiple String allocations from repeated .replace() calls
    let mut final_result = String::with_capacity(result.len());
    let mut chars = result.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' && chars.peek() == Some(&'|') {
            // Potential tag start - scan ahead to find closing |>
            let mut tag_buf = String::with_capacity(16);
            tag_buf.push(c);
            let mut found_end = false;
            for next_c in chars.by_ref() {
                tag_buf.push(next_c);
                if next_c == '>' && tag_buf.ends_with("|>") {
                    found_end = true;
                    break;
                }
                // Limit tag length to avoid unbounded scanning
                if tag_buf.len() > 20 {
                    break;
                }
            }
            // If not a valid tag pattern, include the characters
            if !found_end {
                final_result.push_str(&tag_buf);
            }
            // Otherwise, skip the tag (don't add to final_result)
        } else {
            final_result.push(c);
        }
    }

    // Trim in-place by finding start/end bounds
    let trimmed = final_result.trim();
    if trimmed.len() == final_result.len() {
        final_result
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{contains_harmony_marker, strip_harmony_syntax};

    #[test]
    fn test_strip_harmony_syntax_basic() {
        let input = r#"<|start|>assistant<|channel|>commentary to=grep_file <|constrain|>json<|message|>{"path":"", "pattern":"TODO"}<|call|>"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_harmony_syntax_with_text() {
        let input = r#"Here is some text <|start|>assistant<|channel|>commentary to=grep_file <|constrain|>json<|message|>{"path":"", "pattern":"TODO"}<|call|> and more text"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "Here is some text  and more text");
    }

    #[test]
    fn test_strip_harmony_syntax_multiple() {
        let input = r#"<|start|>assistant<|channel|>commentary to=tool1<|message|>{}<|call|> text <|start|>assistant<|channel|>commentary to=tool2<|message|>{}<|call|>"#;
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "text");
    }

    #[test]
    fn test_strip_harmony_syntax_no_harmony() {
        let input = "This is normal text without harmony syntax";
        let result = strip_harmony_syntax(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_strip_harmony_syntax_partial() {
        let input = "Text with <|channel|> partial tags <|message|>";
        let result = strip_harmony_syntax(input);
        assert_eq!(result, "Text with  partial tags");
    }

    #[test]
    fn contains_harmony_marker_detects_all_markers() {
        assert!(contains_harmony_marker("<|start|>content"));
        assert!(contains_harmony_marker("<|channel|>content"));
        assert!(contains_harmony_marker("<|message|>content"));
        assert!(contains_harmony_marker("<|call|>"));
        assert!(contains_harmony_marker("<|return|>"));
        assert!(contains_harmony_marker("<|end|>"));
    }

    #[test]
    fn contains_harmony_marker_detects_generic_pipe() {
        // Any `<|` sequence is a potential harmony marker
        assert!(contains_harmony_marker("text <|unknown|> more"));
    }

    #[test]
    fn contains_harmony_marker_returns_false_for_clean_text() {
        assert!(!contains_harmony_marker("Just normal text"));
        assert!(!contains_harmony_marker(""));
        assert!(!contains_harmony_marker("Some < but not harmony > text"));
    }
}
