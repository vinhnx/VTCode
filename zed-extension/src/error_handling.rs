/// Error Handling and User Experience
///
/// Provides comprehensive error handling, user-friendly messages, and recovery strategies.
/// Implements graceful degradation and helpful troubleshooting guidance.
use std::fmt;

/// Error type for VT Code extension operations
#[derive(Debug, Clone)]
pub struct ExtensionError {
    /// Error code for programmatic handling
    pub code: ErrorCode,
    /// User-friendly error message
    pub message: String,
    /// Technical details for debugging
    pub details: Option<String>,
    /// Suggested remediation steps
    pub suggestions: Vec<String>,
    /// Severity level
    pub severity: ErrorSeverity,
}

/// Error codes for VT Code extension
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// VT Code CLI not found in PATH
    CliNotFound,
    /// VT Code CLI execution failed
    CliExecutionFailed,
    /// Configuration file not found or invalid
    ConfigError,
    /// Invalid workspace path
    InvalidWorkspace,
    /// File operation failed
    FileOperationFailed,
    /// Workspace scanning timeout
    ScanTimeout,
    /// Memory limit exceeded
    MemoryLimitExceeded,
    /// Unsupported file type
    UnsupportedFileType,
    /// Context too large
    ContextTooLarge,
    /// Unknown error
    Unknown,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::CliNotFound => "CliNotFound",
            ErrorCode::CliExecutionFailed => "CliExecutionFailed",
            ErrorCode::ConfigError => "ConfigError",
            ErrorCode::InvalidWorkspace => "InvalidWorkspace",
            ErrorCode::FileOperationFailed => "FileOperationFailed",
            ErrorCode::ScanTimeout => "ScanTimeout",
            ErrorCode::MemoryLimitExceeded => "MemoryLimitExceeded",
            ErrorCode::UnsupportedFileType => "UnsupportedFileType",
            ErrorCode::ContextTooLarge => "ContextTooLarge",
            ErrorCode::Unknown => "Unknown",
        }
    }
}

/// Error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    /// Informational message
    Info,
    /// Warning - operation may be degraded
    Warning,
    /// Error - operation failed but recovery possible
    Error,
    /// Critical - operation failed completely
    Critical,
}

impl ErrorSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorSeverity::Info => "info",
            ErrorSeverity::Warning => "warning",
            ErrorSeverity::Error => "error",
            ErrorSeverity::Critical => "critical",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ErrorSeverity::Info => "[I]",
            ErrorSeverity::Warning => "[W]",
            ErrorSeverity::Error => "[E]",
            ErrorSeverity::Critical => "[C]",
        }
    }
}

impl ExtensionError {
    /// Create a new error with code and message
    pub fn new(code: ErrorCode, message: String) -> Self {
        Self {
            code,
            message,
            details: None,
            suggestions: Vec::new(),
            severity: ErrorSeverity::Error,
        }
    }

    /// Create a CLI not found error
    pub fn cli_not_found() -> Self {
        let mut err = Self::new(
            ErrorCode::CliNotFound,
            "VT Code CLI not found in PATH".to_string(),
        );
        err.suggestions = vec![
            "Install VT Code: cargo install vtcode".to_string(),
            "Check PATH: echo $PATH".to_string(),
            "Verify installation: which vtcode".to_string(),
        ];
        err.details = Some("The vtcode command is not available in your system PATH".to_string());
        err
    }

    /// Create a CLI execution failed error
    pub fn cli_execution_failed(reason: String) -> Self {
        let mut err = Self::new(
            ErrorCode::CliExecutionFailed,
            format!("VT Code CLI execution failed: {}", reason),
        );
        err.suggestions = vec![
            "Check VT Code installation: vtcode --version".to_string(),
            "Verify configuration: vtcode config".to_string(),
            "Check logs: ~/.vtcode/logs/".to_string(),
        ];
        err.severity = ErrorSeverity::Error;
        err
    }

    /// Create a config error
    pub fn config_error(reason: String) -> Self {
        let mut err = Self::new(
            ErrorCode::ConfigError,
            format!("Configuration error: {}", reason),
        );
        err.suggestions = vec![
            "Check vtcode.toml syntax".to_string(),
            "Validate configuration: vtcode config validate".to_string(),
            "Review configuration guide: docs/config/config.md".to_string(),
        ];
        err
    }

