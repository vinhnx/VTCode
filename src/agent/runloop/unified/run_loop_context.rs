use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::ui::tui::InlineSession;
use vtcode_core::utils::ansi::AnsiRenderer;
// use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager; // unused
use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::tools::ApprovalRecorder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRunId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPhase {
    Preparing,
    Requesting,
    ExecutingTools,
    Finalizing,
}

pub struct HarnessTurnState {
    #[allow(dead_code)]
    pub run_id: TurnRunId,
    #[allow(dead_code)]
    pub turn_id: TurnId,
    pub phase: TurnPhase,
    pub turn_started_at: Instant,
    pub tool_calls: usize,
    pub blocked_tool_calls: usize,
    pub consecutive_blocked_tool_calls: usize,
    pub consecutive_spool_chunk_reads: usize,
    pub tool_budget_warning_emitted: bool,
    pub tool_budget_exhausted_emitted: bool,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock: Duration,
    pub max_tool_retries: u32,
}

impl HarnessTurnState {
    pub fn new(
        run_id: TurnRunId,
        turn_id: TurnId,
        max_tool_calls: usize,
        max_tool_wall_clock_secs: u64,
        max_tool_retries: u32,
    ) -> Self {
        Self {
            run_id,
            turn_id,
            phase: TurnPhase::Preparing,
            turn_started_at: Instant::now(),
            tool_calls: 0,
            blocked_tool_calls: 0,
            consecutive_blocked_tool_calls: 0,
            consecutive_spool_chunk_reads: 0,
            tool_budget_warning_emitted: false,
            tool_budget_exhausted_emitted: false,
            max_tool_calls,
            max_tool_wall_clock: Duration::from_secs(max_tool_wall_clock_secs),
            max_tool_retries,
        }
    }

    pub fn tool_budget_exhausted(&self) -> bool {
        self.tool_calls >= self.max_tool_calls
    }

    pub fn wall_clock_exhausted(&self) -> bool {
        self.turn_started_at.elapsed() >= self.max_tool_wall_clock
    }

    pub fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    pub fn record_blocked_tool_call(&mut self) -> usize {
        self.blocked_tool_calls = self.blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls = self.consecutive_blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls
    }

    pub fn reset_blocked_tool_call_streak(&mut self) {
        self.consecutive_blocked_tool_calls = 0;
    }

    pub fn tool_budget_usage_ratio(&self) -> f64 {
        if self.max_tool_calls == 0 {
            1.0
        } else {
            self.tool_calls as f64 / self.max_tool_calls as f64
        }
    }

    pub fn remaining_tool_calls(&self) -> usize {
        self.max_tool_calls.saturating_sub(self.tool_calls)
    }

    pub fn should_emit_tool_budget_warning(&self, threshold: f64) -> bool {
        !self.tool_budget_warning_emitted && self.tool_budget_usage_ratio() >= threshold
    }

    pub fn mark_tool_budget_warning_emitted(&mut self) {
        self.tool_budget_warning_emitted = true;
    }

    pub fn mark_tool_budget_exhausted_emitted(&mut self) {
        self.tool_budget_exhausted_emitted = true;
    }

    pub fn record_spool_chunk_read(&mut self) -> usize {
        self.consecutive_spool_chunk_reads = self.consecutive_spool_chunk_reads.saturating_add(1);
        self.consecutive_spool_chunk_reads
    }

    pub fn reset_spool_chunk_read_streak(&mut self) {
        self.consecutive_spool_chunk_reads = 0;
    }

    pub fn set_phase(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }
}

#[allow(dead_code)]
pub(crate) struct RunLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
    pub session: &'a mut InlineSession,
    pub safety_validator: Option<&'a Arc<RwLock<ToolCallSafetyValidator>>>,
    pub traj: &'a TrajectoryLogger,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
}

// Lightweight adapter that provides a smaller context for per-turn operations.
#[allow(dead_code)]
pub(crate) struct TurnExecutionContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
}

#[cfg(test)]
mod tests {
    use super::{HarnessTurnState, TurnId, TurnPhase, TurnRunId};

    #[test]
    fn harness_state_tracks_phase_transitions() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            2,
            10,
            1,
        );

        // Verify that run_id and turn_id are accessible
        assert_eq!(state.run_id.0, "run-1");
        assert_eq!(state.turn_id.0, "turn-1");

        assert_eq!(state.phase, TurnPhase::Preparing);
        state.set_phase(TurnPhase::Requesting);
        assert_eq!(state.phase, TurnPhase::Requesting);
        state.set_phase(TurnPhase::ExecutingTools);
        assert_eq!(state.phase, TurnPhase::ExecutingTools);
        state.set_phase(TurnPhase::Finalizing);
        assert_eq!(state.phase, TurnPhase::Finalizing);
    }

    #[test]
    fn harness_state_tracks_spool_chunk_read_streak() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            2,
            10,
            1,
        );

        assert_eq!(state.record_spool_chunk_read(), 1);
        assert_eq!(state.record_spool_chunk_read(), 2);
        state.reset_spool_chunk_read_streak();
        assert_eq!(state.record_spool_chunk_read(), 1);
    }

    #[test]
    fn harness_state_tracks_budget_warning_threshold_once() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert!(!state.should_emit_tool_budget_warning(0.75));
        state.record_tool_call(); // 1/4
        assert!(!state.should_emit_tool_budget_warning(0.75));
        state.record_tool_call(); // 2/4
        assert!(!state.should_emit_tool_budget_warning(0.75));
        state.record_tool_call(); // 3/4 => 75%
        assert!(state.should_emit_tool_budget_warning(0.75));
        state.mark_tool_budget_warning_emitted();
        assert!(!state.should_emit_tool_budget_warning(0.75));
        assert_eq!(state.remaining_tool_calls(), 1);
    }

    #[test]
    fn harness_state_tracks_budget_exhaustion_notice_flag() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            1,
            10,
            1,
        );

        assert!(!state.tool_budget_exhausted());
        assert!(!state.tool_budget_exhausted_emitted);
        state.record_tool_call();
        assert!(state.tool_budget_exhausted());
        state.mark_tool_budget_exhausted_emitted();
        assert!(state.tool_budget_exhausted_emitted);
    }

    #[test]
    fn harness_state_tracks_blocked_call_streak() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert_eq!(state.blocked_tool_calls, 0);
        assert_eq!(state.record_blocked_tool_call(), 1);
        assert_eq!(state.record_blocked_tool_call(), 2);
        assert_eq!(state.blocked_tool_calls, 2);
        state.reset_blocked_tool_call_streak();
        assert_eq!(state.consecutive_blocked_tool_calls, 0);
    }
}
