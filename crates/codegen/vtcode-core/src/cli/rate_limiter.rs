//! Rate limiter for API requests and tool calls
//!
//! Re-exports the canonical rate limiter from the tools module.

pub use crate::tools::rate_limiter::{
    PerToolRateLimiter as RateLimiter, RateLimiterConfig, try_acquire, try_acquire_for,
};
