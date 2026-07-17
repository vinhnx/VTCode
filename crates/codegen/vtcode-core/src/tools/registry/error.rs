use anyhow::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use vtcode_commons::ErrorCategory;

use crate::retry::{RetryDecision, RetryPolicy, RetryPolicyCoreExt};
use crate::tools::tool_intent::is_command_tool;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolErrorDebugContext {
    pub surface: Option<String>,
    pub attempt: Option<u32>,
    pub invocation_id: Option<String>,
    pub metadata: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionError {
    pub tool_name: String,
    pub error_type: ToolErrorType,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub is_recoverable: bool,
    pub recovery_suggestions: Vec<Cow<'static, str>>,
    pub retry_delay_ms: Option<u64>,
    pub retry_after_ms: Option<u64>,
    pub circuit_breaker_impact: bool,
    pub partial_state_possible: bool,
    pub rollback_performed: bool,
    pub debug_context: Option<ToolErrorDebugContext>,
    pub original_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolErrorType {
    InvalidParameters,
    ToolNotFound,
    PermissionDenied,
    ResourceNotFound,
    NetworkError,
    Timeout,
    ExecutionError,
    PolicyViolation,
}

impl ToolErrorType {
    /// Return the error type as a static string for serialization.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidParameters => "InvalidParameters",
            Self::ToolNotFound => "ToolNotFound",
            Self::PermissionDenied => "PermissionDenied",
            Self::ResourceNotFound => "ResourceNotFound",
            Self::NetworkError => "NetworkError",
            Self::Timeout => "Timeout",
            Self::ExecutionError => "ExecutionError",
            Self::PolicyViolation => "PolicyViolation",
        }
    }
}

impl ToolExecutionError {
    #[inline]
    #[must_use]
    pub fn new(
        tool_name: impl Into<String>,
        error_type: ToolErrorType,
        message: impl Into<String>,
    ) -> Self {
        Self::from_parts(
            tool_name.into(),
            ErrorCategory::from(error_type),
            error_type,
            message.into(),
        )
    }

    /// Construct from a full-fidelity `ErrorCategory`, deriving the lossy
    /// `error_type` view from it. Prefer this over [`Self::new`] when the
    /// category came from `vtcode_commons::classify_anyhow_error` /
    /// `classify_error_message`, so distinctions such as `RateLimit` vs
    /// `Network` are preserved on the struct.
    #[must_use]
    fn from_category(
        tool_name: impl Into<String>,
        category: ErrorCategory,
        message: impl Into<String>,
    ) -> Self {
        Self::from_parts(
            tool_name.into(),
            category,
            ToolErrorType::from(category),
            message.into(),
        )
    }

    fn from_parts(
        tool_name: String,
        category: ErrorCategory,
        error_type: ToolErrorType,
        message: String,
    ) -> Self {
        let (retryable, is_recoverable, recovery_suggestions) =
            generate_recovery_info(tool_name.as_str(), category, error_type);

        Self {
            tool_name,
            error_type,
            category,
            message,
            retryable,
            is_recoverable,
            recovery_suggestions,
            retry_delay_ms: None,
            retry_after_ms: None,
            circuit_breaker_impact: category.should_trip_circuit_breaker(),
            partial_state_possible: false,
            rollback_performed: false,
            debug_context: None,
            original_error: None,
        }
    }

    #[inline]
    #[must_use]
    pub fn with_original_error(
        tool_name: impl Into<String>,
        error_type: ToolErrorType,
        message: impl Into<String>,
        original_error: impl Into<String>,
    ) -> Self {
        let mut error = Self::new(tool_name, error_type, message);
        error.original_error = Some(original_error.into());
        error
    }

    #[must_use]
    pub fn from_anyhow(
        tool_name: impl Into<String>,
        error: &Error,
        attempt_index: u32,
        partial_state_possible: bool,
        rollback_performed: bool,
        surface: Option<&str>,
    ) -> Self {
        let tool_name = tool_name.into();
        // Classify exactly once into the canonical category; the wire-visible
        // `error_type` is derived from it inside `from_category`.
        let category = vtcode_commons::classify_anyhow_error(error);
        let mut structured = Self::from_category(tool_name.clone(), category, error.to_string());
        structured.original_error = Some(format!("{error:#}"));
        structured = RetryPolicy::default().apply_to_tool_execution_error(
            structured,
            attempt_index,
            Some(tool_name.as_str()),
        );
        structured.partial_state_possible = partial_state_possible;
        structured.rollback_performed = rollback_performed;
        structured = apply_explicit_error_state(structured, tool_name.as_str(), error);
        if let Some(surface) = surface {
            structured = structured.with_surface(surface);
        }
        structured
    }

