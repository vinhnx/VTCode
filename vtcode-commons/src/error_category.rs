//! Unified error categorization system for consistent error classification across VT Code.
//!
//! This module provides a single canonical `ErrorCategory` enum that unifies the
//! previously separate classification systems in `registry::error` (8-variant `ToolErrorType`)
//! and `unified_error` (16-variant `UnifiedErrorKind`). Both systems now map through
//! this shared taxonomy for consistent retry decisions and error handling.
//!
//! # Error Categories
//!
//! Errors are divided into **retryable** (transient) and **non-retryable** (permanent)
//! categories, with sub-classifications for specific handling strategies.
//!
//! # Design Decisions
//!
//! - String-based fallback is preserved only for `anyhow::Error` chains where the
//!   original type is erased. Typed `From` conversions are preferred.
//! - Policy violations are explicitly separated from OS-level permission denials.
//! - Rate limiting is a distinct category (not merged with network errors).
//! - Circuit breaker open is categorized separately for recovery flow routing.

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

/// Canonical error category used throughout VT Code for consistent
/// retry decisions, user-facing messages, and error handling strategies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ErrorCategory {
    // === Retryable (Transient) ===
    /// Network connectivity issue (connection reset, DNS failure, etc.)
    Network,
    /// Request timed out or deadline exceeded
    Timeout,
    /// Rate limit exceeded (HTTP 429, provider throttling)
    RateLimit,
    /// External service temporarily unavailable (HTTP 5xx)
    ServiceUnavailable,
    /// Circuit breaker is open for this tool/service
    CircuitOpen,

    // === Non-Retryable (Permanent) ===
    /// Authentication or authorization failure (invalid API key, expired token)
    Authentication,
    /// Invalid parameters, arguments, or schema validation failure
    InvalidParameters,
    /// Tool not found or unavailable
    ToolNotFound,
    /// Resource not found (file, directory, path does not exist)
    ResourceNotFound,
    /// OS-level permission denied (file permissions, EACCES, EPERM)
    PermissionDenied,
    /// Policy violation (workspace boundary, tool deny policy, safety gate)
    PolicyViolation,
    /// Plan mode violation (mutating tool in read-only mode)
    PlanModeViolation,
    /// Sandbox execution failure
    SandboxFailure,
    /// Resource exhausted (quota, billing, spending limit, disk, memory)
    ResourceExhausted,
    /// User cancelled the operation
    Cancelled,
    /// General execution error (catch-all for unclassified failures)
    ExecutionError,
}

/// Describes whether and how an error can be retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retryability {
    /// Error is transient and may succeed on retry.
    Retryable {
        /// Suggested maximum retry attempts.
        max_attempts: u32,
        /// Suggested backoff strategy.
        backoff: BackoffStrategy,
    },
    /// Error is permanent and should NOT be retried.
    NonRetryable,
    /// Error requires human intervention before proceeding.
    RequiresIntervention,
}

/// Backoff strategy for retryable errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackoffStrategy {
    /// Exponential backoff with base delay and maximum cap.
    Exponential { base: Duration, max: Duration },
    /// Fixed delay between retries (e.g., for rate-limited APIs with Retry-After).
    Fixed(Duration),
}

