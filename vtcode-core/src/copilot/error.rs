//! Error types for ACP operations used by the Copilot stdio transport.

/// Result type for ACP operations.
pub(super) type AcpResult<T> = Result<T, AcpError>;

/// Errors that can occur during ACP communication.
#[derive(Debug, thiserror::Error)]
pub(super) enum AcpError {
    /// Message serialization/deserialization error.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Remote agent returned an error.
    #[error("Remote error from {agent_id}: {message}{code}", code = if let Some(code) = code { format!(" (code: {})", code) } else { String::new() })]
    RemoteError {
        agent_id: String,
        message: String,
        code: Option<i32>,
    },

    /// Request timeout.
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Generic internal error.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<serde_json::Error> for AcpError {
    fn from(err: serde_json::Error) -> Self {
        AcpError::SerializationError(err.to_string())
    }
}
