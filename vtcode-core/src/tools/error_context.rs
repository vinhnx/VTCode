/// Enhanced error context for tool execution
///
/// Provides structured error reporting with file/line context,
/// partial output preservation, and helpful suggestions.
use std::fmt;

/// Error context for tool execution failures
#[derive(Debug, Clone)]
pub struct ToolErrorContext {
    /// Tool name that failed
    pub tool_name: String,
    /// Error message
    pub message: String,
    /// File path associated with error, if any
    pub file_path: Option<String>,
    /// Line number where error occurred, if known
    pub line_number: Option<usize>,
    /// Partial output before failure (if tool produced output)
    pub partial_output: Option<String>,
    /// Maximum length of partial output to preserve
    pub max_output_length: usize,
    /// Suggested next steps
    pub suggestions: Vec<String>,
    /// Error chain for debugging
    pub error_chain: Vec<String>,
}

impl ToolErrorContext {
    pub fn new(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            message: message.into(),
            file_path: None,
            line_number: None,
            partial_output: None,
            max_output_length: 500,
            suggestions: Vec::new(),
            error_chain: Vec::new(),
        }
    }

    /// Add file context to error
    pub fn with_file(mut self, path: impl Into<String>) -> Self {
        self.file_path = Some(path.into());
        self
    }

    /// Add line number to error
    pub fn with_line(mut self, line: usize) -> Self {
        self.line_number = Some(line);
        self
    }

    /// Add partial output from tool execution
    pub fn with_partial_output(mut self, output: impl Into<String>) -> Self {
        let output_str = output.into();
        let truncated = if output_str.len() > self.max_output_length {
            format!(
                "{}... (truncated, {} bytes omitted)",
                &output_str[..self.max_output_length],
                output_str.len() - self.max_output_length
            )
        } else {
            output_str
        };
        self.partial_output = Some(truncated);
        self
    }

    /// Add suggestion for recovery
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add error to chain for debugging
    pub fn with_error_in_chain(mut self, error: impl Into<String>) -> Self {
        self.error_chain.push(error.into());
        self
    }

    /// Format error for user display
    pub fn format_for_user(&self) -> String {
        let mut output = String::new();

        output.push_str(&format!("❌ {} failed\n", self.tool_name));
        output.push_str(&format!("   Error: {}\n", self.message));

        if let Some(file) = &self.file_path {
            output.push_str(&format!("   File: {}\n", file));
            if let Some(line) = self.line_number {
                output.push_str(&format!("   Line: {}\n", line));
            }
        }

        if let Some(output_text) = &self.partial_output {
            output.push_str("\n   Partial Output:\n");
            for line in output_text.lines() {
                output.push_str(&format!("   {}\n", line));
            }
        }

        if !self.suggestions.is_empty() {
            output.push_str("\n   💡 Suggestions:\n");
            for (i, suggestion) in self.suggestions.iter().enumerate() {
                output.push_str(&format!("   {}. {}\n", i + 1, suggestion));
            }
        }

        output
    }

    /// Format error with full debug chain
    pub fn format_for_debug(&self) -> String {
        let mut output = self.format_for_user();

        if !self.error_chain.is_empty() {
            output.push_str("\n   Debug Chain:\n");
            for (i, error) in self.error_chain.iter().enumerate() {
                output.push_str(&format!("   [{}] {}\n", i, error));
            }
        }

        output
    }

    /// Get suggested recovery action based on error type
    pub fn with_auto_recovery(mut self) -> Self {
        // Auto-suggest based on common error patterns
        let lower_msg = self.message.to_lowercase();

        if lower_msg.contains("permission denied") {
            self.suggestions
                .push("Check file permissions with `ls -l <path>`".to_string());
            self.suggestions
                .push("Consider running with appropriate privileges".to_string());
        }

        if lower_msg.contains("not found") || lower_msg.contains("no such file") {
            self.suggestions
                .push("Verify the file path exists".to_string());
            self.suggestions
                .push("Check working directory with `pwd`".to_string());
        }

        if lower_msg.contains("timeout") {
            self.suggestions
                .push("Command took too long - increase timeout or optimize command".to_string());
            self.suggestions
                .push("Check if file is very large or operation is I/O intensive".to_string());
        }

        if lower_msg.contains("parse") || lower_msg.contains("syntax") {
            self.suggestions
                .push("Check file format and syntax".to_string());
            if let Some(path) = &self.file_path.clone() {
                self.suggestions.push(format!("Validate {}", path));
            }
        }

        if lower_msg.contains("memory") || lower_msg.contains("overflow") {
            self.suggestions
                .push("Operation exceeded memory limits".to_string());
            self.suggestions
                .push("Try with smaller input or split into multiple operations".to_string());
        }

        self
    }
}

impl fmt::Display for ToolErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_for_user())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creates_error_context() {
        let ctx = ToolErrorContext::new("grep_file", "Pattern not found");
        assert_eq!(ctx.tool_name, "grep_file");
        assert_eq!(ctx.message, "Pattern not found");
        assert!(ctx.partial_output.is_none());
        assert!(ctx.suggestions.is_empty());
    }

    #[test]
    fn test_adds_file_context() {
        let ctx = ToolErrorContext::new("read_file", "I/O error")
            .with_file("/path/to/file.rs")
            .with_line(42);

        assert_eq!(ctx.file_path, Some("/path/to/file.rs".to_string()));
        assert_eq!(ctx.line_number, Some(42));
    }

    #[test]
    fn test_truncates_long_output() {
        let long_output = "x".repeat(1000);
        let ctx = ToolErrorContext::new("command", "Failed").with_partial_output(&long_output);

        let output = ctx.partial_output.unwrap();
        assert!(output.contains("truncated"));
        assert!(output.len() < long_output.len());
    }

    #[test]
    fn test_formats_for_user() {
        let ctx = ToolErrorContext::new("grep_file", "Permission denied")
            .with_file("secret.txt")
            .with_line(1)
            .with_suggestion("Check file permissions");

        let formatted = ctx.format_for_user();
        assert!(formatted.contains("grep_file"));
        assert!(formatted.contains("Permission denied"));
        assert!(formatted.contains("secret.txt"));
        assert!(formatted.contains("Line: 1"));
        assert!(formatted.contains("Check file permissions"));
    }

    #[test]
    fn test_suggest_recovery_for_permission_error() {
        let ctx = ToolErrorContext::new("command", "permission denied").with_auto_recovery();

        assert!(!ctx.suggestions.is_empty());
        assert!(ctx.suggestions.iter().any(|s| s.contains("permission")));
    }

    #[test]
    fn test_suggest_recovery_for_timeout() {
        let ctx = ToolErrorContext::new("command", "operation timeout").with_auto_recovery();

        assert!(!ctx.suggestions.is_empty());
        assert!(
            ctx.suggestions
                .iter()
                .any(|s| s.to_lowercase().contains("timeout"))
        );
    }

    #[test]
    fn test_error_chain_display() {
        let ctx = ToolErrorContext::new("tool", "Failed")
            .with_error_in_chain("Root cause 1")
            .with_error_in_chain("Root cause 2");

        let debug_output = ctx.format_for_debug();
        assert!(debug_output.contains("Debug Chain"));
        assert!(debug_output.contains("Root cause 1"));
        assert!(debug_output.contains("Root cause 2"));
    }
}
