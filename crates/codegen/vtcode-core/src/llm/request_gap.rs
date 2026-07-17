//! Shared tracker for detecting idle gaps between dispatched LLM requests.
//!
//! Both the interactive session state (`SessionStats` in
//! `src/agent/runloop/unified/state.rs`, in the `vtcode` binary crate) and
//! the headless session state (`AgentSessionState` in
//! [`crate::core::agent::session`]) need to answer the same two questions:
//! "has a request been dispatched yet this session" and "how long has it
//! been since the last one". Both use that signal to warn when the
//! provider's server-side prompt cache has likely expired, so the next
//! request may unexpectedly re-pay full input cost. This tracker holds the
//! single piece of state (`last_request_at`) and the logic derived from it,
//! so each embedding site only has to delegate its existing public methods
//! rather than reimplement the same `Option<Instant>` bookkeeping.

use std::time::{Duration, Instant};

/// Tracks the wall-clock time of the most recently dispatched LLM request.
#[derive(Debug, Default, Clone, Copy)]
pub struct RequestGapTracker {
    last_request_at: Option<Instant>,
}

impl RequestGapTracker {
    /// Records that an LLM request was just dispatched, so the next call to
    /// [`Self::cache_gap_exceeds`] can measure the idle gap since this request.
    pub fn note_request_sent(&mut self) {
        self.last_request_at = Some(Instant::now());
    }

    /// Returns the elapsed time since the last dispatched request when it
    /// exceeds `threshold`, or `None` if there was no prior request or the gap
    /// is still within the threshold. Used to warn that the provider prompt
    /// cache has likely expired before the next request re-pays full input
    /// cost.
    pub fn cache_gap_exceeds(&self, threshold: Duration) -> Option<Duration> {
        let last_request_at = self.last_request_at?;
        let elapsed = last_request_at.elapsed();
        (elapsed > threshold).then_some(elapsed)
    }

    /// Returns whether an LLM request has been dispatched at any point this
    /// session. More precise than inferring from accumulated token usage
    /// (which can be zero even after a request, e.g. an error response).
    pub fn has_sent_request(&self) -> bool {
        self.last_request_at.is_some()
    }
}

/// Formats an elapsed cache gap for the advisory message. Sub-minute gaps
/// are shown in seconds (e.g. "45s") rather than rounding down to "0 min",
/// which would otherwise happen for any threshold under 60 seconds.
pub fn format_gap(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{} min", secs / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_gap_exceeds_is_none_without_a_prior_request() {
        let tracker = RequestGapTracker::default();
        assert_eq!(tracker.cache_gap_exceeds(Duration::from_millis(1)), None);
    }

    #[test]
    fn cache_gap_exceeds_is_none_below_threshold() {
        let mut tracker = RequestGapTracker::default();
        tracker.note_request_sent();
        assert_eq!(tracker.cache_gap_exceeds(Duration::from_secs(60)), None);
    }

    #[test]
    fn cache_gap_exceeds_is_some_above_threshold() {
        let mut tracker = RequestGapTracker::default();
        tracker.note_request_sent();
        std::thread::sleep(Duration::from_millis(10));
        let gap = tracker.cache_gap_exceeds(Duration::from_millis(5));
        assert!(gap.is_some());
    }

    #[test]
    fn has_sent_request_is_false_until_note_request_sent() {
        let mut tracker = RequestGapTracker::default();
        assert!(!tracker.has_sent_request());
        tracker.note_request_sent();
        assert!(tracker.has_sent_request());
    }
}
