//! Provider-agnostic streaming timeout progress tracking
//!
//! This module provides a unified interface for tracking streaming timeout progress
//! across all LLM providers (OpenAI, Anthropic, Gemini, Ollama, etc.)

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};
use tracing::warn;

/// Callback for streaming timeout progress updates
/// Progress value is 0.0-1.0 representing elapsed / total_timeout
pub type StreamingProgressCallback = Box<dyn Fn(f32) + Send + Sync>;

/// Unified streaming progress tracker for all LLM providers
#[derive(Clone)]
pub struct StreamingProgressTracker {
    callback: Option<Arc<StreamingProgressCallback>>,
    warning_threshold: f32,
    total_timeout: Duration,
    start_time: Arc<Instant>,
    last_reported_progress: Arc<AtomicU8>,
}

impl StreamingProgressTracker {
    /// Create a new streaming progress tracker
    pub fn new(total_timeout: Duration) -> Self {
        Self {
            callback: None,
            warning_threshold: 0.8,
            total_timeout,
            start_time: Arc::new(Instant::now()),
            last_reported_progress: Arc::new(AtomicU8::new(0)),
        }
    }

    /// Set a progress callback
    pub fn with_callback(mut self, callback: StreamingProgressCallback) -> Self {
        self.callback = Some(Arc::new(callback));
        self
    }

