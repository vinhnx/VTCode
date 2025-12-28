//! Memory monitoring and pressure detection system.
//!
//! This module provides real-time memory usage tracking for VT Code, including:
//! - RSS-based memory monitoring
//! - Memory pressure classification (Normal, Warning, Critical)
//! - Memory checkpoints for debugging
//! - Adaptive TTL based on memory pressure

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub use self::pressure::MemoryPressure;

mod pressure;

/// Memory monitor for tracking system memory usage
#[derive(Clone)]
pub struct MemoryMonitor {
    state: Arc<Mutex<MemoryMonitorState>>,
}

struct MemoryMonitorState {
    /// Memory checkpoint history for debugging
    checkpoints: VecDeque<MemoryCheckpoint>,
    /// Last recorded RSS in bytes
    last_rss_bytes: usize,
    /// Timestamp of last check
    last_check_timestamp: u64,
}

/// Memory checkpoint for debugging memory spikes
#[derive(Debug, Clone)]
pub struct MemoryCheckpoint {
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
    pub recent_checkpoints: Vec<MemoryCheckpoint>,
}

impl MemoryMonitor {
    /// Create a new memory monitor
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MemoryMonitorState {
                checkpoints: VecDeque::with_capacity(
                    vtcode_config::constants::memory::MAX_CHECKPOINT_HISTORY,
                ),
                last_rss_bytes: 0,
                last_check_timestamp: 0,
            })),
        }
    }

    /// Get current RSS in bytes using platform-specific methods
    #[cfg(target_os = "linux")]
    pub fn get_rss_bytes() -> Result<usize, String> {
        use std::fs;
        use std::io::Read;

        let mut status = String::new();
        fs::File::open("/proc/self/status")
            .and_then(|mut f| f.read_to_string(&mut status))
            .map_err(|e| format!("Failed to read /proc/self/status: {}", e))?;

        for line in status.lines() {
            if line.starts_with("VmRSS:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let kb: usize = parts[1]
                        .parse()
                        .map_err(|_| "Failed to parse VmRSS value".to_string())?;
                    return Ok(kb * 1024); // Convert KB to bytes
                }
            }
        }

        Err("VmRSS not found in /proc/self/status".to_string())
    }

    /// Get current RSS in bytes on macOS using /proc/self/stat or sysctl
    #[cfg(target_os = "macos")]
    pub fn get_rss_bytes() -> Result<usize, String> {
        use std::process::Command;

        // Use `ps` command to get RSS in kilobytes
        let output = Command::new("ps")
            .args(&["-o", "rss=", "-p"])
            .arg(std::process::id().to_string())
            .output()
            .map_err(|e| format!("Failed to run ps command: {}", e))?;

        let rss_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let kb: usize = rss_str
            .parse()
            .map_err(|_| format!("Failed to parse ps output: {}", rss_str))?;

        Ok(kb * 1024) // Convert KB to bytes
    }

    /// Get current RSS in bytes (fallback for unsupported platforms)
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn get_rss_bytes() -> Result<usize, String> {
        Err("Memory monitoring not supported on this platform".to_string())
    }

    /// Check current memory pressure
    pub fn check_pressure(&self) -> Result<MemoryPressure, String> {
        let rss = Self::get_rss_bytes()?;
        let pressure = MemoryPressure::from_rss(rss);

        // Update last known RSS
        if let Ok(mut state) = self.state.lock() {
            state.last_rss_bytes = rss;
            state.last_check_timestamp = current_timestamp_secs();
        }

        Ok(pressure)
    }

    /// Record a memory checkpoint for debugging
    pub fn record_checkpoint(&self, label: String) -> Result<(), String> {
        let rss = Self::get_rss_bytes()?;

        // Only record if change is significant (> 1 MB)
        let min_threshold = vtcode_config::constants::memory::MIN_RSS_CHECKPOINT_BYTES;
        if let Ok(state) = self.state.lock() {
            let diff = (rss as i64 - state.last_rss_bytes as i64).abs() as usize;
            if diff < min_threshold {
                return Ok(());
            }
        }

        let checkpoint = MemoryCheckpoint {
            timestamp: current_timestamp_secs(),
            rss_bytes: rss,
            label,
        };

        if let Ok(mut state) = self.state.lock() {
            state.checkpoints.push_back(checkpoint);

            // Enforce max checkpoint history
            let max_history = vtcode_config::constants::memory::MAX_CHECKPOINT_HISTORY;
            while state.checkpoints.len() > max_history {
                state.checkpoints.pop_front();
            }
        }

        Ok(())
    }

    /// Get memory report for user visibility
    pub fn get_report(&self) -> Result<MemoryReport, String> {
        let rss_bytes = Self::get_rss_bytes()?;
        let pressure = MemoryPressure::from_rss(rss_bytes);

        let soft_limit =
            vtcode_config::constants::memory::SOFT_LIMIT_BYTES as f64 / (1024.0 * 1024.0);
        let hard_limit =
            vtcode_config::constants::memory::HARD_LIMIT_BYTES as f64 / (1024.0 * 1024.0);
        let current_rss_mb = rss_bytes as f64 / (1024.0 * 1024.0);

        // Use hard limit for percentage calculation (worst case)
        let usage_percent =
            (rss_bytes as f64 / vtcode_config::constants::memory::HARD_LIMIT_BYTES as f64) * 100.0;

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
            Ok(MemoryPressure::Warning) => {
                vtcode_config::constants::memory::WARNING_TTL_REDUCTION_FACTOR
            }
            Ok(MemoryPressure::Critical) => {
                vtcode_config::constants::memory::CRITICAL_TTL_REDUCTION_FACTOR
            }
            Err(_) => 1.0, // Assume normal if we can't check
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
        self.state
            .lock()
            .map(|state| state.checkpoints.len())
            .unwrap_or(0)
    }
}

impl Default for MemoryMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
                println!("Warning: Could not get RSS: {}", e);
                // This is acceptable on unsupported platforms
            }
        }
    }

    #[test]
    fn test_check_pressure() {
        let monitor = MemoryMonitor::new();
        match monitor.check_pressure() {
            Ok(pressure) => {
                // Should always return a valid pressure level
                let _ = format!("{:?}", pressure);
            }
            Err(e) => {
                println!("Warning: Could not check pressure: {}", e);
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
                println!("Warning: Could not generate report: {}", e);
                // Acceptable on unsupported platforms
            }
        }
    }
}
