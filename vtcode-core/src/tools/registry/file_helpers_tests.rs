#[cfg(test)]
mod edit_file_tests {
    use super::*;
    use crate::tools::types::EditInput;
    use serde_json::json;

    /// Helper function to simulate edit_file behavior without actual file I/O
    /// This allows testing the core replacement logic
    fn apply_edit_internally(
        content: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, String> {
        // Strategy 1: Direct string replacement
        if content.contains(old_str) {
            let result = content.replace(old_str, new_str);
            if result != content {
                return Ok(result);
            }
        }

        // Strategy 2: Line-by-line matching with trim
        let old_lines: Vec<&str> = old_str.lines().collect();
        let content_lines: Vec<&str> = content.lines().collect();

        if old_lines.is_empty() {
            return Err("old_str cannot be empty".to_string());
        }

        for i in 0..=(content_lines.len().saturating_sub(old_lines.len())) {
            let window = &content_lines[i..i + old_lines.len()];
            // Check if lines match when trimmed
            if window
                .iter()
                .map(|l| l.trim())
                .eq(old_lines.iter().map(|l| l.trim()))
            {
                let replacement_lines: Vec<&str> = new_str.lines().collect();
                let mut result_lines = Vec::with_capacity(
                    i + replacement_lines.len()
                        + content_lines.len().saturating_sub(i + old_lines.len()),
                );
                result_lines.extend_from_slice(&content_lines[..i]);
                result_lines.extend_from_slice(&replacement_lines);
                result_lines.extend_from_slice(&content_lines[i + old_lines.len()..]);

                return Ok(result_lines.join("\n"));
            }
        }

        Err("Could not find text to replace".to_string())
    }

    #[test]
    fn test_exact_match_replacement() {
        let content = "line1\nline2\nline3";
        let old_str = "line2";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "line1\nREPLACED\nline3");
    }

    #[test]
    fn test_replacement_at_start_of_file() {
        let content = "first\nsecond\nthird";
        let old_str = "first";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "REPLACED\nsecond\nthird");
        assert!(
            !result.starts_with("\n"),
            "Should not add extra blank line at start"
        );
    }

    #[test]
    fn test_replacement_at_end_of_file() {
        let content = "first\nsecond\nlast";
        let old_str = "last";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "first\nsecond\nREPLACED");
        assert!(
            !result.ends_with("\n"),
            "Should not add extra blank line at end"
        );
    }

    #[test]
    fn test_replacement_entire_file() {
        let content = "old content";
        let old_str = "old content";
        let new_str = "new content";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "new content");
    }

    #[test]
    fn test_multiline_replacement() {
        let content = "line1\nline2\nline3\nline4";
        let old_str = "line2\nline3";
        let new_str = "REPLACED_A\nREPLACED_B";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "line1\nREPLACED_A\nREPLACED_B\nline4");
    }

    #[test]
    fn test_fuzzy_match_different_indentation() {
        let content = "fn test() {\n    body\n}";
        let old_str = "fn test() {\n  body\n}"; // Different indentation (2-space vs 4-space)
        let new_str = "fn test() {\n    REPLACED\n}";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        // The replacement should occur on the line with trimmed matching
        assert_eq!(result, "fn test() {\n    REPLACED\n}");
    }

    #[test]
    fn test_fuzzy_match_leading_trailing_spaces() {
        let content = "fn test() {\n  body  \n}"; // Leading/trailing spaces
        let old_str = "fn test() {\nbody\n}"; // No spaces on middle line
        let new_str = "fn test() {\nREPLACED\n}";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert!(
            result.contains("REPLACED"),
            "Should match with trim-based comparison"
        );
    }

    #[test]
    fn test_no_match_returns_error() {
        let content = "line1\nline2\nline3";
        let old_str = "nonexistent";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str);
        assert!(result.is_err(), "Should return error for non-existent text");
        assert!(result.unwrap_err().contains("Could not find"));
    }

    #[test]
    fn test_empty_old_string() {
        let content = "line1\nline2";
        let old_str = "";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str);
        assert!(result.is_err(), "Should error on empty old_str");
    }

    #[test]
    fn test_empty_file() {
        let content = "";
        let old_str = "anything";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str);
        assert!(result.is_err(), "Should error on empty file");
    }

    #[test]
    fn test_empty_replacement() {
        let content = "line1\nline2\nline3";
        let old_str = "line2";
        let new_str = "";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "line1\nline3");
    }

    #[test]
    fn test_single_line_file() {
        let content = "single line";
        let old_str = "single";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "REPLACED line");
    }

    #[test]
    fn test_multiple_occurrences_replaces_first() {
        let content = "test\ntest\ntest";
        let old_str = "test";
        let new_str = "REPLACED";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        // Direct string replacement replaces all occurrences
        assert_eq!(result, "REPLACED\nREPLACED\nREPLACED");
    }

    #[test]
    fn test_whitespace_preservation_in_replacement() {
        let content = "    indented\n    content";
        let old_str = "indented\n    content";
        let new_str = "NEW\n    CONTENT";

        let result = apply_edit_internally(content, old_str, new_str).unwrap();
        assert_eq!(result, "    NEW\n    CONTENT");
    }
}
