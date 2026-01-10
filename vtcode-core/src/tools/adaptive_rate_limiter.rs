use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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

/// Adaptive Rate Limiter with per-tool priority and backoff.
pub struct AdaptiveRateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    priority_weights: HashMap<String, f64>,
    default_capacity: f64,
    default_refill_rate: f64,
}

impl AdaptiveRateLimiter {
    pub fn new(default_capacity: f64, default_refill_rate: f64) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            priority_weights: HashMap::new(),
            default_capacity,
            default_refill_rate,
        }
    }
}

impl Default for AdaptiveRateLimiter {
    fn default() -> Self {
        Self::new(10.0, 2.0)
    }
}

impl AdaptiveRateLimiter {
    /// Set a priority weight for a specific tool (lower cost = higher priority).
    /// Weight is a multiplier for the token cost (default 1.0).
    /// e.g. 0.5 means the tool costs half as much tokens (higher throughput).
    pub fn set_priority(&mut self, tool_name: &str, weight: f64) {
        self.priority_weights.insert(tool_name.to_string(), weight);
    }

    /// Try to acquire permission to execute a tool.
    /// Returns Ok(()) if allowed, or Err(Duration) indicating suggested wait time.
    pub fn try_acquire(&self, tool_name: &str) -> Result<(), Duration> {
        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(tool_name.to_string())
            .or_insert_with(|| TokenBucket::new(self.default_capacity, self.default_refill_rate));

        let weight = self.priority_weights.get(tool_name).copied().unwrap_or(1.0);
        let cost = 1.0 * weight;

        if bucket.try_acquire(cost) {
            Ok(())
        } else {
            // Simple backoff estimation: time to refill enough tokens for cost
            let needed = cost - bucket.tokens;
            // needed / rate = seconds
            let wait_secs = needed / bucket.refill_rate;
            Err(Duration::from_secs_f64(wait_secs))
        }
    }
}
