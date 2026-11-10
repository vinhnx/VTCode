//! Memory monitoring and pressure detection system.
//!
//! This module provides real-time memory usage tracking for VT Code, including:
//! - RSS-based memory monitoring
//! - Memory pressure classification (Normal, Warning, Critical)
//! - Memory checkpoints for debugging
//! - Adaptive TTL based on memory pressure

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::warn;
use vtcode_commons::utils::current_timestamp;

pub use self::pressure::MemoryPressure;

mod pressure;

/// Errors that can occur during memory monitoring operations.
#[derive(Debug, Error)]
pub enum MemoryError {
    /// Failed to read system memory information.
    #[error("failed to read memory info: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse memory value from system output.
    #[error("failed to parse memory value: {0}")]
    Parse(String),

    /// Memory monitoring is not supported on this platform.
    #[error("memory monitoring not supported on this platform")]
    Unsupported,
}

/// Memory monitor for tracking system memory usage
#[derive(Clone)]
pub struct MemoryMonitor {
    state: Arc<Mutex<MemoryMonitorState>>,
}

struct MemoryMonitorState {
    /// Memory checkpoint history for debugging
    checkpoints: VecDeque<RssCheckpoint>,
    /// Last recorded RSS in bytes
    last_rss_bytes: usize,
    /// Timestamp of last check
    last_check_timestamp: u64,
}

/// Memory checkpoint for debugging memory spikes
#[derive(Debug, Clone)]
pub struct RssCheckpoint {
    /// Timestamp when checkpoint was recorded
    pub timestamp: u64,
    /// RSS memory at checkpoint (bytes)
    pub rss_bytes: usize,
    /// Label/context for this checkpoint
    pub label: String,
}

/// Memory report for user visibility
#[derive(Debug, Clone)]
pub struct MemoryReport {
    /// Current RSS in MB
    pub current_rss_mb: f64,
    /// Soft limit in MB
    pub soft_limit_mb: f64,
    /// Hard limit in MB
    pub hard_limit_mb: f64,
    /// Current memory pressure
    pub pressure: MemoryPressure,
    /// Usage percentage (0-100)
    pub usage_percent: f64,
    /// Recent memory checkpoints
    pub recent_checkpoints: Vec<RssCheckpoint>,
}

