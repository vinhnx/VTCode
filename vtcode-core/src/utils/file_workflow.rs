//! # File-based Workflow Utilities
//!
//! This module provides utilities for handling large inputs by reading content from files
//! referenced in user input, supporting the Claude Code pattern of using files for large inputs.

use anyhow::{Context, Result};
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
    // Use regex to match @ followed by a potential file path
    // This pattern handles both quoted paths (with spaces) and unquoted paths
    let re = regex::Regex::new(r#"@(?:"([^"]+)"|'([^']+)'|([^\s"'\[\](){}<>|\\^`]+))"#)
        .context("Failed to compile regex")?;

    let mut result = String::new();
    let mut last_end = 0;

    for cap in re.captures_iter(input) {
        let full_match = &cap[0];

        // Extract the path from the appropriate capture group (quoted or unquoted)
        let path_part = if let Some(m) = cap.get(1) {
            // First group: quoted with double quotes
            m.as_str()
        } else if let Some(m) = cap.get(2) {
            // Second group: quoted with single quotes
            m.as_str()
        } else if let Some(m) = cap.get(3) {
            // Third group: unquoted
            m.as_str()
        } else {
            continue; // Should not happen if regex is correct
        };

        // Extract the start and end positions to separate text from matches
        let start = cap.get(0).unwrap().start();
        let end = cap.get(0).unwrap().end();

        // Add the text before this match
        if start > last_end {
            result.push_str(&input[last_end..start]);
        }

        // Security validation: prevent directory traversal and validate path
        let normalized_path = normalize_path(path_part);
        if normalized_path.is_empty() {
            // Skip invalid paths (likely directory traversal attempts)
            result.push_str(full_match);
            last_end = end;
            continue;
        }

        // Try to read the file as text content
        let file_path = base_dir.join(&normalized_path);

        match fs::read_to_string(&file_path).await {
            Ok(file_content) => {
                // Include the file content
                result.push_str(&file_content);
            }
            Err(_) => {
                // If it's not a valid text file, treat as literal @ usage
                result.push_str(full_match);
            }
        }

        last_end = end;
    }

    // Add any remaining text after the last match
    if last_end < input.len() {
        result.push_str(&input[last_end..]);
    }

    Ok(result)
}

/// Normalizes a path string to prevent directory traversal and validate it's safe
fn normalize_path(path: &str) -> String {
    // Remove leading/trailing whitespace
    let path = path.trim();

    // Check for path traversal attempts
    if path.contains("../")
        || path.contains("..\\")
        || path.starts_with("../")
        || path.starts_with("..\\")
    {
        return String::new(); // Indicates invalid path
    }

    // Block absolute paths for security - all file references must be relative to workspace
    if path.starts_with('/') || (cfg!(windows) && path.contains(':')) {
        return String::new(); // Indicates invalid path
    }

    path.to_string()
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
        let mut temp_file = std::fs::File::create(&file_path).unwrap();
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
        let mut temp_file = std::fs::File::create(&file_path).unwrap();
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
    fn test_normalize_path_security() {
        assert_eq!(normalize_path("../../etc/passwd"), "");
        assert_eq!(normalize_path("../file.txt"), "");
        assert_eq!(normalize_path("file.txt"), "file.txt");
        assert_eq!(normalize_path("./path/file.txt"), "./path/file.txt");
        assert_eq!(
            normalize_path(" path with spaces .txt "),
            "path with spaces .txt"
        );
    }
}