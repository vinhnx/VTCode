//! Structured error handling for VT Code.
//!
//! Provides a VT Code-specific error envelope with machine-readable codes and
//! contextual information while reusing the shared `vtcode_commons`
//! classification system.

use crate::llm::provider::LLMError;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use crate::tools::unified_error::{UnifiedErrorKind, UnifiedToolError};
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

    /// Optional backoff hint for the next retry attempt in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,

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
    ServiceUnavailable,

    // LLM errors
    AuthenticationFailed,
    LLMProviderError,
    TokenLimitExceeded,
    ContextTooLong,

    // Config errors
    ConfigInvalid,
    ConfigMissing,
    ConfigParseFailed,

    // Security errors
    PermissionDenied,
    PolicyViolation,
    PlanModeViolation,
    SandboxViolation,
    DotfileProtection,

    // System errors
    IoError,
    OutOfMemory,
    ResourceUnavailable,
    ResourceNotFound,

    // Internal errors
    ToolNotFound,
    CircuitOpen,
    Cancelled,
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
            retry_after_ms: None,
            source: None,
        }
    }

    /// Add context to the error.
    pub fn with_context<S: Into<String>>(mut self, context: S) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Add a retry-after hint to the error.
    pub fn with_retry_after(mut self, retry_after: std::time::Duration) -> Self {
        self.retry_after_ms = Some(retry_after.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    /// Set the source error for error chaining.
    pub fn with_source<E: std::error::Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Returns the retry-after hint as a duration when present.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        self.retry_after_ms.map(std::time::Duration::from_millis)
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

    /// Create an error from a canonical category using the default error code.
    pub fn from_category<S: Into<String>>(category: ErrorCategory, message: S) -> Self {
        Self::new(category, ErrorCode::from_category(category), message)
    }
}

impl ErrorCode {
    /// Map a canonical error category to a default machine-readable code.
    pub const fn from_category(category: ErrorCategory) -> Self {
        match category {
            ErrorCategory::Network => ErrorCode::ConnectionFailed,
            ErrorCategory::Timeout => ErrorCode::Timeout,
            ErrorCategory::RateLimit => ErrorCode::RateLimited,
            ErrorCategory::ServiceUnavailable => ErrorCode::ServiceUnavailable,
            ErrorCategory::CircuitOpen => ErrorCode::CircuitOpen,
            ErrorCategory::Authentication => ErrorCode::AuthenticationFailed,
            ErrorCategory::InvalidParameters => ErrorCode::InvalidArgument,
            ErrorCategory::ToolNotFound => ErrorCode::ToolNotFound,
            ErrorCategory::ResourceNotFound => ErrorCode::ResourceNotFound,
            ErrorCategory::PermissionDenied => ErrorCode::PermissionDenied,
            ErrorCategory::PolicyViolation => ErrorCode::PolicyViolation,
            ErrorCategory::PlanModeViolation => ErrorCode::PlanModeViolation,
            ErrorCategory::SandboxFailure => ErrorCode::SandboxViolation,
            ErrorCategory::ResourceExhausted => ErrorCode::ResourceUnavailable,
            ErrorCategory::Cancelled => ErrorCode::Cancelled,
            ErrorCategory::ExecutionError => ErrorCode::Unexpected,
        }
    }

    fn from_unified_kind(kind: UnifiedErrorKind) -> Self {
        match kind {
            UnifiedErrorKind::Timeout => ErrorCode::Timeout,
            UnifiedErrorKind::Network => ErrorCode::ConnectionFailed,
            UnifiedErrorKind::RateLimit => ErrorCode::RateLimited,
            UnifiedErrorKind::ArgumentValidation => ErrorCode::ValidationFailed,
            UnifiedErrorKind::ToolNotFound => ErrorCode::ToolNotFound,
            UnifiedErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            UnifiedErrorKind::SandboxFailure => ErrorCode::SandboxViolation,
            UnifiedErrorKind::InternalError => ErrorCode::Unexpected,
            UnifiedErrorKind::CircuitOpen => ErrorCode::CircuitOpen,
            UnifiedErrorKind::ResourceExhausted => ErrorCode::ResourceUnavailable,
            UnifiedErrorKind::Cancelled => ErrorCode::Cancelled,
            UnifiedErrorKind::PolicyViolation => ErrorCode::PolicyViolation,
            UnifiedErrorKind::PlanModeViolation => ErrorCode::PlanModeViolation,
            UnifiedErrorKind::ExecutionFailed | UnifiedErrorKind::Unknown => {
                ErrorCode::ToolExecutionFailed
            }
        }
    }

    fn from_tool_error_type(error_type: ToolErrorType) -> Self {
        match error_type {
            ToolErrorType::InvalidParameters => ErrorCode::ValidationFailed,
            ToolErrorType::ToolNotFound => ErrorCode::ToolNotFound,
            ToolErrorType::PermissionDenied => ErrorCode::PermissionDenied,
            ToolErrorType::ResourceNotFound => ErrorCode::ResourceNotFound,
            ToolErrorType::NetworkError => ErrorCode::ConnectionFailed,
            ToolErrorType::Timeout => ErrorCode::Timeout,
            ToolErrorType::ExecutionError => ErrorCode::ToolExecutionFailed,
            ToolErrorType::PolicyViolation => ErrorCode::PolicyViolation,
        }
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

impl From<anyhow::Error> for VtCodeError {
    fn from(err: anyhow::Error) -> Self {
        let category = vtcode_commons::classify_anyhow_error(&err);
        VtCodeError::new(
            category,
            ErrorCode::from_category(category),
            err.to_string(),
        )
        .with_context(format!("{err:#}"))
    }
}

impl From<LLMError> for VtCodeError {
    fn from(err: LLMError) -> Self {
        let category = ErrorCategory::from(&err);
        let code = match &err {
            LLMError::Authentication { .. } => ErrorCode::AuthenticationFailed,
            LLMError::RateLimit { .. } => {
                if category == ErrorCategory::ResourceExhausted {
                    ErrorCode::from_category(category)
                } else {
                    ErrorCode::RateLimited
                }
            }
            LLMError::InvalidRequest { .. } => ErrorCode::ValidationFailed,
            LLMError::Network { message, .. } => {
                if vtcode_commons::classify_error_message(message) == ErrorCategory::Timeout {
                    ErrorCode::Timeout
                } else {
                    ErrorCode::ConnectionFailed
                }
            }
            LLMError::Provider { metadata, .. } => {
                if category == ErrorCategory::ResourceExhausted {
                    ErrorCode::from_category(category)
                } else {
                    metadata
                        .as_ref()
                        .and_then(|meta| meta.status)
                        .map(|status| match status {
                            408 => ErrorCode::Timeout,
                            429 => ErrorCode::RateLimited,
                            500 | 502 | 503 | 504 | 529 => ErrorCode::ServiceUnavailable,
                            _ => ErrorCode::LLMProviderError,
                        })
                        .unwrap_or(ErrorCode::LLMProviderError)
                }
            }
        };
        let message = llm_error_message(&err);
        let retry_after = llm_retry_after(&err);

        let error = VtCodeError::new(category, code, message).with_source(err);
        if let Some(retry_after) = retry_after {
            error.with_retry_after(retry_after)
        } else {
            error
        }
    }
}

impl From<UnifiedToolError> for VtCodeError {
    fn from(err: UnifiedToolError) -> Self {
        let mut error = VtCodeError::new(
            ErrorCategory::from(err.kind),
            ErrorCode::from_unified_kind(err.kind),
            err.user_message.clone(),
        );

        if let Some(ctx) = &err.debug_context {
            let mut metadata = vec![
                format!("tool={}", ctx.tool_name),
                format!("attempt={}", ctx.attempt),
            ];
            if let Some(invocation_id) = &ctx.invocation_id {
                metadata.push(format!("invocation_id={invocation_id}"));
            }
            metadata.extend(
                ctx.metadata
                    .iter()
                    .map(|(key, value)| format!("{key}={value}")),
            );
            error = error.with_context(metadata.join(", "));
        }

        error.with_source(err)
    }
}

impl From<ToolExecutionError> for VtCodeError {
    fn from(err: ToolExecutionError) -> Self {
        let category = ErrorCategory::from(err.error_type);
        let mut error = VtCodeError::new(
            category,
            ErrorCode::from_tool_error_type(err.error_type),
            err.message.clone(),
        );

        let mut context_parts = Vec::new();
        if let Some(original_error) = &err.original_error {
            context_parts.push(format!("original_error={original_error}"));
        }
        if !err.recovery_suggestions.is_empty() {
            context_parts.push(format!(
                "recovery_suggestions={}",
                err.recovery_suggestions.join(" | ")
            ));
        }
        if !context_parts.is_empty() {
            error = error.with_context(context_parts.join(", "));
        }

        error
    }
}

fn llm_error_message(error: &LLMError) -> String {
    match error {
        LLMError::Authentication { message, .. }
        | LLMError::InvalidRequest { message, .. }
        | LLMError::Network { message, .. }
        | LLMError::Provider { message, .. } => message.clone(),
        LLMError::RateLimit { metadata } => metadata
            .as_ref()
            .and_then(|meta| meta.message.clone())
            .unwrap_or_else(|| "rate limit exceeded".to_string()),
    }
}

fn llm_retry_after(error: &LLMError) -> Option<std::time::Duration> {
    let metadata = match error {
        LLMError::Authentication { metadata, .. }
        | LLMError::RateLimit { metadata }
        | LLMError::InvalidRequest { metadata, .. }
        | LLMError::Network { metadata, .. }
        | LLMError::Provider { metadata, .. } => metadata.as_ref(),
    }?;

    metadata
        .retry_after
        .as_deref()
        .and_then(parse_retry_after_header)
}

fn parse_retry_after_header(raw: &str) -> Option<std::time::Duration> {
    raw.trim()
        .parse::<u64>()
        .ok()
        .map(std::time::Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::LLMErrorMetadata;
    use crate::tools::unified_error::DebugContext;

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

    #[test]
    fn test_error_with_retry_after() {
        let err = VtCodeError::network(ErrorCode::RateLimited, "rate limit")
            .with_retry_after(std::time::Duration::from_secs(2));
        assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(2)));
    }

    #[test]
    fn test_llm_error_conversion_preserves_retry_after() {
        let err = LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "OpenAI",
                Some(429),
                Some("rate_limit".to_string()),
                Some("req-1".to_string()),
                None,
                Some("3".to_string()),
                Some("try again later".to_string()),
            )),
        };

        let converted = VtCodeError::from(err);
        assert_eq!(converted.category, ErrorCategory::RateLimit);
        assert_eq!(converted.code, ErrorCode::RateLimited);
        assert_eq!(
            converted.retry_after(),
            Some(std::time::Duration::from_secs(3))
        );
    }

    #[test]
    fn test_llm_quota_exhaustion_uses_resource_exhausted_code() {
        let err = LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "OpenAI",
                Some(429),
                Some("insufficient_quota".to_string()),
                None,
                None,
                None,
                Some("quota exceeded".to_string()),
            )),
        };

        let converted = VtCodeError::from(err);
        assert_eq!(converted.category, ErrorCategory::ResourceExhausted);
        assert_eq!(converted.code, ErrorCode::ResourceUnavailable);
    }

    #[test]
    fn test_unified_tool_error_conversion_preserves_context() {
        let err = UnifiedToolError::new(UnifiedErrorKind::Network, "network down").with_context(
            DebugContext {
                tool_name: "read_file".to_string(),
                invocation_id: Some("inv-1".to_string()),
                attempt: 2,
                metadata: vec![("duration_ms".to_string(), "1500".to_string())],
            },
        );

        let converted = VtCodeError::from(err);
        assert_eq!(converted.category, ErrorCategory::Network);
        assert_eq!(converted.code, ErrorCode::ConnectionFailed);
        assert!(
            converted
                .context
                .as_deref()
                .is_some_and(|ctx| ctx.contains("tool=read_file"))
        );
    }

    #[test]
    fn test_tool_execution_error_conversion_uses_original_context() {
        let err = ToolExecutionError::with_original_error(
            "unified_exec".to_string(),
            ToolErrorType::Timeout,
            "Tool execution failed".to_string(),
            "timed out waiting for process".to_string(),
        );

        let converted = VtCodeError::from(err);
        assert_eq!(converted.category, ErrorCategory::Timeout);
        assert_eq!(converted.code, ErrorCode::Timeout);
        assert!(
            converted
                .context
                .as_deref()
                .is_some_and(|ctx| ctx.contains("original_error=timed out waiting for process"))
        );
    }
}