    /// Create an invalid workspace error
    pub fn invalid_workspace(path: String) -> Self {
        let mut err = Self::new(
            ErrorCode::InvalidWorkspace,
            format!("Invalid workspace path: {}", path),
        );
        err.suggestions = vec![
            "Verify workspace path exists".to_string(),
            "Check directory permissions".to_string(),
            format!("Use absolute paths: {}", path),
        ];
        err
    }

    /// Create a file operation error
    pub fn file_operation_failed(operation: String, reason: String) -> Self {
        let mut err = Self::new(
            ErrorCode::FileOperationFailed,
            format!("File operation failed: {} ({})", operation, reason),
        );
        err.suggestions = vec![
            "Check file permissions".to_string(),
            "Verify file exists".to_string(),
            "Check available disk space".to_string(),
        ];
        err
    }

    /// Create a timeout error
    pub fn scan_timeout(duration_secs: u64) -> Self {
        let mut err = Self::new(
            ErrorCode::ScanTimeout,
            format!("Workspace scan timed out after {} seconds", duration_secs),
        );
        err.suggestions = vec![
            "Try excluding large directories: .gitignore".to_string(),
            format!("Increase timeout: --timeout {}", duration_secs * 2),
            "Check system performance".to_string(),
        ];
        err.severity = ErrorSeverity::Warning;
        err
    }

    /// Create a memory limit error
    pub fn memory_limit_exceeded(limit_mb: usize) -> Self {
        let mut err = Self::new(
            ErrorCode::MemoryLimitExceeded,
            format!("Memory limit exceeded: {} MB", limit_mb),
        );
        err.suggestions = vec![
            "Reduce workspace scope".to_string(),
            format!("Increase memory limit: --memory-limit {}", limit_mb * 2),
            "Exclude large files or directories".to_string(),
        ];
        err.severity = ErrorSeverity::Error;
        err
    }

    /// Create a context too large error
    pub fn context_too_large(actual_tokens: usize, limit_tokens: usize) -> Self {
        let mut err = Self::new(
            ErrorCode::ContextTooLarge,
            format!(
                "Context too large: {} tokens (limit: {})",
                actual_tokens, limit_tokens
            ),
        );
        err.suggestions = vec![
            "Select smaller code sections".to_string(),
            "Close some open files".to_string(),
            "Exclude node_modules or build directories".to_string(),
        ];
        err.severity = ErrorSeverity::Warning;
        err
    }

    /// Add technical details
    pub fn with_details(mut self, details: String) -> Self {
        self.details = Some(details);
        self
    }

    /// Add a suggestion
    pub fn with_suggestion(mut self, suggestion: String) -> Self {
        self.suggestions.push(suggestion);
        self
    }

    /// Add multiple suggestions
    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions.extend(suggestions);
        self
    }

    /// Set severity level
    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }

    /// Format error for display
    pub fn format_display(&self) -> String {
        let mut output = format!(
            "{} [{}] {}",
            self.severity.icon(),
            self.code.as_str(),
            self.message
        );

        if let Some(details) = &self.details {
            output.push_str(&format!("\n\nDetails: {}", details));
        }

        if !self.suggestions.is_empty() {
            output.push_str("\n\nSuggested actions:");
            for (i, suggestion) in self.suggestions.iter().enumerate() {
                output.push_str(&format!("\n  {}. {}", i + 1, suggestion));
            }
        }

        output
    }

    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(self.severity, ErrorSeverity::Info | ErrorSeverity::Warning)
    }
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_display())
    }
}

/// Recovery strategy for degraded operations
#[derive(Debug, Clone)]
pub struct RecoveryStrategy {
    /// Name of the strategy
    pub name: String,
    /// Description
    pub description: String,
    /// Steps to execute
    pub steps: Vec<String>,
    /// Expected outcome
    pub expected_outcome: String,
}

impl RecoveryStrategy {
    /// Create a new recovery strategy
    pub fn new(name: String, description: String, expected_outcome: String) -> Self {
        Self {
            name,
            description,
            steps: Vec::new(),
            expected_outcome,
        }
    }

    /// Add a recovery step
    pub fn add_step(mut self, step: String) -> Self {
        self.steps.push(step);
        self
    }

