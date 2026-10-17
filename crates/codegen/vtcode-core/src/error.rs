//! Structured error handling for VT Code.
//!
//! Provides a VT Code-specific error envelope with machine-readable codes and
//! contextual information while reusing the shared `vtcode_commons`
//! classification system.

use crate::llm::provider::LLMError;
use crate::retry_after::retry_after_from_llm_metadata;
use crate::tools::registry::{ToolErrorType, ToolExecutionError};
use crate::tools::unified_error::{UnifiedErrorKind, UnifiedToolError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use vtcode_commons::sanitizer::sanitize_provider_diagnostic;
pub use vtcode_commons::{
    BackoffStrategy, ConfigGuidance, ErrorCategory, MisconfigurationKind, Retryability, detect_misconfiguration,
    is_misconfiguration,
};
use vtcode_macros::DebugNoInline;

/// Result type alias for VT Code operations.
pub type Result<T> = std::result::Result<T, VtCodeError>;

/// Core error type for VT Code operations.
///
/// Uses `thiserror::Error` for automatic `std::error::Error` implementation
/// and provides clear error messages with context.
///
/// `Debug` is derived via [`DebugNoInline`] rather than `#[derive(Debug)]`: the
/// envelope wraps an arbitrary `source` chain and is formatted on fan-out failure
/// paths, so the built-in derive's implied `#[inline]` would inline the whole
/// chain into every `{:?}` / `?err` site.
#[derive(DebugNoInline, Error, Serialize, Deserialize)]
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
    PlanningPolicyViolation,
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

    /// Check settings/config first: return guidance when this failure is
    /// caused by user misconfiguration.
    ///
    /// Config error codes (`ConfigInvalid`/`ConfigMissing`/`ConfigParseFailed`)
    /// are always misconfiguration. Otherwise the message plus context is
    /// matched against config markers (credentials, model, provider,
    /// `base_url`, `vtcode.toml`, MCP, sampling ranges). Transient failures
    /// and LLM argument mistakes return `None`.
    #[must_use]
    pub fn misconfiguration_guidance(&self) -> Option<ConfigGuidance> {
        if matches!(self.code, ErrorCode::ConfigInvalid | ErrorCode::ConfigMissing | ErrorCode::ConfigParseFailed) {
            if let Some(guidance) = self.message_guidance() {
                return Some(guidance);
            }
            return Some(ConfigGuidance {
                kind: MisconfigurationKind::ConfigFile,
                setting: "vtcode.toml",
                location: "workspace / user / system config layers",
                fix: std::borrow::Cow::Borrowed(
                    "Invalid config file. Validate vtcode.toml syntax and fields, then retry.",
                ),
            });
        }
        self.message_guidance()
    }

    /// Whether this failure is user misconfiguration that must be fixed
    /// before retrying.
    #[must_use]
    pub fn is_misconfiguration(&self) -> bool {
        self.misconfiguration_guidance().is_some()
    }

    /// Attach config guidance to the error so it is visible in `Display`
    /// (`{category}: {message}`) and does not get retried blindly.
    /// Idempotent: does nothing when guidance is absent or already present.
    #[must_use]
    pub fn with_misconfiguration_guidance(mut self) -> Self {
        let Some(guidance) = self.misconfiguration_guidance() else {
            return self;
        };
        let suffix = guidance.user_message();
        // Use the full terminal phrase as the idempotency marker; it is far
        // less likely to collide with user content than the shorter prefix.
        if self.message.contains("Correct the configuration before retrying.") {
            return self;
        }
        // `context` is not part of thiserror's Display output, so keep the
        // guidance in the visible message even when the provider returned a
        // large diagnostic payload.
        self.message.push(' ');
        self.message.push_str(&suffix);
        self
    }

    fn message_guidance(&self) -> Option<ConfigGuidance> {
        let mut haystack = self.message.clone();
        if let Some(context) = self.context.as_deref() {
            haystack.push('\n');
            haystack.push_str(context);
        }
        let mut source = self.source.as_deref().map(|source| source as &dyn std::error::Error);
        while let Some(error) = source {
            haystack.push('\n');
            haystack.push_str(&error.to_string());
            source = error.source();
        }
        detect_misconfiguration(self.category, &haystack)
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
            ErrorCategory::PlanningPolicyViolation => ErrorCode::PlanningPolicyViolation,
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
            UnifiedErrorKind::PlanningPolicyViolation => ErrorCode::PlanningPolicyViolation,
            UnifiedErrorKind::ExecutionFailed | UnifiedErrorKind::Unknown => ErrorCode::ToolExecutionFailed,
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
        VtCodeError::system(ErrorCode::IoError, err.to_string())
            .with_source(err)
            .with_misconfiguration_guidance()
    }
}

