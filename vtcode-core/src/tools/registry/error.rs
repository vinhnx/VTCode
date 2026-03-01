use anyhow::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use vtcode_commons::ErrorCategory;

use crate::config::constants::tools as tool_names;

/// Check if a tool is a command/PTY tool that spawns external processes.
/// These tools should not have their timeouts retried, as the underlying
/// process may still be running and cause resource conflicts.
fn is_command_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        n if n == tool_names::RUN_PTY_CMD
            || n == tool_names::UNIFIED_EXEC
            || n == tool_names::CREATE_PTY_SESSION
            || n == tool_names::SEND_PTY_INPUT
            || n == "shell"
            || n == "bash"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionError {
    pub tool_name: String,
    pub error_type: ToolErrorType,
    pub message: String,
    pub is_recoverable: bool,
    pub recovery_suggestions: Vec<Cow<'static, str>>,
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
        let (is_recoverable, recovery_suggestions) = generate_recovery_info(error_type);

        // PTY/command tool timeouts should NOT be retried - the underlying process
        // may still be running and retrying will cause Cargo.lock contention or
        // other resource conflicts
        let is_recoverable =
            if matches!(error_type, ToolErrorType::Timeout) && is_command_tool(&tool_name) {
                false
            } else {
                is_recoverable
            };

        Self {
            tool_name,
            error_type,
            message,
            is_recoverable,
            recovery_suggestions,
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

    pub fn to_json_value(&self) -> Value {
        json!({
            "error": {
                "tool_name": self.tool_name,
                "error_type": format!("{:?}", self.error_type),
                "message": self.message,
                "is_recoverable": self.is_recoverable,
                "recovery_suggestions": self.recovery_suggestions,
                "original_error": self.original_error,
            }
        })
    }
}

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
fn generate_recovery_info(error_type: ToolErrorType) -> (bool, Vec<Cow<'static, str>>) {
    let category = ErrorCategory::from(error_type);
    let is_recoverable = category.is_retryable()
        || matches!(
            error_type,
            ToolErrorType::InvalidParameters
                | ToolErrorType::PermissionDenied
                | ToolErrorType::ResourceNotFound
        );
    (is_recoverable, category.recovery_suggestions())
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
}
