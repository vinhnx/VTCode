use anyhow::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;

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
    let error_msg = error.to_string().to_lowercase();

    if error_msg.contains("permission") || error_msg.contains("access denied") {
        ToolErrorType::PermissionDenied
    } else if error_msg.contains("not found") || error_msg.contains("no such file") {
        ToolErrorType::ResourceNotFound
    } else if error_msg.contains("timeout") || error_msg.contains("timed out") {
        ToolErrorType::Timeout
    } else if error_msg.contains("network") || error_msg.contains("connection") {
        ToolErrorType::NetworkError
    } else if error_msg.contains("invalid") || error_msg.contains("malformed") {
        ToolErrorType::InvalidParameters
    } else if error_msg.contains("policy") || error_msg.contains("denied") {
        ToolErrorType::PolicyViolation
    } else {
        ToolErrorType::ExecutionError
    }
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
