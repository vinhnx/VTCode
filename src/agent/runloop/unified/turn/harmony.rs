pub(crate) fn strip_harmony_syntax(text: &str) -> String {
    // Remove harmony tool call patterns
    let mut result = text.to_string();
    // Pattern: <|start|>assistant<|channel|>commentary to=... <|constrain|>...<|message|>...<|call|>
    // We want to remove everything from <|start|> to <|call|> inclusive
    while let Some(start_pos) = result.find("<|start|>") {
        if let Some(call_pos) = result[start_pos..].find("<|call|>") {
            let end_pos = start_pos + call_pos + "<|call|>".len();
            result.replace_range(start_pos..end_pos, "");
        } else {
            // If no matching <|call|>, just remove <|start|>
            result.replace_range(start_pos..start_pos + "<|start|>".len(), "");
        }
    }

    // Clean up any remaining harmony tags
    result = result.replace("<|channel|>", "");
    result = result.replace("<|constrain|>", "");
    result = result.replace("<|message|>", "");
    result = result.replace("<|call|>", "");

    // Clean up extra whitespace
    result.trim().to_string()
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
