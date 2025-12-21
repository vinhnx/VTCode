//! Split tool results: Dual-channel output for LLM and UI
//!
//! Implements Phase 4 of pi-coding-agent integration:
//! - LLM content: Concise summaries optimized for token efficiency
//! - UI content: Rich output with full details for user display
//!
//! Expected savings: 20-30% on tool-heavy sessions (97% on tool output tokens)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Result from tool execution with dual-channel output
///
/// Tools return two versions of their output:
/// 1. `llm_content` - Concise summary for model context (token-optimized)
/// 2. `ui_content` - Rich output for user display (full details)
///
/// This enables significant token savings while preserving user experience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool name that produced this result
    pub tool_name: String,

    /// Concise summary for LLM context (token-optimized)
    ///
    /// Example: "Found 127 matches in 15 files. Key: src/tools/grep.rs (3), src/tools/list.rs (1)"
    /// vs full output which might be 2,500 tokens
    pub llm_content: String,

    /// Rich output for UI display (full details)
    ///
    /// Can include ANSI codes, formatting, full listings, etc.
    /// Not sent to LLM, only displayed to user
    pub ui_content: String,

    /// Whether the tool execution succeeded
    pub success: bool,

    /// Error message if execution failed
    pub error: Option<String>,

    /// Structured metadata for both channels
    pub metadata: ToolMetadata,
}

/// Metadata accompanying tool results
///
/// Provides structured data that can be used by both LLM and UI
/// without being embedded in content strings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolMetadata {
    /// File paths referenced by this tool (for UI linking, LLM context)
    pub files: Vec<PathBuf>,

    /// Line numbers referenced (for UI jump-to-line)
    pub lines: Vec<usize>,

    /// Key-value pairs for structured data
    ///
    /// Examples:
    /// - match_count: 127
    /// - files_searched: 50
    /// - execution_time_ms: 234
    pub data: HashMap<String, serde_json::Value>,

    /// Token counts for observability
    pub token_counts: TokenCounts,
}

/// Token counting for split tool results
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenCounts {
    /// Tokens in LLM content (what we send to model)
    pub llm_tokens: usize,

    /// Tokens in UI content (what we DON'T send to model)
    pub ui_tokens: usize,

    /// Tokens saved by splitting (ui_tokens - llm_tokens)
    pub savings_tokens: usize,

    /// Percentage saved (0-100)
    pub savings_percent: f32,
}

impl ToolResult {
    /// Create a new tool result with dual content
    pub fn new(
        tool_name: impl Into<String>,
        llm_content: impl Into<String>,
        ui_content: impl Into<String>,
    ) -> Self {
        let llm_str = llm_content.into();
        let ui_str = ui_content.into();

        let llm_tokens = estimate_tokens(&llm_str);
        let ui_tokens = estimate_tokens(&ui_str);
        let savings = if ui_tokens > llm_tokens {
            ui_tokens - llm_tokens
        } else {
            0
        };
        let savings_pct = if ui_tokens > 0 {
            (savings as f32 / ui_tokens as f32) * 100.0
        } else {
            0.0
        };

        Self {
            tool_name: tool_name.into(),
            llm_content: llm_str,
            ui_content: ui_str,
            success: true,
            error: None,
            metadata: ToolMetadata {
                token_counts: TokenCounts {
                    llm_tokens,
                    ui_tokens,
                    savings_tokens: savings,
                    savings_percent: savings_pct,
                },
                ..Default::default()
            },
        }
    }

    /// Create an error result
    pub fn error(tool_name: impl Into<String>, error: impl Into<String>) -> Self {
        let error_msg = error.into();
        Self {
            tool_name: tool_name.into(),
            llm_content: format!("Tool failed: {}", error_msg),
            ui_content: format!("Error: {}", error_msg),
            success: false,
            error: Some(error_msg),
            metadata: ToolMetadata::default(),
        }
    }

    /// Create a simple result with same content for both channels
    ///
    /// Use this for backward compatibility or when splitting doesn't make sense
    pub fn simple(tool_name: impl Into<String>, content: impl Into<String>) -> Self {
        let content_str = content.into();
        Self::new(tool_name, content_str.clone(), content_str)
    }

    /// Add metadata to the result
    pub fn with_metadata(mut self, metadata: ToolMetadata) -> Self {
        // Preserve token counts from construction
        let token_counts = self.metadata.token_counts.clone();
        self.metadata = metadata;
        self.metadata.token_counts = token_counts;
        self
    }

