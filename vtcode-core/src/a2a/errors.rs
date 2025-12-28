//! A2A Protocol error types and error codes
//!
//! Implements both standard JSON-RPC 2.0 error codes and A2A-specific error codes
//! as defined in the A2A specification.

use std::fmt;
use thiserror::Error;

// ============================================================================
// Standard JSON-RPC 2.0 Error Codes
// ============================================================================

/// Parse error - Invalid JSON was received
pub const JSON_PARSE_ERROR: i32 = -32700;

/// Invalid Request - The JSON sent is not a valid Request object
pub const INVALID_REQUEST_ERROR: i32 = -32600;

/// Method not found - The method does not exist / is not available
pub const METHOD_NOT_FOUND_ERROR: i32 = -32601;

/// Invalid params - Invalid method parameters
pub const INVALID_PARAMS_ERROR: i32 = -32602;

/// Internal error - Internal JSON-RPC error
pub const INTERNAL_ERROR: i32 = -32603;

// ============================================================================
// A2A-Specific Error Codes
// ============================================================================

/// Task not found - The specified task does not exist
pub const TASK_NOT_FOUND_ERROR: i32 = -32001;

/// Task not cancelable - The task cannot be canceled in its current state
pub const TASK_NOT_CANCELABLE_ERROR: i32 = -32002;

/// Push notifications not supported - The agent does not support push notifications
pub const PUSH_NOTIFICATION_NOT_SUPPORTED_ERROR: i32 = -32003;

/// Unsupported operation - The requested operation is not supported
pub const UNSUPPORTED_OPERATION_ERROR: i32 = -32004;

/// Content type not supported - The content type is not supported
pub const CONTENT_TYPE_NOT_SUPPORTED_ERROR: i32 = -32005;

/// Invalid agent response - The agent returned an invalid response
pub const INVALID_AGENT_RESPONSE_ERROR: i32 = -32006;

/// Authenticated extended card not configured
pub const AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED_ERROR: i32 = -32007;

// ============================================================================
// Error Types
// ============================================================================

/// A2A error code enum for type-safe error handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A2aErrorCode {
    // Standard JSON-RPC errors
    JsonParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    InternalError,
    // A2A-specific errors
    TaskNotFound,
    TaskNotCancelable,
    PushNotificationNotSupported,
    UnsupportedOperation,
    ContentTypeNotSupported,
    InvalidAgentResponse,
    AuthenticatedExtendedCardNotConfigured,
    /// Custom error code
    Custom(i32),
}

impl From<A2aErrorCode> for i32 {
    fn from(code: A2aErrorCode) -> Self {
        match code {
            A2aErrorCode::JsonParseError => JSON_PARSE_ERROR,
            A2aErrorCode::InvalidRequest => INVALID_REQUEST_ERROR,
            A2aErrorCode::MethodNotFound => METHOD_NOT_FOUND_ERROR,
            A2aErrorCode::InvalidParams => INVALID_PARAMS_ERROR,
            A2aErrorCode::InternalError => INTERNAL_ERROR,
            A2aErrorCode::TaskNotFound => TASK_NOT_FOUND_ERROR,
            A2aErrorCode::TaskNotCancelable => TASK_NOT_CANCELABLE_ERROR,
            A2aErrorCode::PushNotificationNotSupported => PUSH_NOTIFICATION_NOT_SUPPORTED_ERROR,
            A2aErrorCode::UnsupportedOperation => UNSUPPORTED_OPERATION_ERROR,
            A2aErrorCode::ContentTypeNotSupported => CONTENT_TYPE_NOT_SUPPORTED_ERROR,
            A2aErrorCode::InvalidAgentResponse => INVALID_AGENT_RESPONSE_ERROR,
            A2aErrorCode::AuthenticatedExtendedCardNotConfigured => {
                AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED_ERROR
            }
            A2aErrorCode::Custom(code) => code,
        }
    }
}

impl From<i32> for A2aErrorCode {
    fn from(code: i32) -> Self {
        match code {
            JSON_PARSE_ERROR => A2aErrorCode::JsonParseError,
            INVALID_REQUEST_ERROR => A2aErrorCode::InvalidRequest,
            METHOD_NOT_FOUND_ERROR => A2aErrorCode::MethodNotFound,
            INVALID_PARAMS_ERROR => A2aErrorCode::InvalidParams,
            INTERNAL_ERROR => A2aErrorCode::InternalError,
            TASK_NOT_FOUND_ERROR => A2aErrorCode::TaskNotFound,
            TASK_NOT_CANCELABLE_ERROR => A2aErrorCode::TaskNotCancelable,
            PUSH_NOTIFICATION_NOT_SUPPORTED_ERROR => A2aErrorCode::PushNotificationNotSupported,
            UNSUPPORTED_OPERATION_ERROR => A2aErrorCode::UnsupportedOperation,
            CONTENT_TYPE_NOT_SUPPORTED_ERROR => A2aErrorCode::ContentTypeNotSupported,
            INVALID_AGENT_RESPONSE_ERROR => A2aErrorCode::InvalidAgentResponse,
            AUTHENTICATED_EXTENDED_CARD_NOT_CONFIGURED_ERROR => {
                A2aErrorCode::AuthenticatedExtendedCardNotConfigured
            }
            other => A2aErrorCode::Custom(other),
        }
    }
}

