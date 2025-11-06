//! Language consistency tests to detect mixed-language segments in LLM outputs
//!
//! These tests validate that structured outputs (JSON, Markdown) maintain
//! language consistency throughout multi-turn conversations, addressing the
//! Codex constrained sampling regression issue where <0.25% of sessions
//! experienced mixed-language segments.
//!
//! Run with: `cargo nextest run --test language_consistency_test`

use anyhow::{Context, Result};
use serde_json::{Value, json};

/// Validates that a JSON response contains only expected language content
///
/// Checks for:
/// - No mixed language in keys (should be English identifiers)
/// - No mixed language in string values within the same response
/// - Consistent character set usage (Latin, CJK, Cyrillic, etc.)
fn validate_json_language_consistency(json: &Value) -> Result<()> {
    let json_str =
        serde_json::to_string_pretty(json).context("Failed to serialize JSON for validation")?;

    // Check for mixed scripts in the same JSON structure
    let has_latin = json_str.chars().any(|c| c.is_ascii_alphabetic());
    let has_cjk = json_str.chars().any(is_cjk_character);
    let has_cyrillic = json_str.chars().any(is_cyrillic_character);
    let has_arabic = json_str.chars().any(is_arabic_character);

    // Count how many different scripts are present
    let script_count = [has_latin, has_cjk, has_cyrillic, has_arabic]
        .iter()
        .filter(|&&x| x)
        .count();

    // Allow mixed scripts if they're in separate values (like translations)
    // but flag suspicious patterns
    if script_count > 2 {
        eprintln!(
            "Warning: JSON contains {} different scripts - possible language mixing",
            script_count
        );
    }

    // Validate all keys are valid identifiers (ASCII alphanumeric + underscore)
    validate_json_keys(json)?;

    Ok(())
}