impl ErrorCategory {
    /// Whether this error category is safe to retry.
    #[inline]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorCategory::Network
                | ErrorCategory::Timeout
                | ErrorCategory::RateLimit
                | ErrorCategory::ServiceUnavailable
                | ErrorCategory::CircuitOpen
        )
    }

    /// Whether this error is an LLM argument mistake (should not count toward
    /// circuit breaker thresholds).
    #[inline]
    pub const fn is_llm_mistake(&self) -> bool {
        matches!(self, ErrorCategory::InvalidParameters)
    }

    /// Whether this error represents a permanent, non-recoverable condition.
    #[inline]
    pub const fn is_permanent(&self) -> bool {
        matches!(
            self,
            ErrorCategory::Authentication
                | ErrorCategory::PolicyViolation
                | ErrorCategory::PlanModeViolation
                | ErrorCategory::ResourceExhausted
        )
    }

    /// Get the recommended retryability for this error category.
    pub fn retryability(&self) -> Retryability {
        match self {
            ErrorCategory::Network | ErrorCategory::ServiceUnavailable => Retryability::Retryable {
                max_attempts: 3,
                backoff: BackoffStrategy::Exponential {
                    base: Duration::from_millis(500),
                    max: Duration::from_secs(10),
                },
            },
            ErrorCategory::Timeout => Retryability::Retryable {
                max_attempts: 2,
                backoff: BackoffStrategy::Exponential {
                    base: Duration::from_millis(1000),
                    max: Duration::from_secs(15),
                },
            },
            ErrorCategory::RateLimit => Retryability::Retryable {
                max_attempts: 3,
                backoff: BackoffStrategy::Exponential {
                    base: Duration::from_secs(1),
                    max: Duration::from_secs(30),
                },
            },
            ErrorCategory::CircuitOpen => Retryability::Retryable {
                max_attempts: 1,
                backoff: BackoffStrategy::Fixed(Duration::from_secs(10)),
            },
            ErrorCategory::PermissionDenied => Retryability::RequiresIntervention,
            _ => Retryability::NonRetryable,
        }
    }

    /// Get recovery suggestions for this error category.
    /// Returns static strings to avoid allocation.
    pub fn recovery_suggestions(&self) -> Vec<Cow<'static, str>> {
        match self {
            ErrorCategory::Network => vec![
                Cow::Borrowed("Check network connectivity"),
                Cow::Borrowed("Retry the operation after a brief delay"),
                Cow::Borrowed("Verify external service availability"),
            ],
            ErrorCategory::Timeout => vec![
                Cow::Borrowed("Increase timeout values if appropriate"),
                Cow::Borrowed("Break large operations into smaller chunks"),
                Cow::Borrowed("Check system resources and performance"),
            ],
            ErrorCategory::RateLimit => vec![
                Cow::Borrowed("Wait before retrying the request"),
                Cow::Borrowed("Reduce request frequency"),
                Cow::Borrowed("Check provider rate limit documentation"),
            ],
            ErrorCategory::ServiceUnavailable => vec![
                Cow::Borrowed("The service is temporarily unavailable"),
                Cow::Borrowed("Retry after a brief delay"),
                Cow::Borrowed("Check service status page if available"),
            ],
            ErrorCategory::CircuitOpen => vec![
                Cow::Borrowed("This tool has been temporarily disabled due to repeated failures"),
                Cow::Borrowed("Wait for the circuit breaker cooldown period"),
                Cow::Borrowed("Try an alternative approach"),
            ],
            ErrorCategory::Authentication => vec![
                Cow::Borrowed("Verify your API key or credentials"),
                Cow::Borrowed("Check that your account is active and has sufficient permissions"),
                Cow::Borrowed("Ensure environment variables for API keys are set correctly"),
            ],
            ErrorCategory::InvalidParameters => vec![
                Cow::Borrowed("Check parameter names and types against the tool schema"),
                Cow::Borrowed("Ensure required parameters are provided"),
                Cow::Borrowed("Verify parameter values are within acceptable ranges"),
            ],
            ErrorCategory::ToolNotFound => vec![
                Cow::Borrowed("Verify the tool name is spelled correctly"),
                Cow::Borrowed("Check if the tool is available in the current context"),
            ],
            ErrorCategory::ResourceNotFound => vec![
                Cow::Borrowed("Verify file paths and resource locations"),
                Cow::Borrowed("Check if files exist and are accessible"),
                Cow::Borrowed("Use list_dir to explore available resources"),
            ],
            ErrorCategory::PermissionDenied => vec![
                Cow::Borrowed("Check file permissions and access rights"),
                Cow::Borrowed("Ensure workspace boundaries are respected"),
            ],
            ErrorCategory::PolicyViolation => vec![
                Cow::Borrowed("Review workspace policies and restrictions"),
                Cow::Borrowed("Use alternative tools that comply with policies"),
            ],
            ErrorCategory::PlanModeViolation => vec![
                Cow::Borrowed("This operation is not allowed in plan/read-only mode"),
                Cow::Borrowed("Exit plan mode to perform mutating operations"),
            ],
            ErrorCategory::SandboxFailure => vec![
                Cow::Borrowed("The sandbox denied this operation"),
                Cow::Borrowed("Check sandbox configuration and permissions"),
            ],
            ErrorCategory::ResourceExhausted => vec![
                Cow::Borrowed("Check your account usage limits and billing status"),
                Cow::Borrowed("Review resource consumption and optimize if possible"),
            ],
            ErrorCategory::Cancelled => vec![Cow::Borrowed("The operation was cancelled")],
            ErrorCategory::ExecutionError => vec![
                Cow::Borrowed("Review error details for specific issues"),
                Cow::Borrowed("Check tool documentation for known limitations"),
            ],
        }
    }

    /// Get a concise, user-friendly label for this error category.
    pub const fn user_label(&self) -> &'static str {
        match self {
            ErrorCategory::Network => "Network error",
            ErrorCategory::Timeout => "Request timed out",
            ErrorCategory::RateLimit => "Rate limit exceeded",
            ErrorCategory::ServiceUnavailable => "Service temporarily unavailable",
            ErrorCategory::CircuitOpen => "Tool temporarily disabled",
            ErrorCategory::Authentication => "Authentication failed",
            ErrorCategory::InvalidParameters => "Invalid parameters",
            ErrorCategory::ToolNotFound => "Tool not found",
            ErrorCategory::ResourceNotFound => "Resource not found",
            ErrorCategory::PermissionDenied => "Permission denied",
            ErrorCategory::PolicyViolation => "Blocked by policy",
            ErrorCategory::PlanModeViolation => "Not allowed in plan mode",
            ErrorCategory::SandboxFailure => "Sandbox denied",
            ErrorCategory::ResourceExhausted => "Resource limit reached",
            ErrorCategory::Cancelled => "Operation cancelled",
            ErrorCategory::ExecutionError => "Execution failed",
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.user_label())
    }
}

