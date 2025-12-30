//! Simple in‑process rate limiter for tool execution.
//!
//! This is a lightweight fallback used when the full `governor` crate is not
//! available in the build. It limits the number of tool calls per second
//! globally for the process. The limiter is deliberately cheap – a `Mutex`
//! protecting a counter and a timestamp – because the surrounding code already
//! performs async work, so contention is minimal.
//!
//! The implementation is intentionally minimalistic: it provides a `RateLimiter`
//! struct with a `try_acquire()` method that returns `Ok(())` when the call is
//! allowed or an `Err` when the limit would be exceeded. Callers can decide how
//! to handle the error (e.g., retry after a delay or surface a user‑friendly
//! message).
//!
//! Usage example:
//! ```rust
//! use vtcode_core::tools::rate_limiter::GLOBAL_RATE_LIMITER;
//!
//! fn execute_tool() -> anyhow::Result<()> {
//!     GLOBAL_RATE_LIMITER.try_acquire()?;
//!     // … actual tool logic …
//!     Ok(())
//! }
//! ```
//!
//! The limiter is configured via environment variables to keep the core
//! library free of additional runtime configuration files:
//!
//! * `VTTOOL_RATE_LIMIT` – maximum calls per second (default = 20).
//! * `VTTOOL_BURST` – maximum burst size (default = 5).
//!
//! The values are read once at startup.
//!
//! This file is added as part of the optimization plan to provide a central
//! rate‑limiting mechanism for all external tool invocations (PTY, web fetch,
//! filesystem, etc.).

use anyhow::{Result, anyhow};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

/// Configuration for the limiter.
#[derive(Debug, Clone, Copy)]
pub struct RateLimiterConfig {
    /// Allowed calls per second.
    pub per_sec: u32,
    /// Maximum burst capacity.
    pub burst: u32,
}