    /// Strategy for CLI not found
    pub fn cli_not_found_recovery() -> Self {
        Self::new(
            "Install VT Code CLI".to_string(),
            "The VT Code command-line interface is not installed".to_string(),
            "VT Code CLI available in PATH".to_string(),
        )
        .add_step("Install Rust: https://rustup.rs/".to_string())
        .add_step("Install VT Code: cargo install vtcode".to_string())
        .add_step("Verify: vtcode --version".to_string())
    }

    /// Strategy for degraded workspace analysis
    pub fn degraded_workspace_analysis() -> Self {
        Self::new(
            "Limit workspace scope".to_string(),
            "Workspace is too large for full analysis".to_string(),
            "Workspace analyzed with reduced scope".to_string(),
        )
        .add_step("Create .vtcodeignore file".to_string())
        .add_step("Add large directories: node_modules, build, .git".to_string())
        .add_step("Retry analysis".to_string())
    }

    /// Format recovery strategy for display
    pub fn format_display(&self) -> String {
        let mut output = format!(
            "Recovery Strategy: {}\n\n{}\n\n",
            self.name, self.description
        );
        output.push_str("Steps:\n");
        for (i, step) in self.steps.iter().enumerate() {
            output.push_str(&format!("  {}. {}\n", i + 1, step));
        }
        output.push_str(&format!("\nExpected outcome: {}", self.expected_outcome));
        output
    }
}

