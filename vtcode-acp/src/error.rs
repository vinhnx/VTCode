//! Error types for ACP operations

/// Result type for ACP operations
pub type AcpResult<T> = Result<T, AcpError>;

/// Errors that can occur during ACP communication
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    /// Agent not found or unavailable
    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    /// Network/HTTP error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Message serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Invalid request format
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Remote agent returned an error
    #[error("Remote error from {agent_id}: {message}{code}", code = if let Some(code) = code { format!(" (code: {})", code) } else { String::new() })]
    RemoteError {
        agent_id: String,
        message: String,
        code: Option<i32>,
    },

    /// Request timeout
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Generic internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<reqwest::Error> for AcpError {
    fn from(err: reqwest::Error) -> Self {
        AcpError::NetworkError(err.to_string())
    }
}

impl From<serde_json::Error> for AcpError {
    fn from(err: serde_json::Error) -> Self {
        AcpError::SerializationError(err.to_string())
    }
}

impl From<anyhow::Error> for AcpError {
    fn from(err: anyhow::Error) -> Self {
        AcpError::Internal(err.to_string())
    }
}