// ---------------------------------------------------------------------------
// Classify from anyhow::Error (string-based fallback for erased types)
// ---------------------------------------------------------------------------

/// Classify an `anyhow::Error` into a canonical `ErrorCategory`.
///
/// This uses string matching as a last resort when the original error type has
/// been erased through `anyhow` wrapping. Typed conversions (e.g., `From<LLMError>`)
/// should be preferred where the original error type is available.
pub fn classify_anyhow_error(err: &anyhow::Error) -> ErrorCategory {
    let msg = err.to_string().to_ascii_lowercase();
    classify_error_message(&msg)
}

/// Classify an error message string into an `ErrorCategory`.
///
/// Marker groups are checked in priority order to handle overlapping patterns
/// (e.g., "tool permission denied by policy" → `PolicyViolation`, not `PermissionDenied`).
pub fn classify_error_message(msg: &str) -> ErrorCategory {
    let msg = if msg.as_bytes().iter().any(|b| b.is_ascii_uppercase()) {
        Cow::Owned(msg.to_ascii_lowercase())
    } else {
        Cow::Borrowed(msg)
    };

    // --- Priority 1: Policy violations (before permission checks) ---
    if contains_any(
        &msg,
        &[
            "policy violation",
            "denied by policy",
            "tool permission denied",
            "safety validation failed",
            "not allowed in plan mode",
            "only available when plan mode is active",
            "workspace boundary",
            "blocked by policy",
        ],
    ) {
        return ErrorCategory::PolicyViolation;
    }

    // --- Priority 2: Plan mode violations ---
    if contains_any(
        &msg,
        &["plan mode", "read-only mode", "plan_mode_violation"],
    ) {
        return ErrorCategory::PlanModeViolation;
    }

    // --- Priority 3: Authentication / Authorization ---
    if contains_any(
        &msg,
        &[
            "invalid api key",
            "authentication failed",
            "unauthorized",
            "401",
            "invalid credentials",
        ],
    ) {
        return ErrorCategory::Authentication;
    }

    // --- Priority 4: Non-retryable resource exhaustion (billing, quotas) ---
    if contains_any(
        &msg,
        &[
            "weekly usage limit",
            "daily usage limit",
            "monthly spending limit",
            "insufficient credits",
            "quota exceeded",
            "billing",
            "payment required",
        ],
    ) {
        return ErrorCategory::ResourceExhausted;
    }

    // --- Priority 5: Invalid parameters ---
    if contains_any(
        &msg,
        &[
            "invalid argument",
            "invalid parameters",
            "malformed",
            "missing required",
            "schema validation",
            "argument validation failed",
            "unknown field",
            "type mismatch",
        ],
    ) {
        return ErrorCategory::InvalidParameters;
    }

    // --- Priority 6: Tool not found ---
    if contains_any(
        &msg,
        &[
            "tool not found",
            "unknown tool",
            "unsupported tool",
            "no such tool",
        ],
    ) {
        return ErrorCategory::ToolNotFound;
    }

    // --- Priority 7: Resource not found ---
    if contains_any(
        &msg,
        &[
            "no such file",
            "no such directory",
            "file not found",
            "directory not found",
            "resource not found",
            "path not found",
            "enoent",
        ],
    ) {
        return ErrorCategory::ResourceNotFound;
    }

    // --- Priority 8: Permission denied (OS-level) ---
    if contains_any(
        &msg,
        &[
            "permission denied",
            "access denied",
            "operation not permitted",
            "eacces",
            "eperm",
            "forbidden",
            "403",
        ],
    ) {
        return ErrorCategory::PermissionDenied;
    }

    // --- Priority 9: Cancellation ---
    if contains_any(&msg, &["cancelled", "interrupted", "canceled"]) {
        return ErrorCategory::Cancelled;
    }

    // --- Priority 10: Circuit breaker ---
    if contains_any(&msg, &["circuit breaker", "circuit open"]) {
        return ErrorCategory::CircuitOpen;
    }

    // --- Priority 11: Sandbox ---
    if contains_any(&msg, &["sandbox denied", "sandbox failure"]) {
        return ErrorCategory::SandboxFailure;
    }

    // --- Priority 12: Rate limiting (before general network) ---
    if contains_any(&msg, &["rate limit", "too many requests", "429", "throttl"]) {
        return ErrorCategory::RateLimit;
    }

    // --- Priority 13: Timeout ---
    if contains_any(&msg, &["timeout", "timed out", "deadline exceeded"]) {
        return ErrorCategory::Timeout;
    }

    // --- Priority 14: Network / Service unavailable ---
    if contains_any(
        &msg,
        &[
            "network",
            "connection reset",
            "connection refused",
            "broken pipe",
            "dns",
            "name resolution",
            "service unavailable",
            "temporarily unavailable",
            "internal server error",
            "bad gateway",
            "gateway timeout",
            "overloaded",
            "try again",
            "retry later",
            "500",
            "502",
            "503",
            "504",
            "upstream connect error",
            "tls handshake",
            "socket hang up",
            "econnreset",
            "etimedout",
        ],
    ) {
        return ErrorCategory::Network;
    }

    // --- Priority 15: Resource exhausted (memory, disk) ---
    if contains_any(&msg, &["out of memory", "disk full", "no space left"]) {
        return ErrorCategory::ResourceExhausted;
    }

    // --- Fallback ---
    ErrorCategory::ExecutionError
}