/// Result type for extension operations
pub type ExtensionResult<T> = Result<T, ExtensionError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_str() {
        assert_eq!(ErrorCode::CliNotFound.as_str(), "CliNotFound");
        assert_eq!(ErrorCode::ConfigError.as_str(), "ConfigError");
    }

    #[test]
    fn test_error_severity_str() {
        assert_eq!(ErrorSeverity::Info.as_str(), "info");
        assert_eq!(ErrorSeverity::Warning.as_str(), "warning");
        assert_eq!(ErrorSeverity::Error.as_str(), "error");
        assert_eq!(ErrorSeverity::Critical.as_str(), "critical");
    }

    #[test]
    fn test_error_severity_icon() {
        assert_eq!(ErrorSeverity::Info.icon(), "[I]");
        assert_eq!(ErrorSeverity::Warning.icon(), "[W]");
        assert_eq!(ErrorSeverity::Error.icon(), "[E]");
        assert_eq!(ErrorSeverity::Critical.icon(), "[C]");
    }

    #[test]
    fn test_severity_ordering() {
        assert!(ErrorSeverity::Info < ErrorSeverity::Warning);
        assert!(ErrorSeverity::Warning < ErrorSeverity::Error);
        assert!(ErrorSeverity::Error < ErrorSeverity::Critical);
    }

    #[test]
    fn test_cli_not_found_error() {
        let err = ExtensionError::cli_not_found();
        assert_eq!(err.code, ErrorCode::CliNotFound);
        assert!(!err.suggestions.is_empty());
        assert!(err.details.is_some());
    }

    #[test]
    fn test_cli_execution_failed_error() {
        let err = ExtensionError::cli_execution_failed("timeout".to_string());
        assert_eq!(err.code, ErrorCode::CliExecutionFailed);
        assert!(err.message.contains("timeout"));
    }

    #[test]
    fn test_config_error() {
        let err = ExtensionError::config_error("missing field".to_string());
        assert_eq!(err.code, ErrorCode::ConfigError);
        assert!(!err.suggestions.is_empty());
    }

    #[test]
    fn test_error_with_details() {
        let err = ExtensionError::new(ErrorCode::Unknown, "Test error".to_string())
            .with_details("Additional details".to_string());
        assert_eq!(err.details, Some("Additional details".to_string()));
    }

    #[test]
    fn test_error_with_suggestion() {
        let err = ExtensionError::new(ErrorCode::Unknown, "Test error".to_string())
            .with_suggestion("Try this".to_string());
        assert_eq!(err.suggestions.len(), 1);
    }

    #[test]
    fn test_error_with_suggestions() {
        let suggestions = vec!["Try 1".to_string(), "Try 2".to_string()];
        let err = ExtensionError::new(ErrorCode::Unknown, "Test error".to_string())
            .with_suggestions(suggestions);
        assert_eq!(err.suggestions.len(), 2);
    }

    #[test]
    fn test_error_with_severity() {
        let err = ExtensionError::new(ErrorCode::Unknown, "Test error".to_string())
            .with_severity(ErrorSeverity::Critical);
        assert_eq!(err.severity, ErrorSeverity::Critical);
    }

    #[test]
    fn test_error_is_recoverable() {
        let info = ExtensionError::new(ErrorCode::Unknown, "Test".to_string())
            .with_severity(ErrorSeverity::Info);
        assert!(info.is_recoverable());

        let warning = ExtensionError::new(ErrorCode::Unknown, "Test".to_string())
            .with_severity(ErrorSeverity::Warning);
        assert!(warning.is_recoverable());

        let error = ExtensionError::new(ErrorCode::Unknown, "Test".to_string())
            .with_severity(ErrorSeverity::Error);
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_error_format_display() {
        let err = ExtensionError::new(ErrorCode::Unknown, "Test error".to_string())
            .with_details("Details".to_string())
            .with_suggestion("Try this".to_string());

        let display = err.format_display();
        assert!(display.contains("Test error"));
        assert!(display.contains("Details"));
        assert!(display.contains("Try this"));
    }

    #[test]
    fn test_memory_limit_error() {
        let err = ExtensionError::memory_limit_exceeded(1024);
        assert_eq!(err.code, ErrorCode::MemoryLimitExceeded);
        assert_eq!(err.severity, ErrorSeverity::Error);
        assert!(err.message.contains("1024"));
    }

    #[test]
    fn test_scan_timeout_error() {
        let err = ExtensionError::scan_timeout(30);
        assert_eq!(err.code, ErrorCode::ScanTimeout);
        assert_eq!(err.severity, ErrorSeverity::Warning);
    }

    #[test]
    fn test_context_too_large_error() {
        let err = ExtensionError::context_too_large(10000, 5000);
        assert_eq!(err.code, ErrorCode::ContextTooLarge);
        assert!(err.message.contains("10000"));
        assert!(err.message.contains("5000"));
    }

    #[test]
    fn test_recovery_strategy_creation() {
        let strategy = RecoveryStrategy::new(
            "Test".to_string(),
            "Test description".to_string(),
            "Expected outcome".to_string(),
        );

        assert_eq!(strategy.name, "Test");
        assert!(strategy.steps.is_empty());
    }

    #[test]
    fn test_recovery_strategy_add_step() {
        let strategy = RecoveryStrategy::new(
            "Test".to_string(),
            "Desc".to_string(),
            "Outcome".to_string(),
        )
        .add_step("Step 1".to_string())
        .add_step("Step 2".to_string());

        assert_eq!(strategy.steps.len(), 2);
    }

    #[test]
    fn test_cli_not_found_recovery() {
        let recovery = RecoveryStrategy::cli_not_found_recovery();
        assert!(recovery.name.contains("VT Code"));
        assert_eq!(recovery.steps.len(), 3);
    }

    #[test]
    fn test_degraded_workspace_recovery() {
        let recovery = RecoveryStrategy::degraded_workspace_analysis();
        assert!(!recovery.steps.is_empty());
        assert!(recovery.name.contains("Limit"));
    }



    #[test]
    fn test_recovery_strategy_format() {
        let recovery = RecoveryStrategy::new(
            "Test".to_string(),
            "Desc".to_string(),
            "Outcome".to_string(),
        )
        .add_step("Step 1".to_string());

        let display = recovery.format_display();
        assert!(display.contains("Test"));
        assert!(display.contains("Desc"));
        assert!(display.contains("Step 1"));
        assert!(display.contains("Outcome"));
    }

    #[test]
    fn test_extension_error_display() {
        let err = ExtensionError::cli_not_found();
        let display = format!("{}", err);
        assert!(display.contains("VT Code CLI not found"));
    }

    #[test]
    fn test_invalid_workspace_error() {
        let err = ExtensionError::invalid_workspace("/nonexistent".to_string());
        assert_eq!(err.code, ErrorCode::InvalidWorkspace);
        assert!(err.message.contains("/nonexistent"));
    }

    #[test]
    fn test_file_operation_error() {
        let err = ExtensionError::file_operation_failed(
            "read".to_string(),
            "permission denied".to_string(),
        );
        assert_eq!(err.code, ErrorCode::FileOperationFailed);
        assert!(err.message.contains("read"));
    }
}
