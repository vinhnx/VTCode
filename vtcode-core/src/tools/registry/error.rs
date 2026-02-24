use anyhow::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;

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

pub fn classify_error(error: &Error) -> ToolErrorType {
    let error_msg = error.to_string().to_ascii_lowercase();

    let policy_markers = [
        "policy violation",
        "denied by policy",
        "tool permission denied",
        "safety validation failed",
        "not allowed in plan mode",
        "only available when plan mode is active",
        "workspace boundary",
        "blocked by policy",
    ];
    if contains_any(&error_msg, &policy_markers) {
        return ToolErrorType::PolicyViolation;
    }

    let invalid_markers = [
        "invalid argument",
        "invalid parameters",
        "malformed",
        "missing required",
        "schema validation",
        "argument validation failed",
        "unknown field",
        "type mismatch",
    ];
    if contains_any(&error_msg, &invalid_markers) {
        return ToolErrorType::InvalidParameters;
    }

    let tool_not_found_markers = [
        "tool not found",
        "unknown tool",
        "unsupported tool",
        "no such tool",
    ];
    if contains_any(&error_msg, &tool_not_found_markers) {
        return ToolErrorType::ToolNotFound;
    }

    let resource_not_found_markers = [
        "no such file",
        "no such directory",
        "file not found",
        "directory not found",
        "resource not found",
        "path not found",
        "enoent",
    ];
    if contains_any(&error_msg, &resource_not_found_markers) {
        return ToolErrorType::ResourceNotFound;
    }

    let permission_markers = [
        "permission denied",
        "access denied",
        "operation not permitted",
        "eacces",
        "eperm",
    ];
    if contains_any(&error_msg, &permission_markers) {
        return ToolErrorType::PermissionDenied;
    }

    let timeout_markers = ["timeout", "timed out", "deadline exceeded"];
    if contains_any(&error_msg, &timeout_markers) {
        return ToolErrorType::Timeout;
    }

    let non_retryable_limit_markers = [
        "weekly usage limit",
        "daily usage limit",
        "monthly spending limit",
        "insufficient credits",
        "quota exceeded",
        "billing",
        "payment required",
    ];
    if contains_any(&error_msg, &non_retryable_limit_markers) {
        return ToolErrorType::ExecutionError;
    }

    let network_markers = [
        "network",
        "connection",
        "connection reset",
        "connection refused",
        "broken pipe",
        "dns",
        "name resolution",
        "temporary failure in name resolution",
        "service unavailable",
        "temporarily unavailable",
        "internal server error",
        "bad gateway",
        "gateway timeout",
        "rate limit",
        "too many requests",
        "429",
        "500",
        "502",
        "503",
        "504",
        "upstream connect error",
        "tls handshake",
        "socket hang up",
        "econnreset",
        "etimedout",
    ];
    if contains_any(&error_msg, &network_markers) {
        return ToolErrorType::NetworkError;
    }

    ToolErrorType::ExecutionError
}

#[inline]
fn contains_any(message: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| message.contains(marker))
}

// Use static string slices to avoid allocations for recovery suggestions
#[inline]
fn generate_recovery_info(error_type: ToolErrorType) -> (bool, Vec<Cow<'static, str>>) {
    match error_type {
        ToolErrorType::InvalidParameters => (
            true,
            vec![
                Cow::Borrowed("Check parameter names and types against the tool schema"),
                Cow::Borrowed("Ensure required parameters are provided"),
                Cow::Borrowed("Verify parameter values are within acceptable ranges"),
            ],
        ),
        ToolErrorType::ToolNotFound => (
            false,
            vec![
                Cow::Borrowed("Verify the tool name is spelled correctly"),
                Cow::Borrowed("Check if the tool is available in the current context"),
                Cow::Borrowed("Contact administrator if tool should be available"),
            ],
        ),
        ToolErrorType::PermissionDenied => (
            true,
            vec![
                Cow::Borrowed("Check file permissions and access rights"),
                Cow::Borrowed("Ensure workspace boundaries are respected"),
                Cow::Borrowed("Try running with appropriate permissions"),
            ],
        ),
        ToolErrorType::ResourceNotFound => (
            true,
            vec![
                Cow::Borrowed("Verify file paths and resource locations"),
                Cow::Borrowed("Check if files exist and are accessible"),
                Cow::Borrowed("Use list_dir to explore available resources"),
            ],
        ),
        ToolErrorType::NetworkError => (
            true,
            vec![
                Cow::Borrowed("Check network connectivity"),
                Cow::Borrowed("Retry the operation after a brief delay"),
                Cow::Borrowed("Verify external service availability"),
            ],
        ),
        ToolErrorType::Timeout => (
            true,
            vec![
                Cow::Borrowed("Increase timeout values if appropriate"),
                Cow::Borrowed("Break large operations into smaller chunks"),
                Cow::Borrowed("Check system resources and performance"),
            ],
        ),
        ToolErrorType::ExecutionError => (
            false,
            vec![
                Cow::Borrowed("Review error details for specific issues"),
                Cow::Borrowed("Check tool documentation for known limitations"),
                Cow::Borrowed("Report the issue if it appears to be a bug"),
            ],
        ),
        ToolErrorType::PolicyViolation => (
            false,
            vec![
                Cow::Borrowed("Review workspace policies and restrictions"),
                Cow::Borrowed("Contact administrator for policy changes"),
                Cow::Borrowed("Use alternative tools that comply with policies"),
            ],
        ),
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
