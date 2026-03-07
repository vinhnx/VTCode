//! Structured error handling for VT Code.
//!
//! Provides a VT Code-specific error envelope with machine-readable codes and
//! contextual information while reusing the shared `vtcode_commons`
//! classification system.

use serde::{Deserialize, Serialize};
use thiserror::Error;
pub use vtcode_commons::{BackoffStrategy, ErrorCategory, Retryability};

/// Result type alias for VT Code operations.
pub type Result<T> = std::result::Result<T, VtCodeError>;

/// Core error type for VT Code operations.
///
/// Uses `thiserror::Error` for automatic `std::error::Error` implementation
/// and provides clear error messages with context.
#[derive(Debug, Error, Serialize, Deserialize)]
#[error("{category}: {message}")]
pub struct VtCodeError {
    /// Error category for categorization and handling.
    pub category: ErrorCategory,

    /// Machine-readable error code.
    pub code: ErrorCode,

    /// Human-readable error message.
    pub message: String,

    /// Optional context for debugging.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Optional source error for chained errors.
    #[serde(skip)]
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

/// Machine-readable error codes for precise error identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    // Input errors
    InvalidArgument,
    ValidationFailed,
    ParseError,

    // Execution errors
    CommandFailed,
    ToolExecutionFailed,
    Timeout,

    // Network errors
    ConnectionFailed,
    RequestFailed,
    RateLimited,

    // LLM errors
    LLMProviderError,
    TokenLimitExceeded,
    ContextTooLong,

    // Config errors
    ConfigInvalid,
    ConfigMissing,
    ConfigParseFailed,

    // Security errors
    PermissionDenied,
    SandboxViolation,
    DotfileProtection,

    // System errors
    IoError,
    OutOfMemory,
    ResourceUnavailable,

    // Internal errors
    Unexpected,
    NotImplemented,
}

impl VtCodeError {
    /// Create a new error with the given category, code, and message.
    pub fn new<S: Into<String>>(category: ErrorCategory, code: ErrorCode, message: S) -> Self {
        Self {
            category,
            code,
            message: message.into(),
            context: None,
            source: None,
        }
    }

    /// Add context to the error.
    pub fn with_context<S: Into<String>>(mut self, context: S) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Set the source error for error chaining.
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Returns whether the error can be retried safely.
    pub const fn is_retryable(&self) -> bool {
        self.category.is_retryable()
    }

    /// Returns the retry strategy for this error category.
    pub fn retryability(&self) -> Retryability {
        self.category.retryability()
    }

    /// Convenience method for input errors.
    pub fn input<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::InvalidParameters, code, message)
    }

    /// Convenience method for execution errors.
    pub fn execution<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::ExecutionError, code, message)
    }

    /// Convenience method for network errors.
    pub fn network<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::Network, code, message)
    }

    /// Convenience method for LLM errors.
    pub fn llm<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::ExecutionError, code, message)
    }

    /// Convenience method for config errors.
    pub fn config<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::InvalidParameters, code, message)
    }

    /// Convenience method for security errors.
    pub fn security<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::PolicyViolation, code, message)
    }

    /// Convenience method for system errors.
    pub fn system<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::ExecutionError, code, message)
    }

    /// Convenience method for internal errors.
    pub fn internal<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self::new(ErrorCategory::ExecutionError, code, message)
    }
}

// Implement conversions from common error types
impl From<std::io::Error> for VtCodeError {
    fn from(err: std::io::Error) -> Self {
        VtCodeError::system(ErrorCode::IoError, err.to_string()).with_source(err)
    }
}

impl From<serde_json::Error> for VtCodeError {
    fn from(err: serde_json::Error) -> Self {
        VtCodeError::config(ErrorCode::ConfigParseFailed, err.to_string()).with_source(err)
    }
}

impl From<reqwest::Error> for VtCodeError {
    fn from(err: reqwest::Error) -> Self {
        let code = if err.is_timeout() {
            ErrorCode::Timeout
        } else if err.is_connect() {
            ErrorCode::ConnectionFailed
        } else {
            ErrorCode::RequestFailed
        };
        VtCodeError::network(code, err.to_string()).with_source(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = VtCodeError::input(ErrorCode::InvalidArgument, "Invalid argument");
        assert_eq!(err.category, ErrorCategory::InvalidParameters);
        assert_eq!(err.code, ErrorCode::InvalidArgument);
        assert_eq!(err.message, "Invalid argument");
    }

    #[test]
    fn test_error_with_context() {
        let err = VtCodeError::input(ErrorCode::InvalidArgument, "Invalid argument")
            .with_context("While parsing user input");
        assert_eq!(err.context, Some("While parsing user input".to_string()));
    }

    #[test]
    fn test_error_with_source() {
        let io_err = std::io::Error::other("IO error");
        let err =
            VtCodeError::system(ErrorCode::IoError, "File operation failed").with_source(io_err);
        assert!(err.source.is_some());
    }

    #[test]
    fn test_error_category_display() {
        let err = VtCodeError::network(ErrorCode::ConnectionFailed, "Connection failed");
        let display = format!("{}", err);
        assert!(display.contains("Network error"));
        assert!(display.contains("Connection failed"));
    }

    #[test]
    fn test_error_serialization_skips_source() {
        let io_err = std::io::Error::other("IO error");
        let err = VtCodeError::system(ErrorCode::IoError, "File operation failed")
            .with_context("While reading config")
            .with_source(io_err);

        let json = serde_json::to_string(&err).expect("vtcode error should serialize");
        assert!(json.contains("\"message\":\"File operation failed\""));
        assert!(json.contains("\"context\":\"While reading config\""));
        assert!(!json.contains("source"));
    }
}
