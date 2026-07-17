//! # File-based Workflow Utilities
//!
//! This module provides utilities for handling large inputs by reading content from files
//! referenced in user input, supporting the Claude Code pattern of using files for large inputs.

use anyhow::Result;
use std::path::Path;
use tokio::fs;

/// Include content from text files referenced in user input
///
/// This function looks for patterns like `@./path/to/file.txt` and replaces them with
/// the actual file content, allowing users to reference large files without pasting content.
///
/// # Arguments
///
/// * `input` - The user input text that may contain @ patterns referencing files
/// * `base_dir` - The base directory to resolve relative paths from
///
/// # Returns
///
/// * `String` - The input with file references replaced by their content
pub async fn include_file_content(input: &str, base_dir: &Path) -> Result<String> {
    let matches = vtcode_commons::at_pattern::find_at_patterns(input);
    if matches.is_empty() {
        return Ok(input.to_string());
    }

    let mut result = String::new();
    let mut last_end = 0;

    for m in matches {
        // Add the text before this match
        if m.start > last_end {
            result.push_str(&input[last_end..m.start]);
        }

        // Security validation: prevent directory traversal and validate path
        if !vtcode_commons::paths::is_safe_relative_path(m.path) {
            // Skip invalid paths (likely directory traversal attempts)
            result.push_str(m.full_match);
            last_end = m.end;
            continue;
        }

        // Try to read the file as text content
        let file_path = base_dir.join(m.path.trim());

        match fs::read_to_string(&file_path).await {
            Ok(file_content) => {
                // Include the file content
                result.push_str(&file_content);
            }
            Err(_) => {
                // If it's not a valid text file, treat as literal @ usage
                result.push_str(m.full_match);
            }
        }

        last_end = m.end;
    }

    // Add any remaining text after the last match
    if last_end < input.len() {
        result.push_str(&input[last_end..]);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_include_file_content_with_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");

        // Create a test file with content
        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&file_path).unwrap());
        writeln!(temp_file, "This is test file content").unwrap();
        temp_file.flush().unwrap();

        let input = format!(
            "Look at this file: @{}",
            file_path.file_name().unwrap().to_string_lossy()
        );

        let result = include_file_content(&input, temp_dir.path()).await.unwrap();

        // Should contain the file content instead of the @ reference
        assert!(result.contains("This is test file content"));
        assert!(!result.contains('@')); // The @ symbol should be replaced
    }

    #[tokio::test]
    async fn test_include_file_content_regular_text() {
        let temp_dir = TempDir::new().unwrap();
        let input = "This is just regular text with @ symbol not followed by file";

        let result = include_file_content(input, temp_dir.path()).await.unwrap();

        // Should return original text since there's no valid file
        assert_eq!(result, input);
    }

    #[tokio::test]
    async fn test_include_file_content_invalid_file() {
        let temp_dir = TempDir::new().unwrap();
        let input = "Look at @nonexistent.txt which doesn't exist";

        let result = include_file_content(input, temp_dir.path()).await.unwrap();

        // Should return original text since file doesn't exist
        assert_eq!(result, input);
    }

    #[tokio::test]
    async fn test_include_file_content_with_quoted_path() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("file with spaces.txt");

        // Create a test file with content
        let mut temp_file = std::io::BufWriter::new(std::fs::File::create(&file_path).unwrap());
        writeln!(temp_file, "Content with spaces in filename").unwrap();
        temp_file.flush().unwrap();

        let input = format!(
            "Look at this file: @\"{}\"",
            file_path.file_name().unwrap().to_string_lossy()
        );

        let result = include_file_content(&input, temp_dir.path()).await.unwrap();

        // Should contain the file content
        assert!(result.contains("Content with spaces in filename"));
    }

    #[test]
    fn test_is_safe_relative_path() {
        use vtcode_commons::paths::is_safe_relative_path;
        assert!(!is_safe_relative_path("../../etc/passwd"));
        assert!(!is_safe_relative_path("../file.txt"));
        assert!(is_safe_relative_path("file.txt"));
        assert!(is_safe_relative_path("./path/file.txt"));
        assert!(is_safe_relative_path(" path with spaces .txt "));
    }
}
