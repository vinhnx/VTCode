//! Unified tool error envelope for consistent error handling across execution paths
//!
//! This module consolidates error types from:
//! - `handlers::ToolCallError`
//! - `handlers::ToolError`
//! - `middleware::MiddlewareError`
//! - `improvements_errors::ImprovementError`
//! - Registry execution errors
//!
//! By routing all errors through this envelope, we achieve:
//! - Consistent retry classification
//! - Uniform user-facing messaging
//! - Preserved debug context for diagnostics

use std::fmt;
use thiserror::Error;
use vtcode_commons::ErrorCategory;

/// Unified error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// Transient error, safe to retry
    Transient,
    /// Permanent error, do not retry
    Permanent,
    /// User intervention required (HITL)
    RequiresApproval,
    /// Tool blocked by policy
    PolicyBlocked,
}

/// Unified error kind for classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnifiedErrorKind {
    /// Network or I/O timeout
    Timeout,
    /// Network connectivity issue
    Network,
    /// Rate limit exceeded
    RateLimit,
    /// Invalid arguments from LLM
    ArgumentValidation,
    /// Tool not found or unavailable
    ToolNotFound,
    /// Permission denied by policy
    PermissionDenied,
    /// Sandbox execution failed or denied
    SandboxFailure,
    /// Internal tool error
    InternalError,
    /// Circuit breaker open
    CircuitOpen,
    /// Resource exhausted (memory, disk, etc.)
    ResourceExhausted,
    /// User cancelled operation
    Cancelled,
    /// Policy violation (blocked by safety gateway)
    PolicyViolation,
    /// Planning workflow violation (mutating tool without write permissions)
    PlanningPolicyViolation,
    /// Execution failed (general tool execution failure)
    ExecutionFailed,
    /// Unknown/unclassified error
    Unknown,
}

impl UnifiedErrorKind {
    /// Whether this error kind is retryable.
    ///
    /// Delegates to [`ErrorCategory::is_retryable`] so the retry set has a
    /// single source of truth in `vtcode-commons`.
    #[inline]
    pub fn is_retryable(&self) -> bool {
        ErrorCategory::from(*self).is_retryable()
    }

    /// Whether this is an LLM mistake (argument error) vs tool failure
    #[inline]
    pub const fn is_llm_mistake(&self) -> bool {
        matches!(self, UnifiedErrorKind::ArgumentValidation)
    }
}

/// Unified tool error envelope
#[derive(Error, Debug)]
pub struct UnifiedToolError {
    /// Error classification
    pub kind: UnifiedErrorKind,
    /// Severity level
    pub severity: ErrorSeverity,
    /// User-facing message (safe to display)
    pub user_message: String,
    /// Debug context (tool name, args, etc.)
    pub debug_context: Option<DebugContext>,
    /// Original error (for chaining)
    #[source]
    pub source: Option<anyhow::Error>,
}

impl fmt::Display for UnifiedToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.user_message)
    }
}

/// Debug context for error diagnostics
#[derive(Debug, Clone)]
pub struct DebugContext {
    /// Tool that produced the error
    pub tool_name: String,
    /// Invocation ID for correlation
    pub invocation_id: Option<String>,
    /// Attempt number (for retries)
    pub attempt: u32,
    /// Additional context key-value pairs
    pub metadata: Vec<(String, String)>,
}

impl UnifiedToolError {
    /// Ensure debug context exists and return a mutable reference.
    fn debug_context_mut(&mut self) -> &mut DebugContext {
        self.debug_context.get_or_insert_with(|| DebugContext {
            tool_name: String::new(),
            invocation_id: None,
            attempt: 1,
            metadata: Vec::new(),
        })
    }

    /// Create a new unified error
    #[must_use]
    pub fn new(kind: UnifiedErrorKind, user_message: impl Into<String>) -> Self {
        let severity = match kind {
            UnifiedErrorKind::Timeout
            | UnifiedErrorKind::Network
            | UnifiedErrorKind::RateLimit
            | UnifiedErrorKind::CircuitOpen => ErrorSeverity::Transient,
            UnifiedErrorKind::PermissionDenied => ErrorSeverity::RequiresApproval,
            _ => ErrorSeverity::Permanent,
        };

        Self {
            kind,
            severity,
            user_message: user_message.into(),
            debug_context: None,
            source: None,
        }
    }

    /// Add debug context
    #[must_use]
    pub fn with_context(mut self, ctx: DebugContext) -> Self {
        self.debug_context = Some(ctx);
        self
    }

    /// Add source error
    #[must_use]
    pub fn with_source(mut self, err: anyhow::Error) -> Self {
        self.source = Some(err);
        self
    }

