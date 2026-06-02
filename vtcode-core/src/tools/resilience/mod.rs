//! Resilience primitives for tool execution.
//!
//! Collects the three concerns that govern how tools recover from and bound
//! transient failures:
//!
//! - [`circuit_breaker`] - state-machine fault isolation (Closed/Open/HalfOpen)
//!   keyed by tool name. Tracks failure counts, applies exponential backoff,
//!   and short-circuits calls when a downstream is degraded.
//! - [`adaptive_rate_limiter`] - per-key token-bucket rate limiter with adaptive
//!   refill. Used to throttle tools whose cost varies with the call.
//! - [`rate_limiter`] - global process-wide counter rate limiter (env-configured
//!   via `VTTOOL_RATE_LIMIT` / `VTTOOL_BURST`). Lightweight fallback used when
//!   the full `governor` crate is unavailable.
//!
//! The three primitives share callers (autonomous executor, tool pipeline,
//! agent error recovery) but address distinct failure modes; they are grouped
//! here so a maintainer can audit the full resilience toolkit in one place.

pub mod adaptive_rate_limiter;
pub mod circuit_breaker;
pub mod rate_limiter;
