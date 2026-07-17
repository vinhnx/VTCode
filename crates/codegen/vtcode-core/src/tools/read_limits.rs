//! Read limit configuration for file operations
//!
//! This module provides the single source of truth for bounding file reads.
//! These functions are used by both the new `ReadFileHandler` and the legacy
//! `read_file_legacy` path to ensure consistent behavior.

use super::cache::file_read_cache_config;

/// Default absolute ceiling on lines returned by a single `read_file` call.
///
/// Re-exported from [`vtcode_config::constants::optimization`] so there is a
/// single source of truth for the default cap value.
pub const DEFAULT_MAX_READ_LINES: usize =
    vtcode_config::constants::optimization::DEFAULT_MAX_READ_LINES;

/// Update the read cache config (called during startup)
pub fn configure_read_limits(_config: &vtcode_config::FileReadCacheConfig) {
    // Configuration is managed by cache::configure_file_cache()
    // This function is kept for backward compatibility but is now a no-op
}

/// Absolute ceiling (in lines) for a single line-based `read_file` call.
///
/// Returns the configured `max_read_lines`, or [`DEFAULT_MAX_READ_LINES`] when
/// the value is zero/unspecified, so a misconfigured `0` never disables the cap.
pub fn read_limit_lines() -> usize {
    let cfg = file_read_cache_config();
    if cfg.max_read_lines == 0 {
        DEFAULT_MAX_READ_LINES
    } else {
        cfg.max_read_lines
    }
}

/// Per-call absolute ceiling (in lines) for any line-based `read_file` path.
///
/// Single source of truth shared by the new [`ReadFileHandler`] and the legacy
/// `read_file_legacy` path, so the two can never drift apart. The hard floor of
/// `1` guarantees a misconfigured `0` can never open an unbounded read.
pub fn absolute_line_cap() -> usize {
    read_limit_lines().max(1)
}
