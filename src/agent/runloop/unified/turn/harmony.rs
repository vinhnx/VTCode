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
            let end_tags = ["<|end|>", "<|call|>", "<|return|>"];
            let mut earliest_end = None;
            for tag in end_tags {
                if let Some(pos) = after_msg.find(tag)
                    && earliest_end.is_none_or(|(p, _)| pos < p) {
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

    // Clean up any remaining tags just in case
    let tags = [
        "<|start|>",
        "<|end|>",
        "<|message|>",
        "<|channel|>",
        "<|constrain|>",
        "<|call|>",
        "<|return|>",
    ];
    let mut final_result = result;
    for tag in tags {
        final_result = final_result.replace(tag, "");
    }

    final_result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::strip_harmony_syntax;

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
}
