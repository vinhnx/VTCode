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

use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use anyhow::{anyhow, Result};

/// Configuration for the limiter.
#[derive(Debug, Clone, Copy)]
struct Config {
    /// Allowed calls per second.
    per_sec: u32,
    /// Maximum burst capacity.
    burst: u32,
}

impl Default for Config {
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
        Config { per_sec, burst }
    }
}

/// Simple token‑bucket implementation.
pub struct RateLimiterInner {
    config: Config,
    /// Current number of available tokens.
    tokens: u32,
    /// When the bucket was last refilled.
    last_refill: Instant,
}

impl RateLimiterInner {
    fn new() -> Self {
        let config = Config::default();
        Self {
            config,
            tokens: config.burst,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        if elapsed.is_zero() {
            return;
        }
        // Compute how many whole seconds have passed.
        let secs = elapsed.as_secs() as u32;
        if secs == 0 {
            return;
        }
        let added = secs.saturating_mul(self.config.per_sec);
        self.tokens = (self.tokens + added).min(self.config.burst);
        self.last_refill = now;
    }

    /// Attempt to acquire a single token.
    fn try_acquire(&mut self) -> Result<()> {
        self.refill();
        if self.tokens == 0 {
            Err(anyhow!("tool rate limit exceeded"))
        } else {
            self.tokens -= 1;
            Ok(())
        }
    }
}

/// Global rate limiter instance used by all tools.
///
/// The `lazy_static` pattern is avoided to keep the dependency surface low;
/// instead we rely on `once_cell::sync::Lazy` which is already a transitive
/// dependency of the project.
use once_cell::sync::Lazy;

pub static GLOBAL_RATE_LIMITER: Lazy<Mutex<RateLimiterInner>> = Lazy::new(|| Mutex::new(RateLimiterInner::new()));

/// Public API – try to acquire permission for a tool call.
///
/// Returns `Ok(())` when the call is allowed, otherwise an error.
pub fn try_acquire() -> Result<()> {
    let mut guard: MutexGuard<'_, RateLimiterInner> = GLOBAL_RATE_LIMITER.lock().map_err(|e| anyhow!("rate limiter poisoned: {}", e))?;
    guard.try_acquire()
}

