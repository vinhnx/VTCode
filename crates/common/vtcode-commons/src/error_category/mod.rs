#![expect(
    clippy::let_underscore_must_use,
    reason = "The category formatter intentionally ignores infallible formatting results."
)]

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
use std::fmt::Write;
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
    /// Planning workflow violation (mutating tool without read-only capabilities)
    PlanningPolicyViolation,
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
    /// Return the category as a `&'static str`, avoiding the temporary
    /// `String` allocation that `to_string()` (via `Display`) would create.
    /// Equivalent to `user_label()` but named to match the conventional
    /// `as_str()` accessor used across the codebase.
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        self.user_label()
    }

    /// Whether this error category is safe to retry.
    #[inline]
    #[must_use]
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

    /// Whether this category should count toward circuit breaker transitions.
    #[inline]
    #[must_use]
    pub const fn should_trip_circuit_breaker(&self) -> bool {
        matches!(
            self,
            ErrorCategory::Network
                | ErrorCategory::Timeout
                | ErrorCategory::RateLimit
                | ErrorCategory::ServiceUnavailable
                | ErrorCategory::ExecutionError
        )
    }

    /// Whether this error is an LLM argument mistake (should not count toward
    /// circuit breaker thresholds).
    #[inline]
    #[must_use]
    const fn is_llm_mistake(&self) -> bool {
        matches!(self, ErrorCategory::InvalidParameters)
    }

    /// Whether this error represents a permanent, non-recoverable condition.
    #[inline]
    #[must_use]
    pub const fn is_permanent(&self) -> bool {
        matches!(
            self,
            ErrorCategory::Authentication
                | ErrorCategory::PolicyViolation
                | ErrorCategory::PlanningPolicyViolation
                | ErrorCategory::ResourceExhausted
        )
    }

    /// Get the recommended retryability for this error category.
    #[must_use]
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
    #[must_use]
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
            ErrorCategory::PlanningPolicyViolation => vec![
                Cow::Borrowed("This operation is not allowed in the Planning workflow with read-only permissions"),
                Cow::Borrowed("Exit the Planning workflow to perform mutating operations"),
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
    #[must_use]
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
            ErrorCategory::PlanningPolicyViolation => "Not allowed in planning workflow",
            ErrorCategory::SandboxFailure => "Sandbox denied",
            ErrorCategory::ResourceExhausted => "Resource limit reached",
            ErrorCategory::Cancelled => "Operation cancelled",
            ErrorCategory::ExecutionError => "Execution failed",
        }
    }

    /// Build actionable guidance for authentication errors.
    ///
    /// Returns a single combined line directing the user to `/secret` (API-key
    /// providers) or `/login` (managed-auth providers). Env-var guidance is
    /// intentionally omitted — secure storage via `/secret` is the canonical
    /// path and env vars are a fallback only.
    ///
    /// `has_stored_credential` distinguishes "no key at all" (needs `/secret add`)
    /// from "key stored but rejected by the API" (needs verification).
    #[must_use]
    pub fn auth_recovery_guidance(
        &self,
        provider_label: &str,
        provider_key: &str,
        is_managed_auth: bool,
        has_stored_credential: bool,
    ) -> Vec<String> {
        if !matches!(self, ErrorCategory::Authentication) {
            return vec![];
        }

        if is_managed_auth {
            vec![format!(
                "Authentication failed for {provider_label}. Run /login {provider_key} to re-authenticate."
            )]
        } else if has_stored_credential {
            vec![format!(
                "Authentication failed for {provider_label}. The stored API key was rejected — run /secret add {provider_key} to replace it with a valid key."
            )]
        } else {
            vec![format!(
                "Authentication failed for {provider_label}. Run /secret add {provider_key} to store your API key in secure storage (OS keyring or encrypted file)."
            )]
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
///
/// Recovery decisions are made by *type* when it survives the `anyhow`
/// boundary. The outermost context layer is classified first (preserving the
/// intentional wording of a deliberate `.context(...)`), but when that layer is
/// uninformative we consult the typed root and then the remaining cause chain.
/// Without this, a wrapping `.context("...")` message alone would hide the
/// underlying category — e.g. an `LLMError::RateLimit` buried under
/// `"compaction failed"` would fall through to `ExecutionError` and lose its
/// retry/recovery policy.
#[must_use]
pub fn classify_anyhow_error(err: &anyhow::Error) -> ErrorCategory {
    // Fast path: the outermost message is usually the most specific context and
    // matches the historical behavior exactly.
    let outer = classify_error_message(&err.to_string());
    if outer != ErrorCategory::ExecutionError {
        return outer;
    }

    // The outer context was uninformative (or absent). Prefer the typed source
    // of truth when it survived the boundary, then fall back to the deeper
    // cause chain (`innermost` markers such as `connection reset`).
    if let Some(llm_error) = err.downcast_ref::<crate::llm::LLMError>() {
        return ErrorCategory::from(llm_error);
    }

    for cause in err.chain().skip(1) {
        let category = classify_error_message(&cause.to_string());
        if category != ErrorCategory::ExecutionError {
            return category;
        }
    }

    ErrorCategory::ExecutionError
}

/// Return whether an error explicitly reports that the provider rejected the
/// request because its input context is too large.
///
/// Context-capacity failures are intentionally kept separate from
/// [`ErrorCategory::InvalidParameters`]: they are not model argument mistakes
/// and a bounded history compaction can make the same request valid. The
/// marker set is deliberately specific so ordinary validation messages that
/// merely mention a context field do not enter recovery.
#[must_use]
pub fn is_context_capacity_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| is_context_capacity_message(&cause.to_string()))
}

