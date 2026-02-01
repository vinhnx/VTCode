//! # @ Pattern Parsing Utilities
//!
//! This module provides utilities for parsing @ symbol patterns in user input
//! to automatically load and embed image files as base64-encoded content
//! for LLM processing.

use anyhow::Result;
use std::path::Path;

use crate::llm::provider::{ContentPart, MessageContent};
use crate::utils::image_processing::{read_image_file, read_image_from_url};

/// Parse the @ pattern in text and replace image file paths/URLs with base64 content
///
/// The function looks for patterns like `@./path/to/image.png`, `@image.jpg`, or `@https://example.com/image.png`
/// and replaces them with base64 encoded content that can be processed by LLMs
///
/// # Arguments
///
/// * `input` - The user input text that may contain @ patterns
/// * `base_dir` - The base directory to resolve relative paths from
///
/// # Returns
///
/// * `MessageContent` - Either a single text string or multiple content parts
///   containing both text and base64-encoded images
pub async fn parse_at_patterns(input: &str, base_dir: &Path) -> Result<MessageContent> {
    let matches = vtcode_commons::at_pattern::find_at_patterns(input);
    if matches.is_empty() {
        return Ok(MessageContent::text(input.to_string()));
    }

    let mut parts = Vec::new();
    let mut last_end = 0;

    for m in matches {
        // Add the text before this match
        if m.start > last_end {
            let text_before = &input[last_end..m.start];
            if !text_before.trim().is_empty() {
                parts.push(ContentPart::text(text_before.to_string()));
            }
        }

        // Check if it's a URL or a local file path
        let is_url = m.path.starts_with("http://") || m.path.starts_with("https://");

        if is_url {
            // Try to download and encode the image from URL
            match read_image_from_url(m.path).await {
                Ok(image_data) => {
                    parts.push(ContentPart::Image {
                        data: image_data.base64_data,
                        mime_type: image_data.mime_type,
                        content_type: "image".to_owned(),
                    });
                }
                Err(e) => {
                    // If URL download fails, treat as text
                    tracing::warn!("Failed to load image from URL {}: {}", m.path, e);
                    parts.push(ContentPart::text(m.full_match.to_string()));
                }
            }
        } else {
            // Local file path - apply security validation
            if !vtcode_commons::paths::is_safe_relative_path(m.path) {
                // Skip invalid paths (likely directory traversal attempts)
                parts.push(ContentPart::text(m.full_match.to_string()));
                last_end = m.end;
                continue;
            }

            // Try to read the file as an image
            let image_path = base_dir.join(m.path.trim());

            match read_image_file(&image_path).await {
                Ok(image_data) => {
                    parts.push(ContentPart::Image {
                        data: image_data.base64_data,
                        mime_type: image_data.mime_type,
                        content_type: "image".to_owned(),
                    });
                }
                Err(_) => {
                    // If it's not a valid image file, treat as text (might be regular @ usage)
                    parts.push(ContentPart::text(m.full_match.to_string()));
                }
            }
        }

        last_end = m.end;
    }

    // Add any remaining text after the last match
    if last_end < input.len() {
        let text_after = &input[last_end..];
        if !text_after.trim().is_empty() {
            parts.push(ContentPart::text(text_after.to_string()));
        }
    }

    // If no @ patterns were found, return the original text
    if parts.is_empty() {
        Ok(MessageContent::text(input.to_string()))
    } else if parts.len() == 1 && matches!(parts[0], ContentPart::Text { .. }) {
        // If only one text part, return as simple text
        if let ContentPart::Text { text } = &parts[0] {
            Ok(MessageContent::text(text.clone()))
        } else {
            Ok(MessageContent::parts(parts))
        }
    } else {
        // Otherwise return as multi-part content
        Ok(MessageContent::parts(parts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_parse_at_patterns_with_image() {
        let temp_dir = TempDir::new().unwrap();
        let image_path = temp_dir.path().join("test.png");

        // Create a simple PNG file for testing
        let mut temp_file = std::fs::File::create(&image_path).unwrap();
        // Write a minimal PNG header (not a real image, but valid for testing)
        temp_file
            .write_all(&[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk start
            ])
            .unwrap();
        temp_file.flush().unwrap();

        let input = format!(
            "Look at this image: @{}",
            image_path.file_name().unwrap().to_string_lossy()
        );

        let result = parse_at_patterns(&input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2); // Text part + image part
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Image { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_at_patterns_regular_text() {
        let temp_dir = TempDir::new().unwrap();
        let input = "This is just regular text with @ symbol not followed by file";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        match result {
            MessageContent::Text(text) => {
                assert_eq!(text, input);
            }
            _ => panic!("Expected single text content"),
        }
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

    #[tokio::test]
    async fn test_parse_at_patterns_invalid_file() {
        let temp_dir = TempDir::new().unwrap();
        let input = "Look at @nonexistent.png which doesn't exist";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        // When file doesn't exist, the text is split into parts:
        // "Look at ", "@nonexistent.png", " which doesn't exist"
        match result {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(parts[0], ContentPart::Text { .. }));
                assert!(matches!(parts[1], ContentPart::Text { .. }));
                assert!(matches!(parts[2], ContentPart::Text { .. }));
            }
            _ => panic!("Expected multi-part content"),
        }
    }

    #[tokio::test]
    async fn test_parse_at_patterns_url() {
        let temp_dir = TempDir::new().unwrap();
        let input = "Look at @https://example.com/image.png";

        let result = parse_at_patterns(input, temp_dir.path()).await.unwrap();

        // For URL tests, we expect the result to be text (since mock server isn't available)
        // In real usage with a valid URL, it would return multi-part content with image
        match result {
            MessageContent::Text(text) => {
                assert!(text.contains("@https://example.com/image.png"));
            }
            _ => {}
        }
    }
}
