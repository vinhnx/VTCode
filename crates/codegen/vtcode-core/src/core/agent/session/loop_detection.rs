//! Loop detection state for stagnation and infinite-loop prevention.
//!
//! Extracts the loop detection fields from `AgentSessionState` into a
//! focused, independently testable unit. The parent struct delegates to
//! this module's methods for loop-related logic.

use std::collections::{VecDeque, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

/// Tracks tool call loops, progress stagnation, and escalation chains.
///
/// This struct is self-contained: it can detect stagnation, enforce loop
/// limits, and track escalation chains without access to the rest of the
/// session state.
#[derive(Debug, Clone)]
pub struct LoopDetectionState {
    /// Consecutive tool calls without an LLM response.
    pub consecutive_tool_loops: usize,
    /// Whether the tool loop limit has been hit.
    pub tool_loop_limit_hit: bool,
    /// Consecutive escalation events in the current escalation chain.
    /// Reset to 0 when tool calls dispatch without escalation.
    pub consecutive_escalations: u32,
    /// Rolling window of progress hashes for stagnation detection.
    /// Each entry is a hash of the assistant response content + key state.
    pub progress_hashes: VecDeque<u64>,
    /// Consecutive turns with matching progress hashes.
    pub stagnant_turns: usize,
    /// Consecutive idle turns (no tool calls, no meaningful output).
    pub consecutive_idle_turns: usize,
    /// Maximum tool loop streak observed in this session.
    pub max_tool_loop_streak: usize,
}

impl Default for LoopDetectionState {
    fn default() -> Self {
        Self {
            consecutive_tool_loops: 0,
            tool_loop_limit_hit: false,
            consecutive_escalations: 0,
            progress_hashes: VecDeque::with_capacity(16),
            stagnant_turns: 0,
            consecutive_idle_turns: 0,
            max_tool_loop_streak: 0,
        }
    }
}

impl LoopDetectionState {
    /// Record a tool call, incrementing the loop counter.
    pub fn record_tool_call(&mut self) {
        self.consecutive_tool_loops += 1;
        if self.consecutive_tool_loops > self.max_tool_loop_streak {
            self.max_tool_loop_streak = self.consecutive_tool_loops;
        }
    }

    /// Reset the tool loop counter after an LLM response.
    pub fn reset_tool_loops(&mut self) {
        self.consecutive_tool_loops = 0;
        self.tool_loop_limit_hit = false;
    }

    /// Record that the tool loop limit was hit.
    pub fn mark_loop_limit_hit(&mut self) {
        self.tool_loop_limit_hit = true;
    }

    /// Record an escalation event.
    pub fn record_escalation(&mut self) {
        self.consecutive_escalations += 1;
    }

    /// Reset the escalation chain.
    pub fn reset_escalations(&mut self) {
        self.consecutive_escalations = 0;
    }

    /// Record a progress hash for stagnation detection.
    ///
    /// Returns `true` if the session appears stagnant (same hash repeated).
    pub fn record_progress(&mut self, content_hash: u64, window_size: usize) -> bool {
        self.progress_hashes.push_back(content_hash);
        if self.progress_hashes.len() > window_size {
            self.progress_hashes.pop_front();
        }

        // Check if all recent hashes are identical
        if self.progress_hashes.len() >= window_size {
            let first = self.progress_hashes[0];
            if self.progress_hashes.iter().all(|&h| h == first) {
                self.stagnant_turns += 1;
                return true;
            }
        }

        self.stagnant_turns = 0;
        false
    }

    /// Record an idle turn.
    pub fn record_idle_turn(&mut self) {
        self.consecutive_idle_turns += 1;
    }

    /// Reset idle turn counter.
    pub fn reset_idle_turns(&mut self) {
        self.consecutive_idle_turns = 0;
    }

    /// Check if the session should be halted due to loop detection.
    pub fn should_halt(&self, max_loops: usize, max_stagnant_turns: usize) -> bool {
        self.tool_loop_limit_hit
            || self.consecutive_tool_loops >= max_loops
            || self.stagnant_turns >= max_stagnant_turns
    }
}

/// Hash a content string for progress tracking.
pub fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_has_no_loops() {
        let state = LoopDetectionState::default();
        assert_eq!(state.consecutive_tool_loops, 0);
        assert!(!state.tool_loop_limit_hit);
        assert_eq!(state.stagnant_turns, 0);
    }

    #[test]
    fn record_tool_call_increments_counter() {
        let mut state = LoopDetectionState::default();
        state.record_tool_call();
        state.record_tool_call();
        assert_eq!(state.consecutive_tool_loops, 2);
        assert_eq!(state.max_tool_loop_streak, 2);
    }

    #[test]
    fn reset_tool_loops_clears_counter() {
        let mut state = LoopDetectionState::default();
        state.record_tool_call();
        state.record_tool_call();
        state.reset_tool_loops();
        assert_eq!(state.consecutive_tool_loops, 0);
        assert!(!state.tool_loop_limit_hit);
    }

    #[test]
    fn stagnation_detected_when_hashes_repeat() {
        let mut state = LoopDetectionState::default();
        let hash = hash_content("same content");

        // Fill the window with identical hashes; the first (window_size - 1)
        // calls should not detect stagnation because the window isn't full yet.
        for _ in 0..4 {
            assert!(!state.record_progress(hash, 5));
        }
        // When the window fills with identical hashes, stagnation is detected.
        assert!(state.record_progress(hash, 5));
        assert_eq!(state.stagnant_turns, 1);
    }

    #[test]
    fn stagnation_resets_on_new_content() {
        let mut state = LoopDetectionState::default();

        for i in 0..5 {
            state.record_progress(hash_content(&format!("content {i}")), 5);
        }
        assert_eq!(state.stagnant_turns, 0);
    }

    #[test]
    fn should_halt_on_loop_limit() {
        let state = LoopDetectionState { tool_loop_limit_hit: true, ..Default::default() };
        assert!(state.should_halt(10, 5));
    }

    #[test]
    fn should_halt_on_excessive_loops() {
        let state = LoopDetectionState { consecutive_tool_loops: 10, ..Default::default() };
        assert!(state.should_halt(10, 5));
    }

    #[test]
    fn should_halt_on_stagnation() {
        let state = LoopDetectionState { stagnant_turns: 5, ..Default::default() };
        assert!(state.should_halt(10, 5));
    }

    #[test]
    fn escalation_chain_tracks_and_resets() {
        let mut state = LoopDetectionState::default();
        state.record_escalation();
        state.record_escalation();
        assert_eq!(state.consecutive_escalations, 2);
        state.reset_escalations();
        assert_eq!(state.consecutive_escalations, 0);
    }
}
