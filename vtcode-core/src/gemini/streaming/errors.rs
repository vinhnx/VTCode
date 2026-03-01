use std::time::Duration;

/// Streaming error types for better error classification and handling
#[derive(Debug, Clone, thiserror::Error)]
pub enum StreamingError {
    /// Network-related errors (connection, timeout, DNS, etc.)
    #[error("Network error: {message}")]
    NetworkError { message: String, is_retryable: bool },
    /// API-related errors (rate limits, authentication, etc.)
    #[error("API error ({status_code}): {message}")]
    ApiError {
        status_code: u16,
        message: String,
        is_retryable: bool,
    },
    /// Response parsing errors
    #[error("Parse error: {message}")]
    ParseError {
        message: String,
        raw_response: String,
    },
    /// Timeout errors
    #[error("Timeout during {operation} after {duration:?}")]
    TimeoutError {
        operation: String,
        duration: Duration,
    },
    /// Content validation errors
    #[error("Content error: {message}")]
    ContentError { message: String },
    /// Streaming-specific errors
    #[error("Streaming error: {message}")]
    StreamingError {
        message: String,
        partial_content: Option<String>,
    },
}