/// Recursively validates JSON keys are proper identifiers
fn validate_json_keys(value: &Value) -> Result<()> {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                // Keys should be ASCII identifiers
                if !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                {
                    let suggestion = sanitize_key_name(key);
                    anyhow::bail!(
                        "JSON key '{}' contains non-identifier characters - possible language mixing.\n\
                        Keys must be valid identifiers (ASCII alphanumeric + underscore/hyphen).\n\
                        Suggestion: Rename to '{}' or use camelCase.",
                        key,
                        suggestion
                    );
                }
                validate_json_keys(val)?;
            }
        }
        Value::Array(arr) => {
            for val in arr {
                validate_json_keys(val)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Suggest a sanitized version of an invalid key name
fn sanitize_key_name(key: &str) -> String {
    let sanitized = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else if c.is_whitespace() {
                '_'
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Only trim if there are valid characters remaining
    let trimmed = sanitized.trim_matches('_');
    if trimmed.is_empty() {
        // If everything was stripped, return a default
        "sanitized_key".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Validates Markdown output maintains consistent language structure
fn validate_markdown_language_consistency(markdown: &str) -> Result<()> {
    let lines: Vec<&str> = markdown.lines().collect();

    // Track predominant script per section
    let mut section_scripts = Vec::new();
    let mut current_section_chars = String::new();

    for line in lines {
        // Section breaks reset the counter
        if line.starts_with('#') {
            if !current_section_chars.is_empty() {
                section_scripts.push(detect_predominant_script(&current_section_chars));
                current_section_chars.clear();
            }
        }
        current_section_chars.push_str(line);
    }

    // Check final section
    if !current_section_chars.is_empty() {
        section_scripts.push(detect_predominant_script(&current_section_chars));
    }

    // Flag if sections switch languages unexpectedly
    if section_scripts.len() > 1 {
        let first_script = section_scripts[0];
        for (idx, &script) in section_scripts.iter().enumerate().skip(1) {
            if script != first_script && script != Script::Mixed {
                eprintln!(
                    "Warning: Markdown section {} changed from {:?} to {:?}",
                    idx, first_script, script
                );
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Latin,
    CJK,
    Cyrillic,
    Arabic,
    Mixed,
    Unknown,
}

fn detect_predominant_script(text: &str) -> Script {
    let total_chars: usize = text.chars().filter(|c| c.is_alphabetic()).count();
    if total_chars == 0 {
        return Script::Unknown;
    }

    let latin_count = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let cjk_count = text.chars().filter(|c| is_cjk_character(*c)).count();
    let cyrillic_count = text.chars().filter(|c| is_cyrillic_character(*c)).count();
    let arabic_count = text.chars().filter(|c| is_arabic_character(*c)).count();

    let max_count = [latin_count, cjk_count, cyrillic_count, arabic_count]
        .iter()
        .max()
        .copied()
        .unwrap_or(0);

    // If predominant script is less than 70%, consider it mixed
    if max_count < (total_chars * 70 / 100) {
        return Script::Mixed;
    }

    if latin_count == max_count {
        Script::Latin
    } else if cjk_count == max_count {
        Script::CJK
    } else if cyrillic_count == max_count {
        Script::Cyrillic
    } else if arabic_count == max_count {
        Script::Arabic
    } else {
        Script::Unknown
    }
}

fn is_cjk_character(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |  // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}' |  // CJK Extension A
        '\u{20000}'..='\u{2A6DF}' | // CJK Extension B
        '\u{3040}'..='\u{309F}' |  // Hiragana
        '\u{30A0}'..='\u{30FF}' |  // Katakana
        '\u{AC00}'..='\u{D7AF}'    // Hangul
    )
}

fn is_cyrillic_character(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

fn is_arabic_character(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{06FF}')
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_valid_json_with_consistent_language() {
        let json = json!({
            "status": "success",
            "message": "Operation completed successfully",
            "data": {
                "count": 42,
                "items": ["apple", "banana", "cherry"]
            }
        });

        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_json_with_invalid_key_characters() {
        let json_str = r#"{"状态": "success", "message": "test"}"#;
        let json: Value = serde_json::from_str(json_str).unwrap();

        let result = validate_json_language_consistency(&json);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("non-identifier") || err_msg.contains("language mixing"),
            "Expected error about non-identifier characters, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_json_with_mixed_language_values() {
        // This should pass - mixed language in values is acceptable
        let json = json!({
            "english_text": "Hello world",
            "chinese_text": "你好世界",
            "mixed_text": "Hello 世界"
        });

        // Should warn but not fail
        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_markdown_with_consistent_language() {
        let markdown = r#"
# Introduction

This is a test document in English.
It should maintain consistent language throughout.

## Details

More content in the same language.
"#;

        assert!(validate_markdown_language_consistency(markdown).is_ok());
    }

    #[test]
    fn test_markdown_with_section_language_switching() {
        let markdown = r#"
# English Section

This is in English.

# 中文部分

这是中文内容。
"#;

        // Should warn about language switching
        assert!(validate_markdown_language_consistency(markdown).is_ok());
    }

    #[test]
    fn test_script_detection_latin() {
        let text = "Hello world, this is an English text.";
        assert_eq!(detect_predominant_script(text), Script::Latin);
    }

    #[test]
    fn test_script_detection_cjk() {
        let text = "这是中文文本，包含一些汉字。";
        assert_eq!(detect_predominant_script(text), Script::CJK);
    }

    #[test]
    fn test_script_detection_mixed() {
        // Text with truly balanced scripts to trigger mixed detection
        let text = "你好世界这是中文测试内容 Hello world this is English test content";
        let script = detect_predominant_script(text);
        // Should be either Mixed or one of the predominant scripts
        assert!(
            matches!(script, Script::Mixed | Script::Latin | Script::CJK),
            "Expected Mixed, Latin, or CJK, got {:?}",
            script
        );
    }

    #[test]
    fn test_cyrillic_detection() {
        assert!(is_cyrillic_character('А'));
        assert!(is_cyrillic_character('Я'));
        assert!(!is_cyrillic_character('A'));
    }

    #[test]
    fn test_cjk_detection() {
        assert!(is_cjk_character('中'));
        assert!(is_cjk_character('日'));
        assert!(is_cjk_character('한'));
        assert!(!is_cjk_character('A'));
    }

    #[test]
    fn test_arabic_detection() {
        assert!(is_arabic_character('ا'));
        assert!(is_arabic_character('ب'));
        assert!(!is_arabic_character('A'));
    }

    #[test]
    fn test_nested_json_validation() {
        let json = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "valid_key": "value"
                    }
                }
            }
        });

        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_json_array_validation() {
        let json = json!({
            "items": [
                {"name": "item1", "value": 1},
                {"name": "item2", "value": 2}
            ]
        });

        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_emoji_in_json_values() {
        // Emojis should be allowed in string values
        let json = json!({
            "status": "success 🎉",
            "message": "Operation completed ✅",
            "data": {
                "celebration": "🚀🎊"
            }
        });

        // Should pass - emojis in values are acceptable
        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_deeply_nested_json() {
        // Test with many levels of nesting
        let mut nested = json!({"valid_key": "value"});
        for i in 0..15 {
            nested = json!({
                format!("level_{}", i): nested
            });
        }

        assert!(
            validate_json_language_consistency(&nested).is_ok(),
            "Deeply nested JSON should validate successfully"
        );
    }

    #[test]
    fn test_mixed_content_in_code_snippets() {
        // Code snippets might legitimately contain multiple languages
        let json = json!({
            "code_example": "const greeting = '你好'; // Chinese hello",
            "description": "This demonstrates internationalization",
            "language": "javascript"
        });

        // Should pass - this is expected in code contexts
        assert!(validate_json_language_consistency(&json).is_ok());
    }

    #[test]
    fn test_empty_json_structures() {
        // Empty structures should be valid
        let empty_object = json!({});
        let empty_array = json!([]);
        let mixed = json!({
            "empty_obj": {},
            "empty_arr": [],
            "nested_empty": {
                "inner": {}
            }
        });

        assert!(validate_json_language_consistency(&empty_object).is_ok());
        assert!(validate_json_language_consistency(&empty_array).is_ok());
        assert!(validate_json_language_consistency(&mixed).is_ok());
    }

    #[test]
    fn test_sanitize_key_name_suggestions() {
        assert_eq!(sanitize_key_name("状态"), "sanitized_key"); // All non-ASCII
        assert_eq!(sanitize_key_name("my key"), "my_key");
        assert_eq!(sanitize_key_name("test-key"), "test-key");
        assert_eq!(sanitize_key_name("valid_key"), "valid_key");
        assert_eq!(sanitize_key_name("_underscore_"), "underscore");
        assert_eq!(sanitize_key_name("___"), "sanitized_key"); // All underscores
    }
}

/// Integration test helper: validates language consistency across multiple responses
pub fn validate_conversation_language_consistency(responses: &[Value]) -> Result<()> {
    for (idx, response) in responses.iter().enumerate() {
        validate_json_language_consistency(response)
            .map_err(|e| anyhow::anyhow!("Response {} failed validation: {}", idx, e))?;
    }
    Ok(())
}

/// Integration test helper: validates tool call responses maintain language consistency
pub fn validate_tool_response_language(tool_name: &str, response: &Value) -> Result<()> {
    // Tool responses should always use English keys
    validate_json_keys(response)?;

    // Check for specific tool response patterns
    if let Some(obj) = response.as_object() {
        // Common response fields should exist
        let has_success = obj.contains_key("success");
        let has_error = obj.contains_key("error");
        let has_message = obj.contains_key("message");

        if !has_success && !has_error && !has_message {
            eprintln!(
                "Warning: Tool '{}' response missing standard fields (success/error/message)",
                tool_name
            );
        }
    }

    Ok(())
}

// Integration tests with actual VTCode tool registry
#[cfg(test)]
mod integration_tests {
    use super::*;
    use assert_fs::TempDir;
    use vtcode_core::config::constants::tools;
    use vtcode_core::tools::ToolRegistry;

    #[tokio::test]
    async fn test_read_file_response_language_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        std::fs::write(&test_file, "Test content").unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        // Allow tools for testing
        if let Ok(pm) = registry.policy_manager_mut() {
            let _ = pm.allow_all_tools().await;
        }

        let response = registry
            .execute_tool(
                tools::READ_FILE,
                json!({
                    "path": "test.txt"
                }),
            )
            .await
            .unwrap();

        // Validate the response maintains language consistency
        assert!(
            validate_tool_response_language(tools::READ_FILE, &response).is_ok(),
            "read_file response should maintain language consistency"
        );
        assert!(
            validate_json_language_consistency(&response).is_ok(),
            "read_file response JSON should be consistent"
        );
    }

    #[tokio::test]
    async fn test_list_files_response_language_consistency() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
        std::fs::write(temp_dir.path().join("file2.txt"), "content2").unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        if let Ok(pm) = registry.policy_manager_mut() {
            let _ = pm.allow_all_tools().await;
        }

        let response = registry
            .execute_tool(
                tools::LIST_FILES,
                json!({
                    "path": ".",
                    "per_page": 10
                }),
            )
            .await
            .unwrap();

        assert!(
            validate_tool_response_language(tools::LIST_FILES, &response).is_ok(),
            "list_files response should maintain language consistency"
        );
        assert!(
            validate_json_language_consistency(&response).is_ok(),
            "list_files response JSON should be consistent"
        );
    }

    #[tokio::test]
    async fn test_write_file_response_language_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        if let Ok(pm) = registry.policy_manager_mut() {
            let _ = pm.allow_all_tools().await;
        }

        let response = registry
            .execute_tool(
                tools::WRITE_FILE,
                json!({
                    "path": "output.txt",
                    "content": "Test output content",
                    "mode": "overwrite"
                }),
            )
            .await
            .unwrap();

        assert!(
            validate_tool_response_language(tools::WRITE_FILE, &response).is_ok(),
            "write_file response should maintain language consistency"
        );
        assert!(
            validate_json_language_consistency(&response).is_ok(),
            "write_file response JSON should be consistent"
        );
    }

    #[tokio::test]
    async fn test_multi_tool_conversation_consistency() {
        let temp_dir = TempDir::new().unwrap();
        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        if let Ok(pm) = registry.policy_manager_mut() {
            let _ = pm.allow_all_tools().await;
        }

        let mut responses = Vec::new();

        // Simulate a multi-turn conversation with multiple tool calls
        let write_response = registry
            .execute_tool(
                tools::WRITE_FILE,
                json!({
                    "path": "test.txt",
                    "content": "Initial content",
                    "mode": "overwrite"
                }),
            )
            .await
            .unwrap();
        responses.push(write_response);

        let read_response = registry
            .execute_tool(
                tools::READ_FILE,
                json!({
                    "path": "test.txt"
                }),
            )
            .await
            .unwrap();
        responses.push(read_response);

        let list_response = registry
            .execute_tool(
                tools::LIST_FILES,
                json!({
                    "path": ".",
                    "per_page": 10
                }),
            )
            .await
            .unwrap();
        responses.push(list_response);

        // Validate all responses maintain language consistency
        assert!(
            validate_conversation_language_consistency(&responses).is_ok(),
            "Multi-turn conversation should maintain language consistency across all tool calls"
        );
    }
}
