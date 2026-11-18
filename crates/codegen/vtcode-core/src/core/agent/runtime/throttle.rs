/// Bounded update throttle shared by the streaming lifecycle paths.
///
/// Limits how many intermediate snapshot events are emitted for a single
/// streaming text/reasoning buffer within one turn:
///
/// - emission stops once `max_events` snapshots have been produced;
/// - an update is emitted eagerly when the caller's `eager` signal is true
///   (e.g. the first update, or a reasoning stage change);
/// - otherwise an update requires at least `min_bytes` of new content since
///   the last emitted snapshot.
///
/// The throttle owns only the accounting; the authoritative accumulated text
/// lives on the lifecycle emitter, which always emits a final lossless
/// snapshot on completion. This keeps live and replay streams consistent.
#[derive(Debug, Clone, Default)]
pub(super) struct UpdateThrottle {
    events: usize,
    last_emit_len: usize,
}

impl UpdateThrottle {
    pub(super) const fn new() -> Self {
        Self { events: 0, last_emit_len: 0 }
    }

    /// Whether an intermediate snapshot should be emitted now.
    pub(super) fn should_emit(&self, current_len: usize, eager: bool, min_bytes: usize, max_events: usize) -> bool {
        if self.events >= max_events {
            return false;
        }
        eager || current_len.saturating_sub(self.last_emit_len) >= min_bytes
    }

    /// Record that a snapshot was emitted at `current_len`.
    pub(super) fn record(&mut self, current_len: usize) {
        self.events += 1;
        self.last_emit_len = current_len;
    }

    /// Advance the last-emitted length marker without counting an event.
    /// Used when a snapshot is emitted outside the throttle's accounting
    /// (e.g. the first reasoning snapshot before reasoning has "started").
    pub(super) fn advance_to(&mut self, current_len: usize) {
        self.last_emit_len = current_len;
    }

    /// Whether no snapshot has been emitted yet — the first-update eager signal.
    pub(super) fn is_first(&self) -> bool {
        self.events == 0
    }

    /// Reset the throttle for a new turn.
    pub(super) fn reset(&mut self) {
        self.events = 0;
        self.last_emit_len = 0;
    }
}
