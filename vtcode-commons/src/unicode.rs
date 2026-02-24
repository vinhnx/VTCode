//! Unicode monitoring and validation utilities

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

/// Global unicode monitoring statistics
#[derive(Debug, Default)]
pub struct UnicodeMonitor {
    // Processing statistics
    pub total_bytes_processed: AtomicU64,
    pub total_unicode_bytes: AtomicU64,
    pub total_sequences: AtomicU64,
    pub total_errors: AtomicU64,

    // Error tracking
    pub error_types: Mutex<HashMap<String, usize>>,
    pub last_error: Mutex<Option<String>>,

    // Performance metrics
    pub processing_time_ns: AtomicU64,
    pub max_buffer_size: AtomicUsize,

    // Session tracking
    pub active_sessions: AtomicUsize,
    pub total_sessions: AtomicUsize,
    pub unicode_sessions: AtomicUsize,
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
        self.total_bytes_processed
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_unicode_bytes
            .fetch_add(unicode_bytes as u64, Ordering::Relaxed);
        self.total_sequences.fetch_add(1, Ordering::Relaxed);

        if contains_unicode {
            self.unicode_sessions.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a unicode error
    pub fn record_error(&self, error_type: &str, details: &str) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut errors) = self.error_types.lock() {
            *errors.entry(error_type.to_string()).or_insert(0) += 1;
        }

        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(format!("{}: {}", error_type, details));
        }
    }

    /// Record processing time in nanoseconds
    pub fn record_processing_time(&self, nanoseconds: u64) {
        self.processing_time_ns
            .fetch_add(nanoseconds, Ordering::Relaxed);
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
    pub start_time: Instant,
    pub unicode_detected: bool,
    pub errors: Vec<String>,
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
        // Implementation can be expanded to update global statistics
    }
}

/// Global unicode monitor instance
pub static UNICODE_MONITOR: Lazy<UnicodeMonitor> = Lazy::new(UnicodeMonitor::new);
