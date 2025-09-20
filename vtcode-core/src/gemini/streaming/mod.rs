pub mod errors;
pub mod processor;

pub use errors::StreamingError;
pub use processor::{StreamingConfig, StreamingProcessor};

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Streaming metrics for monitoring and debugging
#[derive(Debug, Clone, Default)]
pub struct StreamingMetrics {
    pub request_start_time: Option<Instant>,
    pub first_chunk_time: Option<Instant>,
    pub total_chunks: usize,
    pub total_bytes: usize,
    pub total_requests: usize,
    pub total_response_time: Duration,
    pub error_count: usize,
    pub retry_count: usize,
}

/// Streaming response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingResponse {
    pub candidates: Vec<StreamingCandidate>,
    pub usage_metadata: Option<serde_json::Value>,
}

/// Streaming candidate structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingCandidate {
    pub content: crate::gemini::models::Content,
    pub finish_reason: Option<String>,
    pub index: Option<usize>,
}