impl Default for RateLimiterConfig {
    fn default() -> Self {
        // Environment variables are optional; fall back to sensible defaults.
        let per_sec = std::env::var("VTTOOL_RATE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);
        let burst = std::env::var("VTTOOL_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        RateLimiterConfig { per_sec, burst }
    }
}

/// Simple token‑bucket implementation.
pub struct RateLimiterInner {
    config: RateLimiterConfig,
    /// Current number of available tokens.
    tokens: u32,
    /// When the bucket was last refilled.
    last_refill: Instant,
}

impl RateLimiterInner {
    fn new() -> Self {
        Self::new_with_config(RateLimiterConfig::default())
    }

    pub fn new_with_config(config: RateLimiterConfig) -> Self {
        Self {
            config,
            tokens: config.burst,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time.
    /// Uses fractional refill based on milliseconds for smoother rate limiting.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        
        // Calculate refill based on milliseconds for finer granularity
        let millis = elapsed.as_millis() as u64;
        
        // Minimum refill interval of 50ms to avoid excessive overhead
        if millis < 50 {
            return;
        }
        
        // Fractional refill: per_sec tokens per 1000ms
        // Using integer math: tokens = (per_sec * millis) / 1000
        let added = ((self.config.per_sec as u64).saturating_mul(millis) / 1000) as u32;
        
        if added > 0 {
            self.tokens = (self.tokens + added).min(self.config.burst);
            self.last_refill = now;
        }
    }

    /// Attempt to acquire a single token.
    pub fn try_acquire(&mut self) -> Result<()> {
        self.refill();
        if self.tokens == 0 {
            Err(anyhow!("tool rate limit exceeded"))
        } else {
            self.tokens -= 1;
            Ok(())
        }
    }
}

/// Public alias for benchmark compatibility
pub type RateLimiter = PerToolRateLimiter;

/// Global rate limiter instance used by all tools.
///
/// The `lazy_static` pattern is avoided to keep the dependency surface low;
/// instead we rely on `once_cell::sync::Lazy` which is already a transitive
/// dependency of the project.
use once_cell::sync::Lazy;
use std::collections::HashMap;

pub static GLOBAL_RATE_LIMITER: Lazy<Mutex<RateLimiterInner>> =
    Lazy::new(|| Mutex::new(RateLimiterInner::new()));

/// Per-tool rate limiter for finer-grained control.
/// Each tool gets its own token bucket, allowing different rate limits per tool.
pub struct PerToolRateLimiter {
    /// Per-tool token buckets. Key is tool name.
    buckets: HashMap<String, RateLimiterInner>,
    /// Default config for new tool buckets.
    default_config: RateLimiterConfig,
}

impl Default for PerToolRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl PerToolRateLimiter {
    /// Create a new per-tool rate limiter with default configuration.
    pub fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            default_config: RateLimiterConfig::default(),
        }
    }

    pub fn new_with_config(config: RateLimiterConfig) -> Self {
        Self {
            buckets: HashMap::new(),
            default_config: config,
        }
    }

    /// Try to acquire a token for a specific tool.
    /// Returns `Ok(())` if allowed, `Err` if rate limited.
    pub fn try_acquire_for(&mut self, tool_name: &str) -> Result<()> {
        let bucket = self
            .buckets
            .entry(tool_name.to_owned())
            .or_insert_with(|| RateLimiterInner::new_with_config(self.default_config));
        bucket.try_acquire()
    }

    /// Alias for try_acquire_for (used by benchmarks)
    pub fn acquire(&mut self, tool_name: &str) -> Result<()> {
        self.try_acquire_for(tool_name)
    }

    /// Check if a tool is currently rate limited without consuming a token.
    pub fn is_limited(&mut self, tool_name: &str) -> bool {
        if let Some(bucket) = self.buckets.get_mut(tool_name) {
            bucket.refill();
            bucket.tokens == 0
        } else {
            false
        }
    }

    /// Reset the rate limiter for a specific tool.
    pub fn reset_tool(&mut self, tool_name: &str) {
        if let Some(bucket) = self.buckets.get_mut(tool_name) {
            bucket.tokens = bucket.config.burst;
            bucket.last_refill = Instant::now();
        }
    }
}

/// Global per-tool rate limiter instance.
pub static PER_TOOL_RATE_LIMITER: Lazy<Mutex<PerToolRateLimiter>> =
    Lazy::new(|| Mutex::new(PerToolRateLimiter::new()));

/// Public API – try to acquire permission for a tool call.
///
/// Returns `Ok(())` when the call is allowed, otherwise an error.
pub fn try_acquire() -> Result<()> {
    let mut guard: MutexGuard<'_, RateLimiterInner> = GLOBAL_RATE_LIMITER
        .lock()
        .map_err(|e| anyhow!("rate limiter poisoned: {}", e))?;
    guard.try_acquire()
}

/// Try to acquire permission for a specific tool.
/// Uses per-tool rate limiting for finer-grained control.
pub fn try_acquire_for(tool_name: &str) -> Result<()> {
    let mut guard = PER_TOOL_RATE_LIMITER
        .lock()
        .map_err(|e| anyhow!("per-tool rate limiter poisoned: {}", e))?;
    guard.try_acquire_for(tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_limiter_allows_burst() {
        let mut limiter = RateLimiterInner::new();
        // Should allow up to burst capacity
        for _ in 0..limiter.config.burst {
            assert!(limiter.try_acquire().is_ok());
        }
        // Next should fail
        assert!(limiter.try_acquire().is_err());
    }

    #[test]
    fn test_per_tool_limiter_isolates_tools() {
        let mut limiter = PerToolRateLimiter::new();
        // Exhaust tool_a
        for _ in 0..5 {
            let _ = limiter.try_acquire_for("tool_a");
        }
        // tool_b should still have tokens
        assert!(limiter.try_acquire_for("tool_b").is_ok());
    }

    #[test]
    fn test_reset_tool_restores_tokens() {
        let mut limiter = PerToolRateLimiter::new();
        // Exhaust tokens
        for _ in 0..10 {
            let _ = limiter.try_acquire_for("tool_x");
        }
        assert!(limiter.is_limited("tool_x"));
        // Reset
        limiter.reset_tool("tool_x");
        assert!(!limiter.is_limited("tool_x"));
    }
}