impl From<serde_json::Error> for VtCodeError {
    fn from(err: serde_json::Error) -> Self {
        VtCodeError::config(ErrorCode::ConfigParseFailed, err.to_string())
            .with_source(err)
            .with_misconfiguration_guidance()
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
        VtCodeError::network(code, err.to_string())
            .with_source(err)
            .with_misconfiguration_guidance()
    }
}

impl From<anyhow::Error> for VtCodeError {
    fn from(err: anyhow::Error) -> Self {
        let category = vtcode_commons::classify_anyhow_error(&err);
        VtCodeError::new(category, ErrorCode::from_category(category), err.to_string())
            .with_context(format!("{err:#}"))
            .with_misconfiguration_guidance()
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
        let metadata_context = llm_metadata_context(&err);
        let retry_after = llm_retry_after(&err);

        let mut error = VtCodeError::new(category, code, message);
        if let Some(context) = metadata_context {
            error = error.with_context(context);
        }
        let error = error.with_source(err);
        let error = if let Some(retry_after) = retry_after {
            error.with_retry_after(retry_after)
        } else {
            error
        };
        error.with_misconfiguration_guidance()
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
            let mut metadata = vec![format!("tool={}", ctx.tool_name), format!("attempt={}", ctx.attempt)];
            if let Some(invocation_id) = &ctx.invocation_id {
                metadata.push(format!("invocation_id={invocation_id}"));
            }
            metadata.extend(ctx.metadata.iter().map(|(key, value)| format!("{key}={value}")));
            error = error.with_context(metadata.join(", "));
        }

        error.with_source(err).with_misconfiguration_guidance()
    }
}

