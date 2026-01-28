//! Structured error handling for Open Responses.
//!
//! Provides consistent error objects with type, code, param, and message
//! fields as defined by the Open Responses specification.

use serde::{Deserialize, Serialize};

/// Structured error object for Open Responses.
///
/// Errors are designed to provide clear, actionable feedback with
/// machine-readable types and codes alongside human-readable messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenResponseError {
    /// Category of the error.
    #[serde(rename = "type")]
    pub error_type: OpenResponseErrorType,

    /// Machine-readable error code providing additional detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<OpenResponseErrorCode>,

    /// The input parameter related to the error, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,

    /// Human-readable explanation of what went wrong.
    pub message: String,
}

impl OpenResponseError {
    /// Creates a new error with the given type and message.
    pub fn new(error_type: OpenResponseErrorType, message: impl Into<String>) -> Self {
        Self {
            error_type,
            code: None,
            param: None,
            message: message.into(),
        }
    }

    /// Creates a server error.
    pub fn server_error(message: impl Into<String>) -> Self {
        Self::new(OpenResponseErrorType::ServerError, message)
    }

    /// Creates an invalid request error.
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(OpenResponseErrorType::InvalidRequest, message)
    }

    /// Creates an invalid request error with a parameter reference.
    pub fn invalid_param(param: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error_type: OpenResponseErrorType::InvalidRequest,
            code: None,
            param: Some(param.into()),
            message: message.into(),
        }
    }

    /// Creates a model error.
    pub fn model_error(message: impl Into<String>) -> Self {
        Self::new(OpenResponseErrorType::ModelError, message)
    }

    /// Creates a not found error.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(OpenResponseErrorType::NotFound, message)
    }

    /// Creates a rate limit error.
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::new(OpenResponseErrorType::TooManyRequests, message)
    }

    /// Sets the error code.
    pub fn with_code(mut self, code: OpenResponseErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Sets the parameter reference.
    pub fn with_param(mut self, param: impl Into<String>) -> Self {
        self.param = Some(param.into());
        self
    }
}

impl std::fmt::Display for OpenResponseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.error_type, self.message)
    }
}

impl std::error::Error for OpenResponseError {}

/// Category of error that occurred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenResponseErrorType {
    /// Internal server error.
    ServerError,

    /// Invalid request parameters.
    InvalidRequest,

    /// Resource not found.
    NotFound,

    /// Model-specific error.
    ModelError,

    /// Rate limit exceeded.
    TooManyRequests,

    /// Authentication error.
    AuthenticationError,

    /// Permission denied.
    PermissionDenied,
}

impl std::fmt::Display for OpenResponseErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServerError => write!(f, "server_error"),
            Self::InvalidRequest => write!(f, "invalid_request"),
            Self::NotFound => write!(f, "not_found"),
            Self::ModelError => write!(f, "model_error"),
            Self::TooManyRequests => write!(f, "too_many_requests"),
            Self::AuthenticationError => write!(f, "authentication_error"),
            Self::PermissionDenied => write!(f, "permission_denied"),
        }
    }
}

/// Specific error codes providing additional detail.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenResponseErrorCode {
    /// Invalid API key.
    InvalidApiKey,

    /// Insufficient quota.
    InsufficientQuota,

    /// Context length exceeded.
    ContextLengthExceeded,

    /// Invalid model.
    InvalidModel,

    /// Content filter triggered.
    ContentFilter,

    /// Tool execution failed.
    ToolExecutionFailed,

    /// Timeout occurred.
    Timeout,

    /// Rate limit exceeded.
    RateLimitExceeded,

    /// Provider-specific error code.
    #[serde(untagged)]
    Custom(String),
}

impl std::fmt::Display for OpenResponseErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidApiKey => write!(f, "invalid_api_key"),
            Self::InsufficientQuota => write!(f, "insufficient_quota"),
            Self::ContextLengthExceeded => write!(f, "context_length_exceeded"),
            Self::InvalidModel => write!(f, "invalid_model"),
            Self::ContentFilter => write!(f, "content_filter"),
            Self::ToolExecutionFailed => write!(f, "tool_execution_failed"),
            Self::Timeout => write!(f, "timeout"),
            Self::RateLimitExceeded => write!(f, "rate_limit_exceeded"),
            Self::Custom(code) => write!(f, "{}", code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = OpenResponseError::invalid_param("model", "Invalid model ID");
        assert_eq!(err.error_type, OpenResponseErrorType::InvalidRequest);
        assert_eq!(err.param, Some("model".to_string()));
        assert_eq!(err.message, "Invalid model ID");
    }

    #[test]
    fn test_error_serialization() {
        let err = OpenResponseError::server_error("Internal error")
            .with_code(OpenResponseErrorCode::Timeout);
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"type\":\"server_error\""));
        assert!(json.contains("\"code\":\"timeout\""));
    }
}