impl MemoryMonitor {
    /// Create a new memory monitor
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryMonitorState {
                checkpoints: VecDeque::with_capacity(vtcode_config::constants::memory::MAX_CHECKPOINT_HISTORY),
                last_rss_bytes: 0,
                last_check_timestamp: 0,
            })),
        }
    }

    /// Get current RSS in bytes using the shared platform sampler.
    ///
    /// Delegates to [`vtcode_commons::memory::resident_set_size_mb`] (Mach task
    /// info on macOS, `/proc/self/statm` on Linux) instead of shelling out or
    /// re-parsing procfs here, so every sample is one cheap syscall.
    pub fn get_rss_bytes() -> Result<usize, MemoryError> {
        let mb = vtcode_commons::memory::resident_set_size_mb().ok_or(MemoryError::Unsupported)?;
        let bytes = (mb * 1024.0 * 1024.0).round();
        // The sampler only returns finite, non-negative values; reject anything
        // else rather than casting blindly.
        if !bytes.is_finite() || bytes < 0.0 || bytes > usize::MAX as f64 {
            return Err(MemoryError::Unsupported);
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "Range-checked above: finite, non-negative, and within usize::MAX."
        )]
        Ok(bytes as usize)
    }

    /// Check current memory pressure
    pub fn check_pressure(&self) -> Result<MemoryPressure, MemoryError> {
        let rss = Self::get_rss_bytes()?;
        let pressure = MemoryPressure::from_rss(rss);

        // Update last known RSS
        if let Ok(mut state) = self.state.lock() {
            state.last_rss_bytes = rss;
            state.last_check_timestamp = current_timestamp();
        }

        Ok(pressure)
    }

    /// Record a memory checkpoint for debugging
    pub fn record_checkpoint(&self, label: String) -> Result<(), MemoryError> {
        let rss = Self::get_rss_bytes()?;

        // Only record if change is significant (> 1 MB).
        // Hold a single lock for both the threshold check and the push to
        // avoid acquiring the mutex twice.
        let min_threshold = vtcode_config::constants::memory::MIN_RSS_CHECKPOINT_BYTES;
        if let Ok(mut state) = self.state.lock() {
            let diff = (rss as i64 - state.last_rss_bytes as i64).unsigned_abs() as usize;
            if diff < min_threshold {
                return Ok(());
            }

            state.checkpoints.push_back(RssCheckpoint {
                timestamp: current_timestamp(),
                rss_bytes: rss,
                label,
            });

            // Enforce max checkpoint history
            let max_history = vtcode_config::constants::memory::MAX_CHECKPOINT_HISTORY;
            while state.checkpoints.len() > max_history {
                state.checkpoints.pop_front();
            }
        }

        Ok(())
    }

    /// Get memory report for user visibility
    pub fn get_report(&self) -> Result<MemoryReport, MemoryError> {
        let rss_bytes = Self::get_rss_bytes()?;
        let pressure = MemoryPressure::from_rss(rss_bytes);

        let soft_limit = vtcode_config::constants::memory::SOFT_LIMIT_BYTES as f64 / (1024.0 * 1024.0);
        let hard_limit = vtcode_config::constants::memory::HARD_LIMIT_BYTES as f64 / (1024.0 * 1024.0);
        let current_rss_mb = rss_bytes as f64 / (1024.0 * 1024.0);

        // Use hard limit for percentage calculation (worst case)
        let usage_percent = (rss_bytes as f64 / vtcode_config::constants::memory::HARD_LIMIT_BYTES as f64) * 100.0;

        let recent_checkpoints = if let Ok(state) = self.state.lock() {
            state.checkpoints.iter().cloned().collect()
        } else {
            Vec::new()
        };

        Ok(MemoryReport {
            current_rss_mb,
            soft_limit_mb: soft_limit,
            hard_limit_mb: hard_limit,
            pressure,
            usage_percent,
            recent_checkpoints,
        })
    }

    /// Get adaptive TTL factor based on current memory pressure
    pub fn adaptive_ttl_factor(&self) -> f64 {
        match self.check_pressure() {
            Ok(MemoryPressure::Normal) => 1.0,
            Ok(MemoryPressure::Warning) => vtcode_config::constants::memory::WARNING_TTL_REDUCTION_FACTOR,
            Ok(MemoryPressure::Critical) => vtcode_config::constants::memory::CRITICAL_TTL_REDUCTION_FACTOR,
            Err(error) => {
                warn!(error = %error, "memory pressure check failed; assuming normal TTL factor");
                1.0
            }
        }
    }

    /// Clear checkpoint history (for testing)
    pub fn clear_checkpoints(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.checkpoints.clear();
        }
    }

    /// Get number of recorded checkpoints
    pub fn checkpoint_count(&self) -> usize {
        self.state.lock().map(|state| state.checkpoints.len()).unwrap_or(0)
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_monitor_creation() {
        let monitor = MemoryMonitor::new();
        assert_eq!(monitor.checkpoint_count(), 0);
    }

    #[test]
    fn test_get_rss_bytes() {
        match MemoryMonitor::get_rss_bytes() {
            Ok(rss) => {
                // RSS should be reasonable (> 1MB and < 10GB)
                assert!(rss > 1024 * 1024, "RSS should be > 1MB");
                assert!(rss < 10 * 1024 * 1024 * 1024, "RSS should be < 10GB");
            }
            Err(e) => {
                println!("Warning: Could not get RSS: {e}");
                // This is acceptable on unsupported platforms
            }
        }
    }

    #[test]
    fn test_get_rss_bytes_matches_shared_sampler() {
        // Independent oracle: the shared sampler this delegates to, with the
        // MiB-to-bytes scale stated here rather than reused from production.
        // RSS can move between the two samples, so assert closeness in both
        // directions (plus an absolute floor) instead of exact equality.
        let sampler_mb = vtcode_commons::memory::resident_set_size_mb();
        match (MemoryMonitor::get_rss_bytes(), sampler_mb) {
            (Ok(bytes), Some(mb)) => {
                let expected = mb * 1024.0 * 1024.0;
                let actual = bytes as f64;
                assert!(actual >= expected * 0.95, "delegated RSS {bytes} bytes fell below sampler {mb} MiB");
                assert!(
                    actual <= expected * 1.05 + 1024.0 * 1024.0,
                    "delegated RSS {bytes} bytes rose above sampler {mb} MiB"
                );
                assert!(bytes > 1024 * 1024, "RSS should exceed 1 MiB, got {bytes}");
            }
            (Err(_), None) => {} // Unsupported platform: both sides agree.
            (Ok(bytes), None) => {
                panic!("get_rss_bytes returned {bytes} but the sampler is unavailable")
            }
            (Err(e), Some(mb)) => {
                panic!("get_rss_bytes failed ({e}) but the sampler returned {mb} MiB")
            }
        }
    }

    #[test]
    fn test_check_pressure() {
        let monitor = MemoryMonitor::new();
        match monitor.check_pressure() {
            Ok(pressure) => {
                // Should always return a valid pressure level
                let _ = format!("{pressure:?}");
            }
            Err(e) => {
                println!("Warning: Could not check pressure: {e}");
                // Acceptable on unsupported platforms
            }
        }
    }

    #[test]
    fn test_record_checkpoint() {
        let monitor = MemoryMonitor::new();
        // Try to record checkpoints (may fail on unsupported platforms)
        let _result = monitor.record_checkpoint("test_checkpoint".to_string());
    }

    #[test]
    fn test_clear_checkpoints() {
        let monitor = MemoryMonitor::new();
        monitor.clear_checkpoints();
        assert_eq!(monitor.checkpoint_count(), 0);
    }

    #[test]
    fn test_adaptive_ttl_factor() {
        let monitor = MemoryMonitor::new();
        let factor = monitor.adaptive_ttl_factor();

        // TTL factor should be between 0.1 and 1.0
        assert!(factor > 0.0);
        assert!(factor <= 1.0);
    }

    #[test]
    fn test_memory_report() {
        let monitor = MemoryMonitor::new();
        match monitor.get_report() {
            Ok(report) => {
                // Report should have reasonable values
                assert!(report.usage_percent >= 0.0);
                assert!(report.soft_limit_mb > 0.0);
                assert!(report.hard_limit_mb > report.soft_limit_mb);
            }
            Err(e) => {
                println!("Warning: Could not generate report: {e}");
                // Acceptable on unsupported platforms
            }
        }
    }
}