    #[must_use]
    pub fn policy_violation(tool_name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(tool_name, ToolErrorType::PolicyViolation, message)
    }

    #[must_use]
    pub fn with_retry_decision(mut self, decision: RetryDecision) -> Self {
        self.category = decision.category;
        // Keep the derived view in lockstep with the authoritative category.
        // `ToolErrorType::from` is the identity on every ToolErrorType ->
        // ErrorCategory -> ToolErrorType round trip, so this only changes
        // `error_type` when the decision actually recategorized the error.
        self.error_type = ToolErrorType::from(decision.category);
        self.retryable = decision.retryable;
        self.retry_delay_ms = decision.delay.map(|delay| delay.as_millis() as u64);
        self.retry_after_ms = decision.retry_after.map(|delay| delay.as_millis() as u64);
        self.circuit_breaker_impact = decision.category.should_trip_circuit_breaker();
        self
    }

    #[must_use]
    pub fn with_partial_state(
        mut self,
        partial_state_possible: bool,
        rollback_performed: bool,
    ) -> Self {
        self.partial_state_possible = partial_state_possible;
        self.rollback_performed = rollback_performed;
        self
    }

    #[must_use]
    pub fn with_surface(mut self, surface: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.surface = Some(surface.into());
        self
    }

    #[must_use]
    pub fn with_attempt(mut self, attempt: u32) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.attempt = Some(attempt);
        self
    }

    #[must_use]
    pub fn with_invocation_id(mut self, invocation_id: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.invocation_id = Some(invocation_id.into());
        self
    }

    #[must_use]
    pub fn with_debug_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.metadata.push((key.into(), value.into()));
        self
    }

    #[must_use]
    pub fn with_tool_call_context(mut self, tool_name: &str, args: &Value) -> Self {
        self.tool_name = tool_name.to_string();

        if tool_name == crate::config::constants::tools::APPLY_PATCH {
            return self;
        }

        let intent = crate::tools::tool_intent::classify_tool_intent(tool_name, args);
        if intent.mutating || is_command_tool(tool_name) {
            self.partial_state_possible = true;
        }

        self
    }

    #[must_use]
    pub fn attempts_made(&self) -> Option<u32> {
        self.debug_context
            .as_ref()
            .and_then(|context| context.attempt)
    }

    #[must_use]
    pub fn retry_summary(&self) -> Option<String> {
        let retry_count = self
            .attempts_made()
            .map(|attempts| attempts.saturating_sub(1))
            .unwrap_or(0);

        let mut summary = if matches!(self.category, ErrorCategory::CircuitOpen) {
            Some("The service is pausing new calls after repeated transient failures.".to_string())
        } else if retry_count > 0 {
            let suffix = if retry_count == 1 { "" } else { "s" };
            Some(format!(
                "Retried {retry_count} time{suffix} before failing."
            ))
        } else {
            None
        };

        if let Some(delay_ms) = self.retry_after_ms.or(self.retry_delay_ms) {
            let delay = format_retry_delay(delay_ms);
            match summary.as_mut() {
                Some(existing) => {
                    existing.push(' ');
                    existing.push_str("Recommended wait: ");
                    existing.push_str(&delay);
                    existing.push('.');
                }
                None => {
                    summary = Some(format!("Recommended wait: {delay}."));
                }
            }
        }

        summary
    }

    #[must_use]
    pub fn user_message(&self) -> String {
        let mut message = format!("[{}] {}", self.category.user_label(), self.message);

        if self.rollback_performed {
            message.push_str(" Any partial changes were rolled back.");
        } else if self.partial_state_possible {
            message.push_str(" Partial changes may still exist.");
        }

        if let Some(retry_summary) = self.retry_summary() {
            message.push(' ');
            message.push_str(&retry_summary);
        }

        if let Some(next_action) = self.recovery_suggestions.first() {
            message.push_str(" Next: ");
            message.push_str(next_action.as_ref());
        }

        message
    }

    #[must_use]
    pub fn retry_delay(&self) -> Option<std::time::Duration> {
        self.retry_delay_ms.map(std::time::Duration::from_millis)
    }

    #[must_use]
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        self.retry_after_ms.map(std::time::Duration::from_millis)
    }

    #[must_use]
    pub fn from_tool_output(output: &Value) -> Option<Self> {
        let error_payload = output.get("error")?;
        Self::from_error_payload(error_payload)
    }

    #[must_use]
    pub fn from_error_payload(error_payload: &Value) -> Option<Self> {
        if let Some(inner) = error_payload.get("error") {
            return Self::from_error_payload(inner);
        }

        if error_payload.is_object() && error_payload.get("message").is_some() {
            // Single source of truth: deserialize via serde. The previous
            // implementation hand-built the error struct from individual
            // fields, which silently dropped any new field that was added to
            // `ToolExecutionError` (it would fall through to `Self::new`'s
            // default for that field, which could be subtly wrong).
            //
            // `serde_json::from_value` honors every `#[serde(default)]` on
            // the struct, so a partial payload still reconstructs safely.
            if let Ok(structured) = serde_json::from_value::<Self>(error_payload.clone()) {
                return Some(structured);
            }

            // Last-resort fallback: pull out the minimum fields needed to
            // construct a usable error. This branch only fires when the
            // payload is malformed (e.g. wrong type for `category`).
            let tool_name = error_payload
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let message = error_payload
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Unknown tool execution error");
            let category = error_payload
                .get("category")
                .and_then(|value| serde_json::from_value(value.clone()).ok())
                .unwrap_or_else(|| vtcode_commons::classify_error_message(message));
            let error_type = error_payload
                .get("error_type")
                .and_then(Value::as_str)
                .map(parse_error_type)
                .unwrap_or_else(|| ToolErrorType::from(category));
            let mut structured = Self::new(tool_name.to_string(), error_type, message.to_string());
            structured.category = category;
            structured.original_error = error_payload
                .get("original_error")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            return Some(structured);
        }

        error_payload.as_str().map(|message| {
            let category = vtcode_commons::classify_error_message(message);
            Self::new(
                "tool".to_string(),
                ToolErrorType::from(category),
                message.to_string(),
            )
        })
    }

    #[must_use]
    pub fn to_json_value(&self) -> Value {
        json!({
            "error": {
                "tool_name": self.tool_name,
                "error_type": self.error_type.as_str(),
                "category": self.category,
                "message": self.message,
                "retryable": self.retryable,
                "is_recoverable": self.is_recoverable,
                "recovery_suggestions": self.recovery_suggestions,
                "retry_delay_ms": self.retry_delay_ms,
                "retry_after_ms": self.retry_after_ms,
                "circuit_breaker_impact": self.circuit_breaker_impact,
                "partial_state_possible": self.partial_state_possible,
                "rollback_performed": self.rollback_performed,
                "debug_context": self.debug_context,
                "original_error": self.original_error,
            }
        })
    }
}

