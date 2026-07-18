use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

/// Cached result of an async (background / prefire) pass-1 sample for
/// two-pass compaction. Held between the background pass-1 and the
/// synchronous pass-2 apply at compaction time.
#[derive(Debug, Clone)]
pub struct AsyncCompactionCache {
    /// The successor-usable NOTE₁ text (extracted `<summary>` or full pass-1 output).
    pub note1: String,
    /// Number of leading conversation items pass-1 summarized.
    pub prefix_len: usize,
    /// Fingerprint of `conversation[..prefix_len]` at pass-1 time. Pass-2 only
    /// applies NOTE₁ when the current conversation still has this exact prefix.
    pub fingerprint: u64,
    /// Model slug pass-1 ran under; invalidated on model switch.
    pub model_slug: String,
    /// Wall time pass-1 took (ms).
    pub pass1_latency_ms: u64,
}

/// Prefire two-pass state. Manages the in-flight guard and cached async
/// pass-1 result between turns.
#[derive(Default)]
pub struct PrefireState {
    /// Set while a background pass-1 sample is running.
    in_flight: AtomicBool,
    /// Cached async pass-1 result, ready for pass-2 apply.
    cache: Mutex<Option<AsyncCompactionCache>>,
}

impl PrefireState {
    /// Try to claim the single in-flight slot. Returns `true` iff this caller
    /// won the race and should start pass-1.
    pub fn try_begin(&self) -> bool {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    /// Release the in-flight slot.
    pub fn finish(&self) {
        self.in_flight.store(false, Ordering::Release);
    }

    /// Whether a pass-1 is currently running.
    pub fn is_in_flight(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }

    /// Stash a completed pass-1 cache for later pass-2 use.
    pub fn store(&self, cache: AsyncCompactionCache) {
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(cache);
    }

    /// Take the cache, leaving `None`.
    pub fn take(&self) -> Option<AsyncCompactionCache> {
        self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    /// Drop any cached async pass-1 result.
    pub fn clear(&self) {
        *self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Whether a valid cache is available.
    pub fn has_cache(&self) -> bool {
        self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_begin_blocks_concurrent_wins() {
        let state = PrefireState::default();
        assert!(state.try_begin());
        assert!(!state.try_begin());
        state.finish();
        assert!(state.try_begin());
    }

    #[test]
    fn store_take_roundtrip() {
        let state = PrefireState::default();
        state.store(AsyncCompactionCache {
            note1: "note".to_string(),
            prefix_len: 3,
            fingerprint: 42,
            model_slug: "model".to_string(),
            pass1_latency_ms: 5,
        });
        assert!(state.has_cache());
        let cache = state.take().unwrap();
        assert_eq!(cache.note1, "note");
        assert_eq!(cache.prefix_len, 3);
        assert!(!state.has_cache());
    }

    #[test]
    fn clear_drops_cache() {
        let state = PrefireState::default();
        state.store(AsyncCompactionCache {
            note1: "note".to_string(),
            prefix_len: 3,
            fingerprint: 42,
            model_slug: "model".to_string(),
            pass1_latency_ms: 5,
        });
        assert!(state.has_cache());
        state.clear();
        assert!(!state.has_cache());
    }

    #[test]
    fn is_in_flight_reflects_try_begin() {
        let state = PrefireState::default();
        assert!(!state.is_in_flight());
        state.try_begin();
        assert!(state.is_in_flight());
        state.finish();
        assert!(!state.is_in_flight());
    }
}