    /// Set the warning threshold (0.0-1.0)
    pub fn with_warning_threshold(mut self, threshold: f32) -> Self {
        self.warning_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Report that the first chunk has been received
    pub fn report_first_chunk(&self) {
        self.report_progress(0.1);
    }

    /// Report progress with elapsed time
    pub fn report_chunk_received(&self) {
        let elapsed = self.start_time.elapsed();
        self.report_progress_with_elapsed(elapsed);
    }

    /// Report progress at a specific elapsed duration
    pub fn report_progress_with_elapsed(&self, elapsed: Duration) {
        if self.total_timeout.as_secs() == 0 {
            return;
        }

        let progress = elapsed.as_secs_f32() / self.total_timeout.as_secs_f32();
        self.report_progress(progress.min(0.99)); // Cap at 99%
    }

    /// Report error or timeout (100% progress)
    pub fn report_error(&self) {
        self.report_progress(1.0);
    }

    /// Get current progress as percentage (0-100)
    pub fn progress_percent(&self) -> u8 {
        self.last_reported_progress.load(Ordering::Relaxed)
    }

    /// Get elapsed time since start
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Check if warning threshold has been exceeded
    pub fn is_approaching_timeout(&self) -> bool {
        let elapsed = self.start_time.elapsed();
        if self.total_timeout.as_secs() == 0 {
            return false;
        }

        let progress = elapsed.as_secs_f32() / self.total_timeout.as_secs_f32();
        progress >= self.warning_threshold
    }

    // Private: Report progress with clamping and threshold checking
    fn report_progress(&self, progress: f32) {
        let progress_clamped = progress.clamp(0.0, 1.0);
        let percent = (progress_clamped * 100.0) as u8;

        // Only update if progress changed by at least 1%
        let last_percent = self.last_reported_progress.load(Ordering::Relaxed);
        if percent <= last_percent {
            return;
        }

        self.last_reported_progress
            .store(percent, Ordering::Relaxed);

        // Call the callback if set
        if let Some(ref callback) = self.callback {
            callback(progress_clamped);
        }

        // Warn if approaching threshold
        if progress_clamped >= self.warning_threshold && progress_clamped < 1.0 {
            warn!(
                "Streaming operation at {:.0}% of timeout limit ({:?}/{:?} elapsed). Approaching timeout.",
                progress_clamped * 100.0,
                self.elapsed(),
                self.total_timeout
            );
        }
    }
}

/// Builder for creating streaming progress trackers with fluent API
pub struct StreamingProgressBuilder {
    total_timeout: Duration,
    callback: Option<StreamingProgressCallback>,
    warning_threshold: f32,
}

impl StreamingProgressBuilder {
    /// Create a new builder with total timeout in seconds
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            total_timeout: Duration::from_secs(timeout_secs),
            callback: None,
            warning_threshold: 0.8,
        }
    }

    /// Create a new builder with a specific duration
    pub fn with_duration(duration: Duration) -> Self {
        Self {
            total_timeout: duration,
            callback: None,
            warning_threshold: 0.8,
        }
    }

    /// Set the progress callback
    pub fn callback(mut self, callback: StreamingProgressCallback) -> Self {
        self.callback = Some(callback);
        self
    }

    /// Set the warning threshold (0.0-1.0)
    pub fn warning_threshold(mut self, threshold: f32) -> Self {
        self.warning_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Build the tracker
    pub fn build(self) -> StreamingProgressTracker {
        let mut tracker = StreamingProgressTracker::new(self.total_timeout);
        if let Some(callback) = self.callback {
            tracker.callback = Some(Arc::new(callback));
        }
        tracker.warning_threshold = self.warning_threshold;
        tracker
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn test_progress_tracker_creation() {
        let tracker = StreamingProgressTracker::new(Duration::from_secs(600));
        assert_eq!(tracker.progress_percent(), 0);
        assert!(!tracker.is_approaching_timeout());
    }

    #[test]
    fn test_progress_reporting() {
        let tracker = StreamingProgressTracker::new(Duration::from_secs(100));

        tracker.report_progress_with_elapsed(Duration::from_secs(30));
        assert_eq!(tracker.progress_percent(), 30);

        tracker.report_progress_with_elapsed(Duration::from_secs(80));
        assert_eq!(tracker.progress_percent(), 80);
    }

    #[test]
    fn test_warning_threshold() {
        let tracker =
            StreamingProgressTracker::new(Duration::from_secs(100)).with_warning_threshold(0.8);

        tracker.report_progress_with_elapsed(Duration::from_secs(50));
        assert!(!tracker.is_approaching_timeout());

        tracker.report_progress_with_elapsed(Duration::from_secs(85));
        assert!(tracker.is_approaching_timeout());
    }

    #[test]
    fn test_callback_execution() {
        let progress_log = Arc::new(Mutex::new(Vec::new()));
        let progress_clone = progress_log.clone();

        let tracker = StreamingProgressTracker::new(Duration::from_secs(100)).with_callback(
            Box::new(move |progress: f32| {
                progress_clone.lock().unwrap().push(progress);
            }),
        );

        tracker.report_progress_with_elapsed(Duration::from_secs(30));
        tracker.report_progress_with_elapsed(Duration::from_secs(60));
        tracker.report_progress_with_elapsed(Duration::from_secs(90));

        let log = progress_log.lock().unwrap();
        assert!(!log.is_empty());
        assert!(log.iter().all(|&p| p >= 0.0 && p <= 1.0));
    }

    #[test]
    fn test_builder_pattern() {
        let tracker = StreamingProgressBuilder::new(300)
            .warning_threshold(0.75)
            .build();

        assert_eq!(tracker.total_timeout.as_secs(), 300);
        assert_eq!(tracker.warning_threshold, 0.75);
    }

    #[test]
    fn test_zero_timeout_safety() {
        let tracker = StreamingProgressTracker::new(Duration::from_secs(0));
        tracker.report_chunk_received(); // Should not panic
        assert!(!tracker.is_approaching_timeout());
    }

    #[test]
    fn test_progress_clamping() {
        let tracker = StreamingProgressTracker::new(Duration::from_secs(100));

        tracker.report_progress_with_elapsed(Duration::from_secs(150)); // Beyond timeout
        assert_eq!(tracker.progress_percent(), 99); // Clamped at 99%

        tracker.report_error();
        assert_eq!(tracker.progress_percent(), 100);
    }
}