impl From<ToolExecutionError> for VtCodeError {
    fn from(err: ToolExecutionError) -> Self {
        let category = ErrorCategory::from(err.error_type);
        let mut error =
            VtCodeError::new(category, ErrorCode::from_tool_error_type(err.error_type), err.message.clone());

        let mut context_parts = Vec::new();
        if let Some(original_error) = &err.original_error {
            context_parts.push(format!("original_error={original_error}"));
        }
        if !err.recovery_suggestions.is_empty() {
            context_parts.push(format!("recovery_suggestions={}", err.recovery_suggestions.join(" | ")));
        }
        if !context_parts.is_empty() {
            error = error.with_context(context_parts.join(", "));
        }

        error.with_misconfiguration_guidance()
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

fn llm_metadata_context(error: &LLMError) -> Option<String> {
    let metadata = error.metadata()?;

    let mut context = Vec::new();
    if let Some(code) = metadata.code.as_deref() {
        context.push(format!("provider_code={}", sanitize_provider_diagnostic(code.as_bytes())));
    }
    if let Some(message) = metadata.message.as_deref() {
        context.push(format!("provider_message={}", sanitize_provider_diagnostic(message.as_bytes())));
    }
    (!context.is_empty()).then(|| context.join(", "))
}

fn llm_retry_after(error: &LLMError) -> Option<std::time::Duration> {
    let metadata = error.metadata()?;

    retry_after_from_llm_metadata(metadata)
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
        let err =
            VtCodeError::input(ErrorCode::InvalidArgument, "Invalid argument").with_context("While parsing user input");
        assert_eq!(err.context, Some("While parsing user input".to_string()));
    }

    #[test]
    fn test_error_with_source() {
        let io_err = std::io::Error::other("IO error");
        let err = VtCodeError::system(ErrorCode::IoError, "File operation failed").with_source(io_err);
        assert!(err.source.is_some());
    }

    #[test]
    fn test_error_category_display() {
        let err = VtCodeError::network(ErrorCode::ConnectionFailed, "Connection failed");
        let display = format!("{err}");
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
        assert_eq!(converted.retry_after(), Some(std::time::Duration::from_secs(3)));
    }

    #[test]
    fn test_llm_metadata_marks_converted_error_as_misconfiguration() {
        let err = LLMError::Provider {
            message: "provider request failed".to_string(),
            metadata: Some(LLMErrorMetadata::new(
                "OpenAI",
                Some(404),
                Some("model_not_found".to_string()),
                None,
                None,
                None,
                Some("The requested model does not exist".to_string()),
            )),
        };

        let converted = VtCodeError::from(err);
        assert!(converted.is_misconfiguration());
        assert!(converted.message.contains("Check settings/config first"));
        assert!(
            converted
                .context
                .as_deref()
                .is_some_and(|context| context.contains("provider_code=model_not_found"))
        );
    }

    #[test]
    fn test_llm_error_conversion_preserves_fractional_retry_after() {
        let err = LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "OpenAI",
                Some(429),
                Some("rate_limit".to_string()),
                Some("req-1".to_string()),
                None,
                Some("0.5".to_string()),
                Some("try again later".to_string()),
            )),
        };

        let converted = VtCodeError::from(err);
        assert_eq!(converted.retry_after(), Some(std::time::Duration::from_millis(500)));
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
        let err = UnifiedToolError::new(UnifiedErrorKind::Network, "network down").with_context(DebugContext {
            tool_name: "read_file".to_string(),
            invocation_id: Some("inv-1".to_string()),
            attempt: 2,
            metadata: vec![("duration_ms".to_string(), "1500".to_string())],
        });

        let converted = VtCodeError::from(err);
        assert_eq!(converted.category, ErrorCategory::Network);
        assert_eq!(converted.code, ErrorCode::ConnectionFailed);
        assert!(converted.context.as_deref().is_some_and(|ctx| ctx.contains("tool=read_file")));
    }

    #[test]
    fn test_unified_tool_source_marks_converted_error_as_misconfiguration() {
        let err = UnifiedToolError::new(UnifiedErrorKind::Network, "provider request failed")
            .with_source(anyhow::anyhow!("unknown model 'gpt-99' in agent.model"));

        let converted = VtCodeError::from(err);
        assert!(converted.is_misconfiguration());
        assert!(converted.message.contains("Check settings/config first"));
    }

    #[test]
    fn test_tool_execution_error_conversion_uses_original_context() {
        let err = ToolExecutionError::with_original_error(
            "command_session".to_string(),
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

    #[test]
    fn test_misconfiguration_guidance_for_auth() {
        let err = VtCodeError::new(
            ErrorCategory::Authentication,
            ErrorCode::AuthenticationFailed,
            "Authentication failed: invalid api key",
        );
        assert!(err.is_misconfiguration());
        let guided = err.with_misconfiguration_guidance();
        assert!(guided.message.contains("Check settings/config first"));
        assert!(guided.message.contains("before retrying"));
    }

    #[test]
    fn test_misconfiguration_guidance_for_config_code() {
        let err = VtCodeError::config(ErrorCode::ConfigInvalid, "custom_providers[x]: `base_url` must not be empty");
        assert!(err.is_misconfiguration());
    }

    #[test]
    fn test_transient_has_no_misconfiguration() {
        let err = VtCodeError::network(ErrorCode::ConnectionFailed, "connection reset by peer");
        assert!(!err.is_misconfiguration());
        let guided = err.with_misconfiguration_guidance();
        assert!(!guided.message.contains("Check settings/config first"));
    }

    #[test]
    fn test_misconfiguration_is_idempotent() {
        let err = VtCodeError::new(ErrorCategory::Authentication, ErrorCode::AuthenticationFailed, "bad key")
            .with_misconfiguration_guidance()
            .with_misconfiguration_guidance();
        assert_eq!(err.message.matches("Correct the configuration before retrying.").count(), 1);
    }

    #[test]
    fn test_misconfiguration_guidance_does_not_echo_secrets() {
        let secret = concat!("sk-", "test1234567890abcdef");
        let err = VtCodeError::new(
            ErrorCategory::Authentication,
            ErrorCode::AuthenticationFailed,
            format!("Authentication failed: invalid api key {secret}"),
        );
        let guidance = err.misconfiguration_guidance().expect("auth must match");
        assert!(!guidance.user_message().contains(secret));
    }

    #[test]
    fn test_misconfiguration_guidance_stays_visible_for_large_errors() {
        let mut message = "x".repeat(8 * 1024);
        message.push_str(" invalid api key");
        let guided = VtCodeError::execution(ErrorCode::Unexpected, message).with_misconfiguration_guidance();
        assert!(guided.message.contains("Check settings/config first"));
        assert!(guided.to_string().contains("Correct the configuration before retrying."));
    }
}
