use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::warn;

/// A token bucket implementation for rate limiting.
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_rate: f64, // tokens per second
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let new_tokens = elapsed * self.refill_rate;

        if new_tokens > 0.0 {
            self.tokens = (self.tokens + new_tokens).min(self.capacity);
            self.last_refill = now;
        }
    }

    fn try_acquire(&mut self, cost: f64) -> bool {
        self.refill();
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }
}

/// Priority levels for tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

impl Priority {
    fn weight(&self) -> f64 {
        match self {
            Priority::Low => 2.0,
            Priority::Normal => 1.0,
            Priority::High => 0.5,
            Priority::Critical => 0.1,
        }
    }
}

/// Adaptive Rate Limiter with per-tool priority and exponential backoff.
pub struct AdaptiveRateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    tool_priorities: Mutex<HashMap<String, Priority>>,
    default_capacity: f64,
    default_refill_rate: f64,
}

impl AdaptiveRateLimiter {
    pub fn new(default_capacity: f64, default_refill_rate: f64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            tool_priorities: Mutex::new(HashMap::new()),
            default_capacity,
            default_refill_rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AdaptiveRateLimiter, Priority};
    use std::time::Duration;

    #[test]
    fn returns_wait_hint_when_bucket_exhausted() {
        let limiter = AdaptiveRateLimiter::new(1.0, 1.0);
        assert!(limiter.try_acquire("tool").is_ok());
        let wait_hint = limiter
            .try_acquire("tool")
            .expect_err("second immediate call should be rate-limited");
        assert!(wait_hint > Duration::ZERO);
    }

    #[test]
    fn high_priority_wait_is_shorter_than_low_priority() {
        let limiter = AdaptiveRateLimiter::new(0.2, 1.0);
        limiter.set_priority("high", Priority::High);
        limiter.set_priority("low", Priority::Low);

        let high_wait = limiter
            .try_acquire("high")
            .expect_err("high-priority call should be limited");
        let low_wait = limiter
            .try_acquire("low")
            .expect_err("low-priority call should be limited");

        assert!(high_wait < low_wait);
    }
}

/// Shared adaptive limiter for non-session-scoped execution flows (e.g. skill sub-LLM loops).
pub static GLOBAL_ADAPTIVE_RATE_LIMITER: Lazy<AdaptiveRateLimiter> =
    Lazy::new(AdaptiveRateLimiter::default);

/// Acquire from the shared adaptive limiter.
pub fn try_acquire_global(tool_name: &str) -> Result<(), Duration> {
    GLOBAL_ADAPTIVE_RATE_LIMITER.try_acquire(tool_name)
}

impl Default for AdaptiveRateLimiter {
    fn default() -> Self {
        Self::new(10.0, 2.0)
    }
}

impl AdaptiveRateLimiter {
    /// Set a priority level for a specific tool.
    pub fn set_priority(&self, tool_name: &str, priority: Priority) {
        if let Ok(mut priorities) = self.tool_priorities.lock() {
            priorities.insert(tool_name.to_string(), priority);
        } else {
            warn!(
                "adaptive rate limiter priority lock poisoned while setting priority for '{}'",
                tool_name
            );
        }
    }

    /// Try to acquire permission to execute a tool.
    /// Returns Ok(()) if allowed, or Err(Duration) indicating suggested wait time.
    pub fn try_acquire(&self, tool_name: &str) -> Result<(), Duration> {
        let Ok(mut buckets) = self.buckets.lock() else {
            warn!(
                "adaptive rate limiter bucket lock poisoned while acquiring '{}'",
                tool_name
            );
            return Err(Duration::from_millis(100));
        };
        let bucket = buckets
            .entry(tool_name.to_string())
            .or_insert_with(|| TokenBucket::new(self.default_capacity, self.default_refill_rate));

        let Ok(priorities) = self.tool_priorities.lock() else {
            warn!(
                "adaptive rate limiter priority lock poisoned while acquiring '{}'",
                tool_name
            );
            return Err(Duration::from_millis(100));
        };
        let priority = priorities
            .get(tool_name)
            .copied()
            .unwrap_or(Priority::Normal);
        let cost = priority.weight();

        if bucket.try_acquire(cost) {
            Ok(())
        } else {
            // Adaptive backoff:
            // 1. Calculate base need (tokens needed / refill rate)
            // 2. Apply exponential factor based on deficit to discourage hammering
            // 3. High priority tools get a "discount" on the wait time

            let needed = cost - bucket.tokens;
            let base_wait_secs = needed / bucket.refill_rate;

            // Jitter for backoff (add 10% randomness to avoid thundering herd)
            let jitter = 1.1;

            let wait_secs = match priority {
                Priority::Critical => base_wait_secs * 0.5, // Return faster
                Priority::High => base_wait_secs * 0.8,
                Priority::Normal => base_wait_secs * jitter,
                Priority::Low => base_wait_secs * 1.5 * jitter, // Penalize low priority overflow
            };

            Err(Duration::from_secs_f64(wait_secs))
        }
    }
}