impl std::fmt::Display for ToolExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ToolExecutionError {}

// Use static string slices to avoid allocations for recovery suggestions.
// Delegates to the shared `ErrorCategory` recovery suggestions where possible.
#[inline]
fn generate_recovery_info(
    tool_name: &str,
    category: ErrorCategory,
    error_type: ToolErrorType,
) -> (bool, bool, Vec<Cow<'static, str>>) {
    let is_recoverable = category.is_retryable()
        || matches!(
            error_type,
            ToolErrorType::InvalidParameters
                | ToolErrorType::PermissionDenied
                | ToolErrorType::ResourceNotFound
        );
    let retryable = category.is_retryable()
        && !crate::retry::is_non_retryable_command_timeout(category, Some(tool_name));
    (retryable, is_recoverable, category.recovery_suggestions())
}

fn parse_error_type(raw: &str) -> ToolErrorType {
    match raw {
        "InvalidParameters" => ToolErrorType::InvalidParameters,
        "ToolNotFound" => ToolErrorType::ToolNotFound,
        "PermissionDenied" => ToolErrorType::PermissionDenied,
        "ResourceNotFound" => ToolErrorType::ResourceNotFound,
        "NetworkError" => ToolErrorType::NetworkError,
        "Timeout" => ToolErrorType::Timeout,
        "ExecutionError" => ToolErrorType::ExecutionError,
        "PolicyViolation" => ToolErrorType::PolicyViolation,
        _ => ToolErrorType::ExecutionError,
    }
}

