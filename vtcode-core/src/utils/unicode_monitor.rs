//! Unicode monitoring and validation utilities
//!
//! Provides comprehensive monitoring for unicode processing in VT Code,
//! including validation, statistics, and performance metrics.

use std::sync::atomic::{AtomicUsize, AtomicU64, Ordering};
use std::sync::Mutex;
use std::collections::HashMap;
use std::time::Instant;

/// Global unicode monitoring statistics
#[derive(Debug, Default)]
pub struct UnicodeMonitor {
    // Processing statistics
    total_bytes_processed: AtomicU64,
    total_unicode_bytes: AtomicU64,
    total_sequences: AtomicU64,
    total_errors: AtomicU64,
    
    // Error tracking
    error_types: Mutex<HashMap<String, usize>>,
    last_error: Mutex<Option<String>>,
    
    // Performance metrics
    processing_time_ns: AtomicU64,
    max_buffer_size: AtomicUsize,
    
    // Session tracking
    active_sessions: AtomicUsize,
    total_sessions: AtomicUsize,
    unicode_sessions: AtomicUsize,
}

impl UnicodeMonitor {
    /// Create a new unicode monitor
    pub fn new() -> Self {
        Self {
            total_bytes_processed: AtomicU64::new(0),
            total_unicode_bytes: AtomicU64::new(0),
            total_sequences: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            error_types: Mutex::new(HashMap::new()),
            last_error: Mutex::new(None),
            processing_time_ns: AtomicU64::new(0),
            max_buffer_size: AtomicUsize::new(0),
            active_sessions: AtomicUsize::new(0),
            total_sessions: AtomicUsize::new(0),
            unicode_sessions: AtomicUsize::new(0),
        }
    }
    
    /// Record unicode processing statistics
    pub fn record_processing(&self, bytes: usize, unicode_bytes: usize, contains_unicode: bool) {
        self.total_bytes_processed.fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_unicode_bytes.fetch_add(unicode_bytes as u64, Ordering::Relaxed);
        self.total_sequences.fetch_add(1, Ordering::Relaxed);
        
        if contains_unicode {
            self.unicode_sessions.fetch_add(1, Ordering::Relaxed);
        }
    }
    
    /// Record a unicode error
    pub fn record_error(&self, error_type: &str, details: &str) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
        
        let mut errors = self.error_types.lock().unwrap();
        *errors.entry(error_type.to_string()).or_insert(0) += 1;
        
        let mut last = self.last_error.lock().unwrap();
        *last = Some(format!("{}: {}", error_type, details));
    }
    
    /// Record processing time in nanoseconds
    pub fn record_processing_time(&self, nanoseconds: u64) {
        self.processing_time_ns.fetch_add(nanoseconds, Ordering::Relaxed);
    }
    
    /// Record maximum buffer size encountered
    pub fn record_max_buffer_size(&self, size: usize) {
        self.max_buffer_size.fetch_max(size, Ordering::Relaxed);
    }
    
    /// Start a new unicode processing session
    pub fn start_session(&self) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
        self.total_sessions.fetch_add(1, Ordering::Relaxed);
    }
    
    /// End a unicode processing session
    pub fn end_session(&self) {
        self.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Unicode validation context for tracking processing of individual buffers
pub struct UnicodeValidationContext {
    start_time: Instant,
    unicode_detected: bool,
    errors: Vec<String>,
}

impl UnicodeValidationContext {
    /// Create a new validation context
    pub fn new(_buffer_size: usize) -> Self {
        Self {
            start_time: Instant::now(),
            unicode_detected: false,
            errors: Vec::new(),
        }
    }
    
    /// Record that unicode was detected in this buffer
    pub fn record_unicode_detected(&mut self) {
        self.unicode_detected = true;
    }
    
    /// Record an error that occurred during processing
    pub fn record_error(&mut self, error: String) {
        self.errors.push(error);
    }
    
    /// Complete the validation and record statistics
    pub fn complete(self, _processed_bytes: usize) {
        let _processing_time = self.start_time.elapsed().as_nanos() as u64;
        
        // For now, just log the completion
        // In a full implementation, this would update global statistics
        if !self.errors.is_empty() {
            eprintln!("Unicode validation completed with {} errors", self.errors.len());
        }
    }
}

/// Global unicode monitor instance
use std::sync::LazyLock;
pub static UNICODE_MONITOR: LazyLock<UnicodeMonitor> = LazyLock::new(|| UnicodeMonitor::new());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unicode_monitor_basic() {
        let monitor = UnicodeMonitor::new();
        
        monitor.record_processing(100, 50, true);
        monitor.record_processing(200, 0, false);
        
        assert_eq!(monitor.total_bytes_processed.load(Ordering::Relaxed), 300);
        assert_eq!(monitor.total_unicode_bytes.load(Ordering::Relaxed), 50);
        assert_eq!(monitor.total_sequences.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_unicode_validation_context() {
        let mut context = UnicodeValidationContext::new(100);
        context.record_unicode_detected();
        context.record_error("test error".to_string());
        context.complete(50);
        
        // Should complete without panicking
    }
}