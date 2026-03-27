use anyhow::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use vtcode_commons::ErrorCategory;

use crate::retry::is_command_tool;
use crate::retry::{RetryDecision, RetryPolicy};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)] // Added Copy since it's a simple enum
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

impl ToolExecutionError {
    #[inline]
    pub fn new(tool_name: String, error_type: ToolErrorType, message: String) -> Self {
        let category = ErrorCategory::from(error_type);
        let (retryable, is_recoverable, recovery_suggestions) =
            generate_recovery_info(tool_name.as_str(), category, error_type);

        // PTY/command tool timeouts should NOT be retried - the underlying process
        // may still be running and retrying will cause Cargo.lock contention or
        // other resource conflicts
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
    pub fn with_original_error(
        tool_name: String,
        error_type: ToolErrorType,
        message: String,
        original_error: String,
    ) -> Self {
        let mut error = Self::new(tool_name, error_type, message);
        error.original_error = Some(original_error);
        error
    }

    pub fn from_anyhow(
        tool_name: impl Into<String>,
        error: &Error,
        attempt_index: u32,
        partial_state_possible: bool,
        rollback_performed: bool,
        surface: Option<&str>,
    ) -> Self {
        let tool_name = tool_name.into();
        let mut structured = Self::with_original_error(
            tool_name.clone(),
            classify_error(error),
            error.to_string(),
            format!("{error:#}"),
        );
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

    pub fn policy_violation(tool_name: String, message: impl Into<String>) -> Self {
        Self::new(tool_name, ToolErrorType::PolicyViolation, message.into())
    }

    pub fn with_retry_decision(mut self, decision: RetryDecision) -> Self {
        self.category = decision.category;
        self.retryable = decision.retryable;
        self.retry_delay_ms = decision.delay.map(|delay| delay.as_millis() as u64);
        self.retry_after_ms = decision.retry_after.map(|delay| delay.as_millis() as u64);
        self.circuit_breaker_impact = decision.category.should_trip_circuit_breaker();
        self
    }

    pub fn with_partial_state(
        mut self,
        partial_state_possible: bool,
        rollback_performed: bool,
    ) -> Self {
        self.partial_state_possible = partial_state_possible;
        self.rollback_performed = rollback_performed;
        self
    }

    pub fn with_surface(mut self, surface: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.surface = Some(surface.into());
        self
    }

    pub fn with_attempt(mut self, attempt: u32) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.attempt = Some(attempt);
        self
    }

    pub fn with_invocation_id(mut self, invocation_id: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.invocation_id = Some(invocation_id.into());
        self
    }

    pub fn with_debug_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let debug = self
            .debug_context
            .get_or_insert_with(ToolErrorDebugContext::default);
        debug.metadata.push((key.into(), value.into()));
        self
    }

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

    pub fn attempts_made(&self) -> Option<u32> {
        self.debug_context
            .as_ref()
            .and_then(|context| context.attempt)
    }

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

    pub fn retry_delay(&self) -> Option<std::time::Duration> {
        self.retry_delay_ms.map(std::time::Duration::from_millis)
    }

    pub fn retry_after(&self) -> Option<std::time::Duration> {
        self.retry_after_ms.map(std::time::Duration::from_millis)
    }

    pub fn from_tool_output(output: &Value) -> Option<Self> {
        let error_payload = output.get("error")?;
        Self::from_error_payload(error_payload)
    }

    pub fn from_error_payload(error_payload: &Value) -> Option<Self> {
        if let Some(inner) = error_payload.get("error") {
            return Self::from_error_payload(inner);
        }

        if error_payload.is_object() && error_payload.get("message").is_some() {
            return serde_json::from_value(error_payload.clone())
                .ok()
                .or_else(|| {
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
                    let mut structured =
                        Self::new(tool_name.to_string(), error_type, message.to_string());
                    structured.category = category;
                    structured.retryable = error_payload
                        .get("retryable")
                        .and_then(Value::as_bool)
                        .unwrap_or(structured.retryable);
                    structured.is_recoverable = error_payload
                        .get("is_recoverable")
                        .and_then(Value::as_bool)
                        .unwrap_or(structured.is_recoverable);
                    structured.retry_delay_ms =
                        error_payload.get("retry_delay_ms").and_then(Value::as_u64);
                    structured.retry_after_ms =
                        error_payload.get("retry_after_ms").and_then(Value::as_u64);
                    structured.circuit_breaker_impact = error_payload
                        .get("circuit_breaker_impact")
                        .and_then(Value::as_bool)
                        .unwrap_or(structured.circuit_breaker_impact);
                    structured.partial_state_possible = error_payload
                        .get("partial_state_possible")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    structured.rollback_performed = error_payload
                        .get("rollback_performed")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    structured.original_error = error_payload
                        .get("original_error")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    Some(structured)
                });
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

    pub fn to_json_value(&self) -> Value {
        json!({
            "error": {
                "tool_name": self.tool_name,
                "error_type": format!("{:?}", self.error_type),
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

/// Classify an `anyhow::Error` into a `ToolErrorType`.
///
/// Delegates to the shared `ErrorCategory` classifier and converts the result
/// to preserve backward compatibility with existing callers.
pub fn classify_error(error: &Error) -> ToolErrorType {
    let category = vtcode_commons::classify_anyhow_error(error);
    ToolErrorType::from(category)
}

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
    let retryable = if matches!(error_type, ToolErrorType::Timeout) && is_command_tool(tool_name) {
        false
    } else {
        category.is_retryable()
    };
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
            ErrorCategory::PolicyViolation | ErrorCategory::PlanModeViolation => {
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

    #[test]
    fn classify_error_marks_rate_limit_as_network_error() {
        let err = anyhow!("provider returned 429 Too Many Requests");
        assert!(matches!(classify_error(&err), ToolErrorType::NetworkError));
    }

    #[test]
    fn classify_error_marks_service_unavailable_as_network_error() {
        let err = anyhow!("503 Service Unavailable");
        assert!(matches!(classify_error(&err), ToolErrorType::NetworkError));
    }

    #[test]
    fn classify_error_marks_weekly_usage_limit_as_execution_error() {
        let err = anyhow!("you have reached your weekly usage limit");
        assert!(matches!(
            classify_error(&err),
            ToolErrorType::ExecutionError
        ));
    }

    #[test]
    fn classify_error_marks_tool_not_found() {
        let err = anyhow!("unknown tool: ask_questions");
        assert!(matches!(classify_error(&err), ToolErrorType::ToolNotFound));
    }

    #[test]
    fn classify_error_marks_policy_violation_before_permission() {
        let err = anyhow!("tool permission denied by policy");
        assert!(matches!(
            classify_error(&err),
            ToolErrorType::PolicyViolation
        ));
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
