//! Streaming buffer for batched output rendering
//!
//! Optimizes large output rendering by batching inline segments and flushing
//! in configurable batches rather than line-by-line, reducing overhead.

use crate::ui::tui::{InlineMessageKind, InlineSegment};

/// Configuration for streaming behavior
#[derive(Clone, Debug)]
pub struct StreamConfig {
    /// Number of lines to buffer before automatic flush
    pub batch_size: usize,
    /// Maximum buffer size before forced flush (bytes)
    pub max_buffer_bytes: usize,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            batch_size: 20,      // Flush every 20 lines
            max_buffer_bytes: 65536, // 64KB max buffer
        }
    }
}

/// Streaming buffer that batches output before rendering
#[derive(Debug)]
pub struct StreamBuffer {
    /// Buffered line segments
    lines: Vec<Vec<InlineSegment>>,
    /// Configuration for batching behavior
    config: StreamConfig,
    /// Approximate size in bytes (for max_buffer_bytes check)
    approximate_size: usize,
}

impl StreamBuffer {
    /// Create a new streaming buffer with default configuration
    pub fn new() -> Self {
        Self::with_config(StreamConfig::default())
    }

    /// Create a streaming buffer with custom configuration
    pub fn with_config(config: StreamConfig) -> Self {
        Self {
            lines: Vec::with_capacity(config.batch_size),
            config,
            approximate_size: 0,
        }
    }

    /// Add a line of segments to the buffer
    pub fn append_line(&mut self, segments: Vec<InlineSegment>) -> bool {
        // Calculate approximate size: sum of all segment text lengths
        let line_size: usize = segments.iter().map(|s| s.text.len()).sum();
        self.approximate_size += line_size;
        self.lines.push(segments);

        // Check if we should flush
        self.should_flush()
    }

    /// Check if buffer should be flushed
    fn should_flush(&self) -> bool {
        self.lines.len() >= self.config.batch_size
            || self.approximate_size >= self.config.max_buffer_bytes
    }

    /// Get buffered lines and clear buffer
    pub fn flush(&mut self) -> Vec<Vec<InlineSegment>> {
        self.approximate_size = 0;
        std::mem::take(&mut self.lines)
    }

    /// Get current buffer size (number of lines)
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get approximate bytes in buffer
    pub fn approximate_bytes(&self) -> usize {
        self.approximate_size
    }

    /// Force flush regardless of batch size
    pub fn force_flush(&mut self) -> Vec<Vec<InlineSegment>> {
        self.flush()
    }

    /// Clear buffer without returning contents
    pub fn clear(&mut self) {
        self.lines.clear();
        self.approximate_size = 0;
    }
}

impl Default for StreamBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming context for rendering multiple lines with metadata
#[derive(Debug)]
pub struct StreamingContext {
    /// Output message kind for all lines in this stream
    pub kind: InlineMessageKind,
    /// Buffer for accumulating output
    pub buffer: StreamBuffer,
    /// Total lines rendered so far
    pub total_lines: usize,
}

impl StreamingContext {
    /// Create a new streaming context
    pub fn new(kind: InlineMessageKind) -> Self {
        Self {
            kind,
            buffer: StreamBuffer::new(),
            total_lines: 0,
        }
    }

    /// Create with custom buffer configuration
    pub fn with_config(kind: InlineMessageKind, config: StreamConfig) -> Self {
        Self {
            kind,
            buffer: StreamBuffer::with_config(config),
            total_lines: 0,
        }
    }

    /// Add a line and track total
    pub fn append(&mut self, segments: Vec<InlineSegment>) -> bool {
        let should_flush = self.buffer.append_line(segments);
        self.total_lines += 1;
        should_flush
    }

    /// Get flushed lines and update tracking
    pub fn flush(&mut self) -> Vec<Vec<InlineSegment>> {
        self.buffer.flush()
    }
}

/// Predicts memory requirements for rendering markdown
pub struct AllocationPredictor {
    /// Estimated bytes per average line
    bytes_per_line: usize,
}