fn format_retry_delay(delay_ms: u64) -> String {
    if delay_ms >= 1_000 {
        format!("{:.1}s", delay_ms as f64 / 1_000.0)
    } else {
        format!("{delay_ms}ms")
    }
}

fn apply_explicit_error_state(
    mut error: ToolExecutionError,
    tool_name: &str,
    source: &Error,
) -> ToolExecutionError {
    if tool_name != crate::config::constants::tools::APPLY_PATCH {
        return error;
    }

    if let Some(patch_error) = source.downcast_ref::<crate::tools::editing::PatchError>() {
        match patch_error {
            crate::tools::editing::PatchError::RolledBack { .. } => {
                error = error.with_partial_state(false, true);
            }
            crate::tools::editing::PatchError::Recovery { .. } => {
                error = error.with_partial_state(true, false);
            }
            _ => {}
        }
    }

    error
}

// === Bridge conversions between ErrorCategory and ToolErrorType ===

impl From<ErrorCategory> for ToolErrorType {
    fn from(cat: ErrorCategory) -> Self {
        match cat {
            ErrorCategory::InvalidParameters => ToolErrorType::InvalidParameters,
            ErrorCategory::ToolNotFound => ToolErrorType::ToolNotFound,
            ErrorCategory::ResourceNotFound => ToolErrorType::ResourceNotFound,
            ErrorCategory::PermissionDenied => ToolErrorType::PermissionDenied,
            ErrorCategory::Network | ErrorCategory::ServiceUnavailable => {
                ToolErrorType::NetworkError
            }
            ErrorCategory::Timeout => ToolErrorType::Timeout,
            ErrorCategory::PolicyViolation | ErrorCategory::PlanningPolicyViolation => {
                ToolErrorType::PolicyViolation
            }
            ErrorCategory::RateLimit => ToolErrorType::NetworkError,
            ErrorCategory::CircuitOpen => ToolErrorType::ExecutionError,
            ErrorCategory::Authentication => ToolErrorType::PermissionDenied,
            ErrorCategory::SandboxFailure => ToolErrorType::PolicyViolation,
            ErrorCategory::ResourceExhausted => ToolErrorType::ExecutionError,
            ErrorCategory::Cancelled => ToolErrorType::ExecutionError,
            ErrorCategory::ExecutionError => ToolErrorType::ExecutionError,
        }
    }
}

