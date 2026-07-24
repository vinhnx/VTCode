pub mod errors;
pub mod processor;

pub use errors::StreamingError;
pub use processor::{StreamingConfig, StreamingProcessor};

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Streaming metrics for monitoring and debugging
#[derive(Debug, Clone, Default)]
pub struct StreamingMetrics {
    request_start_time: Option<Instant>,
    first_chunk_time: Option<Instant>,
    total_chunks: usize,
    total_bytes: usize,
    pub(crate) total_requests: usize,
    pub(crate) total_response_time: Duration,
    error_count: usize,
    retry_count: usize,
}

/// Streaming response structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingResponse {
    pub(crate) candidates: Vec<StreamingCandidate>,
    pub(crate) usage_metadata: Option<serde_json::Value>,
}

/// Streaming candidate structure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingCandidate {
    pub(crate) content: super::models::Content,
    pub(crate) finish_reason: Option<String>,
    pub(crate) index: Option<usize>,
}
