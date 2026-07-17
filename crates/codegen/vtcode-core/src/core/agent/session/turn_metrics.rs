//! Turn-level execution metrics tracking.
//!
//! Extracts the turn timing and tool latency fields from `AgentSessionState`
//! into a focused, independently testable unit.

/// Tracks per-turn execution timing and tool-level latencies.
///
/// This struct is self-contained: it can record turn durations, compute
/// statistics, and track per-tool latencies without access to the rest
/// of the session state.
#[derive(Debug, Clone, Default)]
pub struct TurnMetrics {
    /// Total number of turns executed.
    pub turn_count: usize,
    /// Cumulative turn duration in milliseconds.
    pub turn_total_ms: u128,
    /// Maximum single turn duration in milliseconds.
    pub turn_max_ms: u128,
    /// Individual turn durations in milliseconds.
    pub turn_durations_ms: Vec<u128>,
    /// Per-tool execution latencies recorded during the current turn.
    /// Entries are (tool_name, duration_ms).
    pub turn_tool_latencies: Vec<(String, u64)>,
}

impl TurnMetrics {
    /// Record a completed turn.
    pub fn record_turn(&mut self, duration_ms: u128) {
        self.turn_count += 1;
        self.turn_total_ms += duration_ms;
        if duration_ms > self.turn_max_ms {
            self.turn_max_ms = duration_ms;
        }
        self.turn_durations_ms.push(duration_ms);
    }

    /// Record a tool execution latency within the current turn.
    pub fn record_tool_latency(&mut self, tool_name: impl Into<String>, duration_ms: u64) {
        self.turn_tool_latencies
            .push((tool_name.into(), duration_ms));
    }

    /// Clear per-turn tool latencies (call at the start of each turn).
    pub fn clear_tool_latencies(&mut self) {
        self.turn_tool_latencies.clear();
    }

    /// Average turn duration in milliseconds.
    pub fn avg_turn_ms(&self) -> f64 {
        if self.turn_count == 0 {
            0.0
        } else {
            self.turn_total_ms as f64 / self.turn_count as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metrics_are_empty() {
        let metrics = TurnMetrics::default();
        assert_eq!(metrics.turn_count, 0);
        assert_eq!(metrics.turn_total_ms, 0);
        assert_eq!(metrics.turn_max_ms, 0);
        assert!(metrics.turn_durations_ms.is_empty());
    }

    #[test]
    fn record_turn_updates_stats() {
        let mut metrics = TurnMetrics::default();
        metrics.record_turn(100);
        metrics.record_turn(300);
        metrics.record_turn(200);

        assert_eq!(metrics.turn_count, 3);
        assert_eq!(metrics.turn_total_ms, 600);
        assert_eq!(metrics.turn_max_ms, 300);
        assert_eq!(metrics.turn_durations_ms, vec![100, 300, 200]);
    }

    #[test]
    fn avg_turn_ms_computes_correctly() {
        let mut metrics = TurnMetrics::default();
        metrics.record_turn(100);
        metrics.record_turn(200);
        assert!((metrics.avg_turn_ms() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn avg_turn_ms_zero_when_empty() {
        let metrics = TurnMetrics::default();
        assert!((metrics.avg_turn_ms() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tool_latencies_tracked_and_cleared() {
        let mut metrics = TurnMetrics::default();
        metrics.record_tool_latency("read_file", 50);
        metrics.record_tool_latency("write_file", 100);
        assert_eq!(metrics.turn_tool_latencies.len(), 2);

        metrics.clear_tool_latencies();
        assert!(metrics.turn_tool_latencies.is_empty());
    }
}
