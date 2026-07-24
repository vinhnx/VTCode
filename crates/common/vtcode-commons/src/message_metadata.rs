//! Per-message metadata for conversation history.
//!
//! Each message in the conversation carries metadata about its origin,
//! importance, compression state, and resource usage. This enables smart
//! context pruning (drop low-importance messages first), compression
//! tracking, and latency analysis.
//!
//! Following the "state as a first-class citizen" principle (Hitchhiker's
//! Guide to Agentic AI, Section 18.6.1), metadata is the foundation for
//! conversation state quality-of-service decisions.

use serde::{Deserialize, Serialize};

/// Metadata attached to every message in the conversation history.
///
/// Skipped during serialization when `None` to preserve backward compatibility
/// with all existing persistence formats (session archives, snapshots, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageMetadata {
    /// Unix millisecond timestamp when the message was created.
    timestamp: u64,

    /// Importance score in [0.0, 1.0]: 0.0 = low (safe to drop first),
    /// 1.0 = high (preserve as long as possible).
    ///
    /// Initialised to 0.5 (neutral) and adjusted by the compression/pruning
    /// system or by explicit agent reflection.
    importance_score: f64,

    /// Current compression status of this message.
    compression_status: CompressionStatus,

    /// Cached token estimate for this message. Populated on creation and
    /// updated after compression.
    estimated_tokens: usize,

    /// Origin of this message: "user_input", "llm_response", "tool_result",
    /// "system", or "synthetic".
    source: Option<String>,
}

impl MessageMetadata {
    /// Create metadata for a message originating from a user.
    pub fn user_input(timestamp: u64, estimated_tokens: usize) -> Self {
        Self {
            timestamp,
            importance_score: 0.5,
            compression_status: CompressionStatus::Uncompressed,
            estimated_tokens,
            source: Some("user_input".into()),
        }
    }

    /// Create metadata for a message originating from an LLM response.
    pub fn llm_response(timestamp: u64, estimated_tokens: usize) -> Self {
        Self {
            timestamp,
            importance_score: 0.6,
            compression_status: CompressionStatus::Uncompressed,
            estimated_tokens,
            source: Some("llm_response".into()),
        }
    }

    /// Create metadata for a tool result message.
    pub fn tool_result(timestamp: u64, estimated_tokens: usize) -> Self {
        Self {
            timestamp,
            importance_score: 0.4,
            compression_status: CompressionStatus::Uncompressed,
            estimated_tokens,
            source: Some("tool_result".into()),
        }
    }

    /// Create metadata for a system message.
    pub fn system(timestamp: u64, estimated_tokens: usize) -> Self {
        Self {
            timestamp,
            importance_score: 1.0,
            compression_status: CompressionStatus::Uncompressed,
            estimated_tokens,
            source: Some("system".into()),
        }
    }

    /// Create metadata for a synthetic (e.g., recovery/injected) message.
    pub fn synthetic(timestamp: u64, estimated_tokens: usize) -> Self {
        Self {
            timestamp,
            importance_score: 0.3,
            compression_status: CompressionStatus::Uncompressed,
            estimated_tokens,
            source: Some("synthetic".into()),
        }
    }

    /// Mark this message as compressed, recording the original and new token counts.
    fn mark_compressed(&mut self, original_tokens: usize, compressed_tokens: usize) {
        self.compression_status = CompressionStatus::Compressed {
            original_token_count: original_tokens,
            summary_token_count: compressed_tokens,
        };
        self.estimated_tokens = compressed_tokens;
    }

    /// Mark this message as summarized.
    fn mark_summarized(&mut self, original_tokens: usize, summary_tokens: usize) {
        self.compression_status = CompressionStatus::Summarized {
            original_token_count: original_tokens,
            summary_token_count: summary_tokens,
        };
        self.estimated_tokens = summary_tokens;
    }

    /// Set the importance score (clamped to [0.0, 1.0]).
    fn set_importance(&mut self, score: f64) {
        self.importance_score = score.clamp(0.0, 1.0);
    }

    /// Returns the original (pre-compression) token count, or the current count
    /// if the message was never compressed.
    fn original_token_count(&self) -> usize {
        match self.compression_status {
            CompressionStatus::Uncompressed => self.estimated_tokens,
            CompressionStatus::Compressed { original_token_count, .. }
            | CompressionStatus::Summarized { original_token_count, .. } => original_token_count,
            CompressionStatus::Dropped => 0,
        }
    }

    /// Returns the effective (post-compression) token count.
    fn effective_token_count(&self) -> usize {
        match self.compression_status {
            CompressionStatus::Uncompressed => self.estimated_tokens,
            CompressionStatus::Compressed { summary_token_count, .. }
            | CompressionStatus::Summarized { summary_token_count, .. } => summary_token_count,
            CompressionStatus::Dropped => 0,
        }
    }
}

/// Tracks the compression state of a single message in conversation history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionStatus {
    /// Message is in its original uncompressed form.
    Uncompressed,
    /// Message has been compressed with token-level preservation of information.
    Compressed {
        original_token_count: usize,
        summary_token_count: usize,
    },
    /// Message has been semantically summarized (lossy compression).
    Summarized {
        original_token_count: usize,
        summary_token_count: usize,
    },
    /// Message has been dropped from the active context but may be in long-term
    /// memory.
    Dropped,
}

#[allow(clippy::derivable_impls)]
impl Default for CompressionStatus {
    fn default() -> Self {
        Self::Uncompressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_user_metadata() {
        let meta = MessageMetadata::user_input(1000, 50);
        assert_eq!(meta.timestamp, 1000);
        assert!((meta.importance_score - 0.5).abs() < f64::EPSILON);
        assert_eq!(meta.compression_status, CompressionStatus::Uncompressed);
        assert_eq!(meta.estimated_tokens, 50);
        assert_eq!(meta.source.as_deref(), Some("user_input"));
    }

    #[test]
    fn test_create_llm_response_metadata() {
        let meta = MessageMetadata::llm_response(2000, 150);
        assert!((meta.importance_score - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mark_compressed() {
        let mut meta = MessageMetadata::user_input(1000, 200);
        meta.mark_compressed(200, 50);
        assert_eq!(meta.estimated_tokens, 50);
        assert_eq!(meta.effective_token_count(), 50);
        assert_eq!(meta.original_token_count(), 200);
    }

    #[test]
    fn test_mark_summarized() {
        let mut meta = MessageMetadata::user_input(1000, 300);
        meta.mark_summarized(300, 30);
        assert_eq!(meta.effective_token_count(), 30);
        assert_eq!(meta.original_token_count(), 300);
    }

    #[test]
    fn test_set_importance_clamps() {
        let mut meta = MessageMetadata::user_input(1000, 50);
        meta.set_importance(1.5);
        assert!((meta.importance_score - 1.0).abs() < f64::EPSILON);
        meta.set_importance(-0.5);
        assert!((meta.importance_score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compression_status_serde_roundtrip() {
        let status = CompressionStatus::Compressed { original_token_count: 200, summary_token_count: 50 };
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: CompressionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}