    /// Add tool name to debug context
    #[must_use]
    pub fn with_tool_name(mut self, name: &str) -> Self {
        self.debug_context_mut().tool_name = name.to_string();
        self
    }

    /// Add invocation ID to debug context
    #[must_use]
    pub fn with_invocation_id(mut self, id: crate::tools::invocation::ToolInvocationId) -> Self {
        self.debug_context_mut().invocation_id = Some(id.to_string());
        self
    }

    /// Add duration metadata
    #[must_use]
    pub fn with_duration(mut self, duration: std::time::Duration) -> Self {
        self.debug_context_mut()
            .metadata
            .push(("duration_ms".to_string(), duration.as_millis().to_string()));
        self
    }

    /// Check if error is retryable
    #[inline]
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        self.kind.is_retryable() && matches!(self.severity, ErrorSeverity::Transient)
    }

    /// Check if this is an LLM argument error (should not count toward circuit breaker)
    #[inline]
    #[must_use]
    pub fn is_llm_mistake(&self) -> bool {
        self.kind.is_llm_mistake()
    }

    /// Return the canonical VT Code error category for this tool error.
    #[inline]
    #[must_use]
    pub fn category(&self) -> ErrorCategory {
        ErrorCategory::from(self.kind)
    }
}

// === Bridge conversions between ErrorCategory and UnifiedErrorKind ===
//
// Classification of raw errors happens once, in
// `vtcode_commons::classify_anyhow_error`; these impls only translate the
// canonical `ErrorCategory` into this crate's envelope view and back.

/// Lossy: `ServiceUnavailable` collapses into `Network` and `ResourceNotFound`
/// collapses into `ToolNotFound`, because `UnifiedErrorKind` has no dedicated
/// variants for them. Round-tripping through `ErrorCategory` does not restore
/// the original category for those inputs.
impl From<ErrorCategory> for UnifiedErrorKind {
    fn from(cat: ErrorCategory) -> Self {
        match cat {
            ErrorCategory::Network | ErrorCategory::ServiceUnavailable => UnifiedErrorKind::Network,
            ErrorCategory::Timeout => UnifiedErrorKind::Timeout,
            ErrorCategory::RateLimit => UnifiedErrorKind::RateLimit,
            ErrorCategory::CircuitOpen => UnifiedErrorKind::CircuitOpen,
            ErrorCategory::Authentication => UnifiedErrorKind::PermissionDenied,
            ErrorCategory::InvalidParameters => UnifiedErrorKind::ArgumentValidation,
            ErrorCategory::ToolNotFound => UnifiedErrorKind::ToolNotFound,
            ErrorCategory::ResourceNotFound => UnifiedErrorKind::ToolNotFound,
            ErrorCategory::PermissionDenied => UnifiedErrorKind::PermissionDenied,
            ErrorCategory::PolicyViolation => UnifiedErrorKind::PolicyViolation,
            ErrorCategory::PlanningPolicyViolation => UnifiedErrorKind::PlanningPolicyViolation,
            ErrorCategory::SandboxFailure => UnifiedErrorKind::SandboxFailure,
            ErrorCategory::ResourceExhausted => UnifiedErrorKind::ResourceExhausted,
            ErrorCategory::Cancelled => UnifiedErrorKind::Cancelled,
            ErrorCategory::ExecutionError => UnifiedErrorKind::ExecutionFailed,
        }
    }
}

/// Lossy: `InternalError`, `ExecutionFailed`, and `Unknown` all collapse into
/// `ExecutionError`, so distinct kinds can map to the same category.
impl From<UnifiedErrorKind> for ErrorCategory {
    fn from(kind: UnifiedErrorKind) -> Self {
        match kind {
            UnifiedErrorKind::Timeout => ErrorCategory::Timeout,
            UnifiedErrorKind::Network => ErrorCategory::Network,
            UnifiedErrorKind::RateLimit => ErrorCategory::RateLimit,
            UnifiedErrorKind::ArgumentValidation => ErrorCategory::InvalidParameters,
            UnifiedErrorKind::ToolNotFound => ErrorCategory::ToolNotFound,
            UnifiedErrorKind::PermissionDenied => ErrorCategory::PermissionDenied,
            UnifiedErrorKind::SandboxFailure => ErrorCategory::SandboxFailure,
            UnifiedErrorKind::InternalError => ErrorCategory::ExecutionError,
            UnifiedErrorKind::CircuitOpen => ErrorCategory::CircuitOpen,
            UnifiedErrorKind::ResourceExhausted => ErrorCategory::ResourceExhausted,
            UnifiedErrorKind::Cancelled => ErrorCategory::Cancelled,
            UnifiedErrorKind::PolicyViolation => ErrorCategory::PolicyViolation,
            UnifiedErrorKind::PlanningPolicyViolation => ErrorCategory::PlanningPolicyViolation,
            UnifiedErrorKind::ExecutionFailed => ErrorCategory::ExecutionError,
            UnifiedErrorKind::Unknown => ErrorCategory::ExecutionError,
        }
    }
}

