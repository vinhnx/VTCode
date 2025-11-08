//! Error types for ACP operations

use std::fmt;

/// Result type for ACP operations
pub type AcpResult<T> = std::result::Result<T, AcpError>;

/// Errors that can occur during ACP communication
#[derive(Debug)]
pub enum AcpError {
    /// Agent not found or unavailable
    AgentNotFound(String),

    /// Network/HTTP error
    NetworkError(String),

    /// Message serialization/deserialization error
    SerializationError(String),

    /// Invalid request format
    InvalidRequest(String),

    /// Remote agent returned an error
    RemoteError {
        agent_id: String,
        message: String,
        code: Option<i32>,
    },

    /// Request timeout
    Timeout(String),

    /// Configuration error
    ConfigError(String),

    /// Generic internal error
    Internal(String),
}

impl fmt::Display for AcpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AcpError::AgentNotFound(id) => write!(f, "Agent not found: {}", id),
            AcpError::NetworkError(e) => write!(f, "Network error: {}", e),
            AcpError::SerializationError(e) => write!(f, "Serialization error: {}", e),
            AcpError::InvalidRequest(e) => write!(f, "Invalid request: {}", e),
            AcpError::RemoteError {
                agent_id,
                message,
                code,
            } => {
                write!(f, "Remote error from {}: {}", agent_id, message)?;
                if let Some(code) = code {
                    write!(f, " (code: {})", code)?;
                }
                Ok(())
            }
            AcpError::Timeout(e) => write!(f, "Timeout: {}", e),
            AcpError::ConfigError(e) => write!(f, "Configuration error: {}", e),
            AcpError::Internal(e) => write!(f, "Internal error: {}", e),
        }
    }
}

impl std::error::Error for AcpError {}

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