/// Return whether a provider error message identifies an input-context limit.
#[must_use]
pub fn is_context_capacity_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    contains_any(
        &message,
        &[
            "context_length_exceeded",
            "context length exceeded",
            "maximum context length",
            "maximum context window",
            "context window exceeded",
            "context window is too small",
            "exceeds the model's maximum context",
            "exceeds model context",
            "exceeds the maximum context",
            "prompt is too long",
            "input is too long",
            "input token count exceeds",
            "exceeds the maximum number of tokens",
            "maximum input tokens",
            "too many tokens in the prompt",
            "request exceeds the context",
            // Merge-gateway / Anthropic / OpenAI variants observed on
            // near-budget follow-ups (e.g. ~956k prompt on a 1M budget) that
            // previously fell through to generic `ExecutionError` and missed
            // the compacting tool-enabled recovery path.
            "context limit exceeded",
            "context size exceeded",
            "exceeds context",
            "input tokens exceed",
            "prompt tokens exceed",
            "prompt token count exceeds",
            "token limit exceeded",
            "input too large",
            "prompt too large",
        ],
    )
}

/// Classify an error message string into an `ErrorCategory`.
///
/// Marker groups are checked in priority order to handle overlapping patterns
/// (e.g., "tool permission denied by policy" → `PolicyViolation`, not `PermissionDenied`).
#[inline]
#[must_use]
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
            "not allowed in planning workflow",
            "only available when planning workflow is active",
            "workspace boundary",
            "blocked by policy",
        ],
    ) {
        return ErrorCategory::PolicyViolation;
    }

    // --- Priority 2: Planning workflow violations ---
    if contains_any(
        &msg,
        &[
            "planning workflow",
            "read-only permissions",
            concat!("read-only ", "mode"),
            "planning_policy_violation",
        ],
    ) {
        return ErrorCategory::PlanningPolicyViolation;
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
            "invalid type",
            "malformed",
            "failed to parse arguments",
            "failed to parse argument",
            "missing required",
            "at least one item is required",
            "is required for",
            "schema validation",
            "argument validation failed",
            "unknown field",
            "unknown variant",
            "expected struct",
            "expected enum",
            "type mismatch",
            "must be an absolute path",
            "not parseable",
            "parseable as",
            // Patch format errors — these are LLM argument mistakes (the model
            // sent a malformed patch), not execution failures. Classifying them
            // as InvalidParameters ensures they don't trip the circuit breaker
            // (is_llm_mistake() == true) and get parameter-focused recovery
            // suggestions. See checkpoint turn_615 for the failure this
            // prevents: a unified-diff patch was classified as ExecutionError
            // and got generic "check tool documentation" suggestions.
            "invalid patch format",
            "invalid patch hunk",
            "invalid patch operation",
            "cannot parse empty patch",
            "patch does not contain",
            "semantic patch anchor",
        ],
    ) {
        return ErrorCategory::InvalidParameters;
    }

    // --- Priority 6: Tool not found ---
    if contains_any(&msg, &["tool not found", "unknown tool", "unsupported tool", "no such tool"]) {
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
            "does not exist",
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

    // --- Priority 14: Provider transient response-shape failures ---
    if contains_any(
        &msg,
        &[
            "invalid response format: missing choices",
            "invalid response format: missing message",
            "missing choices in response",
            "missing message in choice",
            "no choices in response",
            "invalid response from ",
            "empty response body",
            "response did not contain",
            "unexpected response format",
            "failed to parse response",
            // Post-tool follow-up streaming failures surface here when the
            // provider drops the SSE/event stream mid-response. Without these
            // markers the follow-up is misclassified as generic
            // `ExecutionError` (non-retryable) and the turn ends instead of
            // scheduling the bounded tool-free retry.
            "stream disconnected",
            "stream closed unexpectedly",
            "incomplete stream",
            "unexpected end of stream",
            "truncated response",
            "failed to decode stream",
            "stream terminated",
        ],
    ) {
        return ErrorCategory::ServiceUnavailable;
    }

    // --- Priority 15: Service unavailable (HTTP 5xx and related) ---
    if contains_any(
        &msg,
        &[
            "service unavailable",
            "temporarily unavailable",
            "internal server error",
            "bad gateway",
            "gateway timeout",
            "overloaded",
            "500",
            "502",
            "503",
            "504",
        ],
    ) {
        return ErrorCategory::ServiceUnavailable;
    }

    // --- Priority 16: Network (connectivity, DNS, transport) ---
    if contains_any(
        &msg,
        &[
            "network",
            "connection reset",
            "connection refused",
            "broken pipe",
            "dns",
            "name resolution",
            "try again",
            "retry later",
            "upstream connect error",
            "tls handshake",
            "socket hang up",
            "econnreset",
            "etimedout",
            // reqwest surfaces truncated/aborted response streams as a body
            // decode failure; Ollama (cloud) emits this on transient drops.
            // It is a transport error, not a payload problem — retryable.
            "error decoding response body",
            // Transport-level stream teardown during a follow-up request.
            // Distinct from response-shape markers above: the connection
            // itself failed, not the payload. `stream error` stays specific
            // (`received`) so `downstream error` prose does not match.
            "connection closed before",
            "stream reset",
            "stream error received",
        ],
    ) {
        return ErrorCategory::Network;
    }

    // --- Priority 17: Resource exhausted (memory, disk) ---
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
#[inline]
#[must_use]
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
            crate::llm::LLMError::RateLimit { metadata } => {
                classify_llm_metadata(metadata.as_deref(), ErrorCategory::RateLimit)
            }
            crate::llm::LLMError::InvalidRequest { .. } => ErrorCategory::InvalidParameters,
            crate::llm::LLMError::Network { .. } => ErrorCategory::Network,
            crate::llm::LLMError::Provider { message, metadata } => {
                let metadata_category = classify_llm_metadata(metadata.as_deref(), ErrorCategory::ExecutionError);
                if metadata_category != ErrorCategory::ExecutionError {
                    return metadata_category;
                }

                // Check metadata status code first for precise classification
                if let Some(meta) = metadata
                    && let Some(status) = meta.status
                {
                    return match status {
                        401 => ErrorCategory::Authentication,
                        403 => ErrorCategory::PermissionDenied,
                        404 => ErrorCategory::ResourceNotFound,
                        429 => ErrorCategory::RateLimit,
                        400 => ErrorCategory::InvalidParameters,
                        500 | 502 | 503 | 504 | 529 => ErrorCategory::ServiceUnavailable,
                        408 => ErrorCategory::Timeout,
                        _ => classify_error_message(message),
                    };
                }
                // Fall back to message-based classification
                classify_error_message(message)
            }
        }
    }
}

fn classify_llm_metadata(metadata: Option<&crate::llm::LLMErrorMetadata>, fallback: ErrorCategory) -> ErrorCategory {
    let Some(metadata) = metadata else {
        return fallback;
    };

    let mut hint = String::new();
    if let Some(code) = &metadata.code {
        hint.push_str(code);
        hint.push(' ');
    }
    if let Some(message) = &metadata.message {
        hint.push_str(message);
        hint.push(' ');
    }
    if let Some(status) = metadata.status {
        let _ = write!(&mut hint, "{status}");
    }

    let classified = classify_error_message(&hint);
    if classified == ErrorCategory::ExecutionError {
        fallback
    } else {
        classified
    }
}

#[cfg(test)]
mod tests;