/// Convert from handlers::ToolCallError
impl From<crate::tools::handlers::ToolCallError> for UnifiedToolError {
    fn from(err: crate::tools::handlers::ToolCallError) -> Self {
        use crate::tools::handlers::ToolCallError;
        match err {
            ToolCallError::Rejected(msg) => UnifiedToolError::new(UnifiedErrorKind::PermissionDenied, msg),
            ToolCallError::RespondToModel(msg) => UnifiedToolError::new(UnifiedErrorKind::InternalError, msg),
            ToolCallError::Internal(e) => {
                let kind = UnifiedErrorKind::from(vtcode_commons::classify_anyhow_error(&e));
                UnifiedToolError::new(kind, e.to_string()).with_source(e)
            }
            ToolCallError::Timeout(ms) => {
                UnifiedToolError::new(UnifiedErrorKind::Timeout, format!("Timeout after {ms}ms"))
            }
        }
    }
}

/// Convert from handlers::sandboxing::ToolError
impl From<crate::tools::handlers::sandboxing::ToolError> for UnifiedToolError {
    fn from(err: crate::tools::handlers::sandboxing::ToolError) -> Self {
        use crate::tools::handlers::sandboxing::ToolError;
        match err {
            ToolError::Rejected(msg) => UnifiedToolError::new(UnifiedErrorKind::PermissionDenied, msg),
            ToolError::Codex(e) => {
                let kind = UnifiedErrorKind::from(vtcode_commons::classify_anyhow_error(&e));
                UnifiedToolError::new(kind, e.to_string()).with_source(e)
            }
            ToolError::SandboxDenied(msg) => UnifiedToolError::new(UnifiedErrorKind::SandboxFailure, msg),
            ToolError::Timeout(ms) => UnifiedToolError::new(UnifiedErrorKind::Timeout, format!("Timeout after {ms}ms")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(err: &anyhow::Error) -> UnifiedErrorKind {
        UnifiedErrorKind::from(vtcode_commons::classify_anyhow_error(err))
    }

    #[test]
    fn test_error_classification() {
        assert_eq!(classify(&anyhow::anyhow!("Connection timeout")), UnifiedErrorKind::Timeout);
        assert_eq!(classify(&anyhow::anyhow!("Rate limit exceeded")), UnifiedErrorKind::RateLimit);
        assert_eq!(classify(&anyhow::anyhow!("Permission denied")), UnifiedErrorKind::PermissionDenied);
        assert_eq!(classify(&anyhow::anyhow!("Invalid argument: missing path")), UnifiedErrorKind::ArgumentValidation);
    }

    #[test]
    fn test_retryable_parity_with_error_category() {
        let kinds = [
            UnifiedErrorKind::Timeout,
            UnifiedErrorKind::Network,
            UnifiedErrorKind::RateLimit,
            UnifiedErrorKind::ArgumentValidation,
            UnifiedErrorKind::ToolNotFound,
            UnifiedErrorKind::PermissionDenied,
            UnifiedErrorKind::SandboxFailure,
            UnifiedErrorKind::InternalError,
            UnifiedErrorKind::CircuitOpen,
            UnifiedErrorKind::ResourceExhausted,
            UnifiedErrorKind::Cancelled,
            UnifiedErrorKind::PolicyViolation,
            UnifiedErrorKind::PlanningPolicyViolation,
            UnifiedErrorKind::ExecutionFailed,
            UnifiedErrorKind::Unknown,
        ];
        for kind in kinds {
            let expected = matches!(
                kind,
                UnifiedErrorKind::Timeout
                    | UnifiedErrorKind::Network
                    | UnifiedErrorKind::RateLimit
                    | UnifiedErrorKind::CircuitOpen
            );
            assert_eq!(kind.is_retryable(), expected, "kind: {kind:?}");
        }
    }

    #[test]
    fn test_retryable_errors() {
        let timeout_err = UnifiedToolError::new(UnifiedErrorKind::Timeout, "timeout");
        assert!(timeout_err.is_retryable());

        let perm_err = UnifiedToolError::new(UnifiedErrorKind::PermissionDenied, "denied");
        assert!(!perm_err.is_retryable());
    }

    #[test]
    fn test_llm_mistake_classification() {
        let arg_err = UnifiedToolError::new(UnifiedErrorKind::ArgumentValidation, "bad args");
        assert!(arg_err.is_llm_mistake());

        let net_err = UnifiedToolError::new(UnifiedErrorKind::Network, "network down");
        assert!(!net_err.is_llm_mistake());
    }
}