impl fmt::Display for A2aErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            A2aErrorCode::JsonParseError => write!(f, "JSON parse error"),
            A2aErrorCode::InvalidRequest => write!(f, "Invalid request"),
            A2aErrorCode::MethodNotFound => write!(f, "Method not found"),
            A2aErrorCode::InvalidParams => write!(f, "Invalid params"),
            A2aErrorCode::InternalError => write!(f, "Internal error"),
            A2aErrorCode::TaskNotFound => write!(f, "Task not found"),
            A2aErrorCode::TaskNotCancelable => write!(f, "Task not cancelable"),
            A2aErrorCode::PushNotificationNotSupported => {
                write!(f, "Push notifications not supported")
            }
            A2aErrorCode::UnsupportedOperation => write!(f, "Unsupported operation"),
            A2aErrorCode::ContentTypeNotSupported => write!(f, "Content type not supported"),
            A2aErrorCode::InvalidAgentResponse => write!(f, "Invalid agent response"),
            A2aErrorCode::AuthenticatedExtendedCardNotConfigured => {
                write!(f, "Authenticated extended card not configured")
            }
            A2aErrorCode::Custom(code) => write!(f, "Custom error ({code})"),
        }
    }
}

/// A2A protocol error
#[derive(Debug, Error)]
pub enum A2aError {
    #[error("JSON-RPC error ({code}): {message}")]
    RpcError {
        code: A2aErrorCode,
        message: String,
        #[source]
        data: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Task not cancelable: {0}")]
    TaskNotCancelable(String),

    #[error("Invalid task state transition: {from:?} -> {to:?}")]
    InvalidStateTransition {
        from: super::types::TaskState,
        to: super::types::TaskState,
    },

    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    #[error("Content type not supported: {0}")]
    ContentTypeNotSupported(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl A2aError {
    /// Get the error code for this error
    pub fn code(&self) -> A2aErrorCode {
        match self {
            A2aError::RpcError { code, .. } => *code,
            A2aError::TaskNotFound(_) => A2aErrorCode::TaskNotFound,
            A2aError::TaskNotCancelable(_) => A2aErrorCode::TaskNotCancelable,
            A2aError::InvalidStateTransition { .. } => A2aErrorCode::InvalidParams,
            A2aError::UnsupportedOperation(_) => A2aErrorCode::UnsupportedOperation,
            A2aError::ContentTypeNotSupported(_) => A2aErrorCode::ContentTypeNotSupported,
            A2aError::Serialization(_) => A2aErrorCode::JsonParseError,
            A2aError::Internal(_) => A2aErrorCode::InternalError,
        }
    }

    /// Create a new RPC error
    pub fn rpc(code: A2aErrorCode, message: impl Into<String>) -> Self {
        A2aError::RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Create a new internal error
    pub fn internal(message: impl Into<String>) -> Self {
        A2aError::Internal(message.into())
    }
}

/// A2A Result type alias
pub type A2aResult<T> = Result<T, A2aError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_conversion() {
        assert_eq!(i32::from(A2aErrorCode::TaskNotFound), TASK_NOT_FOUND_ERROR);
        assert_eq!(
            A2aErrorCode::from(TASK_NOT_FOUND_ERROR),
            A2aErrorCode::TaskNotFound
        );
    }

    #[test]
    fn test_error_code_display() {
        assert_eq!(A2aErrorCode::TaskNotFound.to_string(), "Task not found");
        assert_eq!(A2aErrorCode::MethodNotFound.to_string(), "Method not found");
    }

    #[test]
    fn test_a2a_error_code() {
        let err = A2aError::TaskNotFound("task-123".to_string());
        assert_eq!(err.code(), A2aErrorCode::TaskNotFound);
    }

    #[test]
    fn test_custom_error_code() {
        let custom = A2aErrorCode::Custom(-32099);
        assert_eq!(i32::from(custom), -32099);
        assert_eq!(custom.to_string(), "Custom error (-32099)");
    }
}