/// Check if an LLM error message is retryable (used by the LLM request retry loop).
///
/// This is a focused classifier for LLM provider errors, combining
/// non-retryable and retryable marker checks for the request retry path.
pub fn is_retryable_llm_error_message(msg: &str) -> bool {
    let category = classify_error_message(msg);
    category.is_retryable()
}

#[inline]
fn contains_any(message: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| message.contains(marker))
}

// ---------------------------------------------------------------------------
// Typed conversions from known error types
// ---------------------------------------------------------------------------

impl From<&crate::llm::LLMError> for ErrorCategory {
    fn from(err: &crate::llm::LLMError) -> Self {
        match err {
            crate::llm::LLMError::Authentication { .. } => ErrorCategory::Authentication,
            crate::llm::LLMError::RateLimit { .. } => ErrorCategory::RateLimit,
            crate::llm::LLMError::InvalidRequest { .. } => ErrorCategory::InvalidParameters,
            crate::llm::LLMError::Network { .. } => ErrorCategory::Network,
            crate::llm::LLMError::Provider { message, metadata } => {
                // Check metadata status code first for precise classification
                if let Some(meta) = metadata {
                    if let Some(status) = meta.status {
                        return match status {
                            401 => ErrorCategory::Authentication,
                            403 => ErrorCategory::PermissionDenied,
                            404 => ErrorCategory::ResourceNotFound,
                            429 => ErrorCategory::RateLimit,
                            400 => ErrorCategory::InvalidParameters,
                            500 | 502 | 503 | 504 => ErrorCategory::ServiceUnavailable,
                            408 => ErrorCategory::Timeout,
                            _ => classify_error_message(message),
                        };
                    }
                }
                // Fall back to message-based classification
                classify_error_message(message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- classify_error_message tests ---

    #[test]
    fn policy_violation_takes_priority_over_permission() {
        assert_eq!(
            classify_error_message("tool permission denied by policy"),
            ErrorCategory::PolicyViolation
        );
    }

    #[test]
    fn rate_limit_classified_correctly() {
        assert_eq!(
            classify_error_message("provider returned 429 Too Many Requests"),
            ErrorCategory::RateLimit
        );
        assert_eq!(
            classify_error_message("rate limit exceeded"),
            ErrorCategory::RateLimit
        );
    }

    #[test]
    fn service_unavailable_is_network() {
        assert_eq!(
            classify_error_message("503 service unavailable"),
            ErrorCategory::Network
        );
    }

    #[test]
    fn authentication_errors() {
        assert_eq!(
            classify_error_message("invalid api key provided"),
            ErrorCategory::Authentication
        );
        assert_eq!(
            classify_error_message("401 unauthorized"),
            ErrorCategory::Authentication
        );
    }

    #[test]
    fn billing_errors_are_resource_exhausted() {
        assert_eq!(
            classify_error_message("you have reached your weekly usage limit"),
            ErrorCategory::ResourceExhausted
        );
        assert_eq!(
            classify_error_message("quota exceeded for this model"),
            ErrorCategory::ResourceExhausted
        );
    }

    #[test]
    fn timeout_errors() {
        assert_eq!(
            classify_error_message("connection timeout"),
            ErrorCategory::Timeout
        );
        assert_eq!(
            classify_error_message("request timed out after 30s"),
            ErrorCategory::Timeout
        );
    }

    #[test]
    fn network_errors() {
        assert_eq!(
            classify_error_message("connection reset by peer"),
            ErrorCategory::Network
        );
        assert_eq!(
            classify_error_message("dns name resolution failed"),
            ErrorCategory::Network
        );
    }

    #[test]
    fn tool_not_found() {
        assert_eq!(
            classify_error_message("unknown tool: ask_questions"),
            ErrorCategory::ToolNotFound
        );
    }

    #[test]
    fn resource_not_found() {
        assert_eq!(
            classify_error_message("no such file or directory: /tmp/missing"),
            ErrorCategory::ResourceNotFound
        );
    }

    #[test]
    fn permission_denied() {
        assert_eq!(
            classify_error_message("permission denied: /etc/shadow"),
            ErrorCategory::PermissionDenied
        );
    }

    #[test]
    fn cancelled_operations() {
        assert_eq!(
            classify_error_message("operation cancelled by user"),
            ErrorCategory::Cancelled
        );
    }

    #[test]
    fn plan_mode_violation() {
        assert_eq!(
            classify_error_message("not allowed in plan mode"),
            ErrorCategory::PolicyViolation
        );
    }

    #[test]
    fn sandbox_failure() {
        assert_eq!(
            classify_error_message("sandbox denied this operation"),
            ErrorCategory::SandboxFailure
        );
    }

    #[test]
    fn unknown_error_is_execution_error() {
        assert_eq!(
            classify_error_message("something went wrong"),
            ErrorCategory::ExecutionError
        );
    }

    #[test]
    fn invalid_parameters() {
        assert_eq!(
            classify_error_message("invalid argument: missing path field"),
            ErrorCategory::InvalidParameters
        );
    }

    // --- Retryability tests ---

    #[test]
    fn retryable_categories() {
        assert!(ErrorCategory::Network.is_retryable());
        assert!(ErrorCategory::Timeout.is_retryable());
        assert!(ErrorCategory::RateLimit.is_retryable());
        assert!(ErrorCategory::ServiceUnavailable.is_retryable());
        assert!(ErrorCategory::CircuitOpen.is_retryable());
    }

    #[test]
    fn non_retryable_categories() {
        assert!(!ErrorCategory::Authentication.is_retryable());
        assert!(!ErrorCategory::InvalidParameters.is_retryable());
        assert!(!ErrorCategory::PolicyViolation.is_retryable());
        assert!(!ErrorCategory::ResourceExhausted.is_retryable());
        assert!(!ErrorCategory::Cancelled.is_retryable());
    }

    #[test]
    fn permanent_error_detection() {
        assert!(ErrorCategory::Authentication.is_permanent());
        assert!(ErrorCategory::PolicyViolation.is_permanent());
        assert!(!ErrorCategory::Network.is_permanent());
        assert!(!ErrorCategory::Timeout.is_permanent());
    }

    #[test]
    fn llm_mistake_detection() {
        assert!(ErrorCategory::InvalidParameters.is_llm_mistake());
        assert!(!ErrorCategory::Network.is_llm_mistake());
        assert!(!ErrorCategory::Timeout.is_llm_mistake());
    }

    // --- LLM error conversion ---

    #[test]
    fn llm_error_authentication_converts() {
        let err = crate::llm::LLMError::Authentication {
            message: "bad key".to_string(),
            metadata: None,
        };
        assert_eq!(ErrorCategory::from(&err), ErrorCategory::Authentication);
    }

    #[test]
    fn llm_error_rate_limit_converts() {
        let err = crate::llm::LLMError::RateLimit { metadata: None };
        assert_eq!(ErrorCategory::from(&err), ErrorCategory::RateLimit);
    }

    #[test]
    fn llm_error_network_converts() {
        let err = crate::llm::LLMError::Network {
            message: "connection refused".to_string(),
            metadata: None,
        };
        assert_eq!(ErrorCategory::from(&err), ErrorCategory::Network);
    }

    #[test]
    fn llm_error_provider_with_status_code() {
        use crate::llm::LLMErrorMetadata;
        let err = crate::llm::LLMError::Provider {
            message: "error".to_string(),
            metadata: Some(LLMErrorMetadata::new(
                "openai",
                Some(503),
                None,
                None,
                None,
                None,
                None,
            )),
        };
        assert_eq!(ErrorCategory::from(&err), ErrorCategory::ServiceUnavailable);
    }

    // --- is_retryable_llm_error_message ---

    #[test]
    fn retryable_llm_messages() {
        assert!(is_retryable_llm_error_message("429 too many requests"));
        assert!(is_retryable_llm_error_message("500 internal server error"));
        assert!(is_retryable_llm_error_message("connection timeout"));
        assert!(is_retryable_llm_error_message("network error"));
    }

    #[test]
    fn non_retryable_llm_messages() {
        assert!(!is_retryable_llm_error_message("invalid api key"));
        assert!(!is_retryable_llm_error_message(
            "weekly usage limit reached"
        ));
        assert!(!is_retryable_llm_error_message("permission denied"));
    }

    // --- Recovery suggestions ---

    #[test]
    fn recovery_suggestions_non_empty() {
        for cat in [
            ErrorCategory::Network,
            ErrorCategory::Timeout,
            ErrorCategory::RateLimit,
            ErrorCategory::Authentication,
            ErrorCategory::InvalidParameters,
            ErrorCategory::ToolNotFound,
            ErrorCategory::ResourceNotFound,
            ErrorCategory::PermissionDenied,
            ErrorCategory::PolicyViolation,
            ErrorCategory::ExecutionError,
        ] {
            assert!(
                !cat.recovery_suggestions().is_empty(),
                "Missing recovery suggestions for {:?}",
                cat
            );
        }
    }

    // --- User label ---

    #[test]
    fn user_labels_are_non_empty() {
        assert!(!ErrorCategory::Network.user_label().is_empty());
        assert!(!ErrorCategory::ExecutionError.user_label().is_empty());
    }

    // --- Display ---

    #[test]
    fn display_matches_user_label() {
        assert_eq!(
            format!("{}", ErrorCategory::RateLimit),
            ErrorCategory::RateLimit.user_label()
        );
    }
}