impl AllocationPredictor {
    /// Create predictor with default estimates
    pub fn new() -> Self {
        Self {
            bytes_per_line: 120, // Average terminal line content
        }
    }

    /// Estimate total bytes needed for N lines
    pub fn estimate_total_bytes(&self, line_count: usize) -> usize {
        line_count * self.bytes_per_line
    }

    /// Estimate optimal batch size for given document size
    pub fn optimal_batch_size(&self, _total_bytes: usize) -> usize {
        // Batches of roughly 8KB (can be tuned)
        let target_batch_bytes = 8192;
        let batch_lines = (target_batch_bytes / self.bytes_per_line).max(5);
        batch_lines.min(50) // Cap at 50 lines per batch
    }

    /// Predict pre-allocation size for markdown rendering
    pub fn pre_allocation_capacity(&self, estimated_lines: usize) -> usize {
        (estimated_lines as f64 * 1.2) as usize // 20% headroom
    }
}

impl Default for AllocationPredictor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_buffer_creation() {
        let buffer = StreamBuffer::new();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_stream_buffer_append() {
        let mut buffer = StreamBuffer::new();
        let segment = InlineSegment {
            text: "test".to_string(),
            style: std::sync::Arc::new(Default::default()),
        };
        let should_flush = buffer.append_line(vec![segment]);
        assert!(!should_flush); // Default batch size is 20
        assert_eq!(buffer.len(), 1);
    }

    #[test]
    fn test_stream_buffer_batch_flush() {
        let mut buffer = StreamBuffer::with_config(StreamConfig {
            batch_size: 5,
            max_buffer_bytes: usize::MAX,
        });
        
        for i in 0..5 {
            let segment = InlineSegment {
                text: format!("line {}", i),
                style: std::sync::Arc::new(Default::default()),
            };
            let should_flush = buffer.append_line(vec![segment]);
            if i < 4 {
                assert!(!should_flush);
            } else {
                assert!(should_flush);
            }
        }
        assert_eq!(buffer.len(), 5);
    }

    #[test]
    fn test_stream_buffer_byte_limit_flush() {
        let mut buffer = StreamBuffer::with_config(StreamConfig {
            batch_size: 100,
            max_buffer_bytes: 50,
        });
        
        let segment = InlineSegment {
            text: "x".repeat(60),
            style: std::sync::Arc::new(Default::default()),
        };
        let should_flush = buffer.append_line(vec![segment]);
        assert!(should_flush);
    }

    #[test]
    fn test_stream_buffer_flush_returns_lines() {
        let mut buffer = StreamBuffer::new();
        let segment = InlineSegment {
            text: "test".to_string(),
            style: std::sync::Arc::new(Default::default()),
        };
        buffer.append_line(vec![segment]);
        
        let flushed = buffer.flush();
        assert_eq!(flushed.len(), 1);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_streaming_context() {
        let mut ctx = StreamingContext::new(InlineMessageKind::Agent);
        assert_eq!(ctx.total_lines, 0);
        
        let segment = InlineSegment {
            text: "test".to_string(),
            style: std::sync::Arc::new(Default::default()),
        };
        ctx.append(vec![segment]);
        assert_eq!(ctx.total_lines, 1);
    }

    #[test]
    fn test_allocation_predictor() {
        let predictor = AllocationPredictor::new();
        let estimate = predictor.estimate_total_bytes(100);
        assert!(estimate > 0);
        
        let batch = predictor.optimal_batch_size(10000);
        assert!(batch > 0 && batch <= 50);
    }

    #[test]
    fn test_stream_config_defaults() {
        let config = StreamConfig::default();
        assert_eq!(config.batch_size, 20);
        assert_eq!(config.max_buffer_bytes, 65536);
    }

    #[test]
    fn test_pre_allocation_capacity() {
        let predictor = AllocationPredictor::new();
        let capacity = predictor.pre_allocation_capacity(100);
        assert!(capacity >= 100); // At least original size
        assert!(capacity <= 120); // With 20% headroom
    }
}