impl From<ToolErrorType> for ErrorCategory {
    fn from(t: ToolErrorType) -> Self {
        match t {
            ToolErrorType::InvalidParameters => ErrorCategory::InvalidParameters,
            ToolErrorType::ToolNotFound => ErrorCategory::ToolNotFound,
            ToolErrorType::ResourceNotFound => ErrorCategory::ResourceNotFound,
            ToolErrorType::PermissionDenied => ErrorCategory::PermissionDenied,
            ToolErrorType::NetworkError => ErrorCategory::Network,
            ToolErrorType::Timeout => ErrorCategory::Timeout,
            ToolErrorType::PolicyViolation => ErrorCategory::PolicyViolation,
            ToolErrorType::ExecutionError => ErrorCategory::ExecutionError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    fn classify(err: &Error) -> ToolErrorType {
        ToolErrorType::from(vtcode_commons::classify_anyhow_error(err))
    }

    #[test]
    fn classify_error_marks_rate_limit_as_network_error() {
        let err = anyhow!("provider returned 429 Too Many Requests");
        assert!(matches!(classify(&err), ToolErrorType::NetworkError));
    }

    #[test]
    fn classify_error_marks_service_unavailable_as_network_error() {
        let err = anyhow!("503 Service Unavailable");
        assert!(matches!(classify(&err), ToolErrorType::NetworkError));
    }

    #[test]
    fn classify_error_marks_weekly_usage_limit_as_execution_error() {
        let err = anyhow!("you have reached your weekly usage limit");
        assert!(matches!(classify(&err), ToolErrorType::ExecutionError));
    }

    #[test]
    fn classify_error_marks_tool_not_found() {
        let err = anyhow!("unknown tool: ask_questions");
        assert!(matches!(classify(&err), ToolErrorType::ToolNotFound));
    }

    #[test]
    fn classify_error_marks_policy_violation_before_permission() {
        let err = anyhow!("tool permission denied by policy");
        assert!(matches!(classify(&err), ToolErrorType::PolicyViolation));
    }

    const ALL_TOOL_ERROR_TYPES: [ToolErrorType; 8] = [
        ToolErrorType::InvalidParameters,
        ToolErrorType::ToolNotFound,
        ToolErrorType::PermissionDenied,
        ToolErrorType::ResourceNotFound,
        ToolErrorType::NetworkError,
        ToolErrorType::Timeout,
        ToolErrorType::ExecutionError,
        ToolErrorType::PolicyViolation,
    ];

    #[test]
    fn error_type_wire_string_round_trips() {
        for error_type in ALL_TOOL_ERROR_TYPES {
            assert_eq!(parse_error_type(error_type.as_str()), error_type);
        }
    }

    #[test]
    fn error_type_category_round_trip_is_identity() {
        // Guarantees `with_retry_decision` never changes error_type unless the
        // decision actually recategorized the error.
        for error_type in ALL_TOOL_ERROR_TYPES {
            assert_eq!(
                ToolErrorType::from(ErrorCategory::from(error_type)),
                error_type
            );
        }
    }

    #[test]
    fn from_anyhow_derives_error_type_from_category() {
        let err = anyhow!("provider returned 429 Too Many Requests");
        let structured =
            ToolExecutionError::from_anyhow("grep_search", &err, 0, false, false, None);
        assert_eq!(structured.category, ErrorCategory::RateLimit);
        assert_eq!(
            structured.error_type,
            ToolErrorType::from(structured.category)
        );
    }

    #[test]
    fn retryable_matches_error_category_predicate() {
        for error_type in ALL_TOOL_ERROR_TYPES {
            let category = ErrorCategory::from(error_type);
            let structured =
                ToolExecutionError::new("grep_search".to_string(), error_type, "boom".to_string());
            assert_eq!(
                structured.retryable,
                category.is_retryable(),
                "retryable mismatch for {error_type:?}"
            );
        }
    }

    #[test]
    fn command_tool_timeouts_are_not_retryable() {
        let structured = ToolExecutionError::new(
            crate::config::constants::tools::CREATE_PTY_SESSION.to_string(),
            ToolErrorType::Timeout,
            "command timed out".to_string(),
        );
        assert!(!structured.retryable);
    }

    #[test]
    fn tool_call_context_marks_mutating_tools_as_partial_state_possible() {
        let error = ToolExecutionError::new(
            "write_file".to_string(),
            ToolErrorType::ExecutionError,
            "write failed".to_string(),
        )
        .with_tool_call_context(
            crate::config::constants::tools::WRITE_FILE,
            &serde_json::json!({"path": "note.txt", "content": "hello"}),
        );

        assert!(error.partial_state_possible);
        assert!(!error.rollback_performed);
    }

    #[test]
    fn tool_call_context_marks_apply_patch_failures_as_rolled_back() {
        let source = Error::new(crate::tools::editing::PatchError::RolledBack {
            original: Box::new(crate::tools::editing::PatchError::SegmentNotFound {
                path: "src/lib.rs".to_string(),
                snippet: "fn main()".to_string(),
            }),
        });
        let error = ToolExecutionError::from_anyhow(
            crate::config::constants::tools::APPLY_PATCH,
            &source,
            0,
            false,
            false,
            None,
        );

        assert!(!error.partial_state_possible);
        assert!(error.rollback_performed);
    }

    #[test]
    fn user_message_includes_retry_summary_and_wait() {
        let mut error = ToolExecutionError::new(
            crate::config::constants::tools::READ_FILE.to_string(),
            ToolErrorType::ExecutionError,
            "read failed".to_string(),
        )
        .with_attempt(2);
        error.retry_delay_ms = Some(1_500);

        let message = error.user_message();

        assert!(message.contains("Retried 1 time before failing."));
        assert!(message.contains("Recommended wait: 1.5s."));
    }
}
