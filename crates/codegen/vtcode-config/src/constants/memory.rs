/// Soft memory limit in bytes (400 MB) - triggers warning and TTL reduction
pub const SOFT_LIMIT_BYTES: usize = 400 * 1024 * 1024;

/// Hard memory limit in bytes (600 MB) - triggers aggressive eviction
pub const HARD_LIMIT_BYTES: usize = 600 * 1024 * 1024;

/// Memory check interval in milliseconds (100 ms)
pub const CHECK_INTERVAL_MS: u64 = 100;

/// TTL reduction factor under warning pressure (reduce from 5min to 2min)
pub const WARNING_TTL_REDUCTION_FACTOR: f64 = 0.4;

/// TTL reduction factor under critical pressure (reduce from 5min to 30s)
pub const CRITICAL_TTL_REDUCTION_FACTOR: f64 = 0.1;

/// Minimum RSA size to track in checkpoints (1 MB)
/// Minimum RSS size to track in checkpoints (1 MiB)
pub const MIN_RSS_CHECKPOINT_BYTES: usize = 1024 * 1024;

/// Maximum memory checkpoint history to keep
pub const MAX_CHECKPOINT_HISTORY: usize = 100;

/// Default threshold for memory pressure report (MB)
pub const DEFAULT_REPORT_THRESHOLD_MB: usize = 50;