    /// Add file references to metadata
    pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
        self.metadata.files = files;
        self
    }

    /// Add data to metadata
    pub fn with_data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata.data.insert(key.into(), value);
        self
    }

    /// Get token savings summary for logging
    pub fn savings_summary(&self) -> String {
        let counts = &self.metadata.token_counts;
        format!(
            "{} → {} tokens ({:.1}% saved)",
            counts.ui_tokens, counts.llm_tokens, counts.savings_percent
        )
    }

    /// Check if this result has significant savings (>50%)
    pub fn has_significant_savings(&self) -> bool {
        self.metadata.token_counts.savings_percent > 50.0
    }
}

/// Estimate token count from string
///
/// Simple estimation: 1 token ≈ 4 characters
/// This is conservative and works well for English text
fn estimate_tokens(text: &str) -> usize {
    (text.len() as f32 / 4.0).ceil() as usize
}

/// Builder for ToolMetadata
pub struct ToolMetadataBuilder {
    files: Vec<PathBuf>,
    lines: Vec<usize>,
    data: HashMap<String, serde_json::Value>,
}

impl ToolMetadataBuilder {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            lines: Vec::new(),
            data: HashMap::new(),
        }
    }

    pub fn file(mut self, path: PathBuf) -> Self {
        self.files.push(path);
        self
    }

    pub fn files(mut self, paths: Vec<PathBuf>) -> Self {
        self.files.extend(paths);
        self
    }

    pub fn line(mut self, line: usize) -> Self {
        self.lines.push(line);
        self
    }

    pub fn lines(mut self, lines: Vec<usize>) -> Self {
        self.lines.extend(lines);
        self
    }

    pub fn data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }

    pub fn build(self) -> ToolMetadata {
        ToolMetadata {
            files: self.files,
            lines: self.lines,
            data: self.data,
            token_counts: TokenCounts::default(), // Will be filled by ToolResult
        }
    }
}

impl Default for ToolMetadataBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_creation() {
        let result = ToolResult::new(
            "grep_file",
            "Found 127 matches in 15 files",
            "Very long output with 127 full match listings...",
        );

        assert_eq!(result.tool_name, "grep_file");
        assert!(result.success);
        assert!(result.error.is_none());
        assert!(result.metadata.token_counts.llm_tokens > 0);
        assert!(result.metadata.token_counts.ui_tokens > 0);
        assert!(result.metadata.token_counts.savings_tokens > 0);
    }

    #[test]
    fn test_error_result() {
        let result = ToolResult::error("grep_file", "Pattern invalid");

        assert_eq!(result.tool_name, "grep_file");
        assert!(!result.success);
        assert_eq!(result.error, Some("Pattern invalid".to_string()));
        assert!(result.llm_content.contains("failed"));
    }

    #[test]
    fn test_simple_result() {
        let result = ToolResult::simple("test_tool", "Same content");

        assert_eq!(result.llm_content, result.ui_content);
        assert_eq!(result.metadata.token_counts.savings_tokens, 0);
    }

    #[test]
    fn test_token_estimation() {
        let text = "Hello world";
        let tokens = estimate_tokens(text);
        // "Hello world" = 11 chars / 4 ≈ 3 tokens
        assert_eq!(tokens, 3);

        let long_text = "a".repeat(1000);
        let long_tokens = estimate_tokens(&long_text);
        // 1000 chars / 4 = 250 tokens
        assert_eq!(long_tokens, 250);
    }

    #[test]
    fn test_metadata_builder() {
        let metadata = ToolMetadataBuilder::new()
            .file(PathBuf::from("src/main.rs"))
            .file(PathBuf::from("src/lib.rs"))
            .line(42)
            .line(100)
            .data("match_count", serde_json::json!(127))
            .data("files_searched", serde_json::json!(50))
            .build();

        assert_eq!(metadata.files.len(), 2);
        assert_eq!(metadata.lines.len(), 2);
        assert_eq!(metadata.data.len(), 2);
        assert_eq!(metadata.data["match_count"], 127);
    }

    #[test]
    fn test_with_methods() {
        let result = ToolResult::new("test", "llm", "ui")
            .with_files(vec![PathBuf::from("test.rs")])
            .with_data("key", serde_json::json!("value"));

        assert_eq!(result.metadata.files.len(), 1);
        assert_eq!(result.metadata.data["key"], "value");
    }

    #[test]
    fn test_savings_calculation() {
        let result = ToolResult::new(
            "grep",
            "Short summary",  // ~4 tokens
            "a".repeat(1000), // ~250 tokens
        );

        assert!(result.metadata.token_counts.savings_tokens > 200);
        assert!(result.metadata.token_counts.savings_percent > 90.0);
        assert!(result.has_significant_savings());
    }

    #[test]
    fn test_savings_summary() {
        let result = ToolResult::new("grep", "Short", "Long content here");

        let summary = result.savings_summary();
        assert!(summary.contains("→"));
        assert!(summary.contains("tokens"));
        assert!(summary.contains("%"));
    }
}
