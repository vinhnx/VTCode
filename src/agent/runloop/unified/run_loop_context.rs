use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use hashbrown::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::ToolRegistry;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHandle;
use vtcode_ui::tui::app::InlineSession;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnRunId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnPhase {
    Preparing,
    Requesting,
    ExecutingTools,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnExecutionPhase {
    Preparing,
    Requesting,
    ExecutingTools,
    Finalizing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryPhase {
    Inactive,
    Pending,
    InPass,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryMode {
    ToolEnabledRetry,
    ToolFreeSynthesis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetWarning {
    pub used: usize,
    pub max: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetExhaustion {
    pub used: usize,
    pub max: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolBudgetExhaustionNotice {
    pub exhaustion: ToolBudgetExhaustion,
    pub first_notice: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolWallClockExhaustion {
    pub max_secs: u64,
}

pub(crate) const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

impl ToolBudgetWarning {
    pub(crate) fn system_message(self) -> String {
        format!(
            "Tool-call budget warning: {}/{} used; {} remaining for this turn. Use targeted extraction/batching before additional tool calls.",
            self.used, self.max, self.remaining
        )
    }

    pub(crate) fn log_threshold_reached(self, path: &'static str) {
        tracing::info!(
            used = self.used,
            max = self.max,
            remaining = self.remaining,
            "{path}"
        );
    }
}

impl ToolBudgetExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!(
            "Policy violation: exceeded max tool calls per turn ({})",
            self.max
        )
    }

    pub(crate) fn blocked_turn_reason(self) -> String {
        debug_assert!(
            self.max > 0,
            "disabled tool-call caps must not emit exhaustion"
        );
        format!(
            "Tool-call budget exhausted for this turn ({}/{}). Start a new turn with \"continue\" or provide a new instruction to proceed.",
            self.used, self.max
        )
    }
}

impl ToolWallClockExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!(
            "Policy violation: exceeded tool wall clock budget ({}s)",
            self.max_secs
        )
    }
}

impl From<TurnPhase> for TurnExecutionPhase {
    fn from(value: TurnPhase) -> Self {
        match value {
            TurnPhase::Preparing => Self::Preparing,
            TurnPhase::Requesting => Self::Requesting,
            TurnPhase::ExecutingTools => Self::ExecutingTools,
            TurnPhase::Finalizing => Self::Finalizing,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnExecutionSnapshot {
    pub run_id: String,
    pub turn_id: String,
    pub phase: TurnExecutionPhase,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock_secs: u64,
    pub max_tool_retries: u32,
}

pub(crate) struct HarnessTurnState {
    pub run_id: TurnRunId,
    pub turn_id: TurnId,
    pub phase: TurnPhase,
    pub turn_started_at: Instant,
    pub turn_timeout: Duration,
    pub tool_calls: usize,
    pub blocked_tool_calls: usize,
    pub consecutive_blocked_tool_calls: usize,
    pub consecutive_spool_chunk_reads: usize,
    pub consecutive_same_shell_command_runs: usize,
    pub last_shell_command_signature: Option<String>,
    pub consecutive_same_file_read_family_calls: usize,
    last_file_read_family_signature: Option<String>,
    seen_successful_readonly_signatures: HashSet<String>,
    streamed_tool_call_item_ids: HashMap<String, String>,
    pub stop_hook_active: bool,
    pub seen_task_tracker_create_signatures: HashSet<String>,
    pub replaceable_task_tracker_block: Option<Vec<String>>,
    pub tool_budget_warning_emitted: bool,
    pub tool_budget_exhausted_emitted: bool,
    pub recovery_reason: Option<String>,
    recovery_phase: RecoveryPhase,
    recovery_mode: Option<RecoveryMode>,
    recovery_retry_count: u8,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock: Duration,
    pub max_tool_retries: u32,
}

impl HarnessTurnState {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
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
            turn_timeout: Duration::from_secs(max_tool_wall_clock_secs.max(1)),
            tool_calls: 0,
            blocked_tool_calls: 0,
            consecutive_blocked_tool_calls: 0,
            consecutive_spool_chunk_reads: 0,
            consecutive_same_shell_command_runs: 0,
            last_shell_command_signature: None,
            consecutive_same_file_read_family_calls: 0,
            last_file_read_family_signature: None,
            seen_successful_readonly_signatures: HashSet::new(),
            streamed_tool_call_item_ids: HashMap::new(),
            stop_hook_active: false,
            seen_task_tracker_create_signatures: HashSet::new(),
            replaceable_task_tracker_block: None,
            tool_budget_warning_emitted: false,
            tool_budget_exhausted_emitted: false,
            recovery_reason: None,
            recovery_phase: RecoveryPhase::Inactive,
            recovery_mode: None,
            recovery_retry_count: 0,
            max_tool_calls,
            max_tool_wall_clock: Duration::from_secs(max_tool_wall_clock_secs),
            max_tool_retries,
        }
    }

    pub(crate) fn has_tool_call_budget(&self) -> bool {
        self.max_tool_calls > 0
    }

    pub(crate) fn tool_budget_exhausted(&self) -> bool {
        self.has_tool_call_budget() && self.tool_calls >= self.max_tool_calls
    }

    pub(crate) fn tool_budget_exhaustion(&self) -> Option<ToolBudgetExhaustion> {
        self.tool_budget_exhausted()
            .then_some(ToolBudgetExhaustion {
                used: self.tool_calls,
                max: self.max_tool_calls,
                remaining: self.remaining_tool_calls(),
            })
    }

    pub(crate) fn wall_clock_exhausted(&self) -> bool {
        self.turn_started_at.elapsed() >= self.max_tool_wall_clock
    }

    pub(crate) fn wall_clock_budget_exhaustion(&self) -> Option<ToolWallClockExhaustion> {
        self.wall_clock_exhausted()
            .then_some(ToolWallClockExhaustion {
                max_secs: self.max_tool_wall_clock.as_secs(),
            })
    }

    pub(crate) fn set_turn_timeout_secs(&mut self, turn_timeout_secs: u64) {
        self.turn_timeout = Duration::from_secs(turn_timeout_secs.max(1));
    }

    pub(crate) fn remaining_turn_timeout(&self) -> Duration {
        self.turn_timeout
            .saturating_sub(self.turn_started_at.elapsed())
    }

    pub(crate) fn should_force_recovery_before_turn_timeout(&self, reserve: Duration) -> bool {
        self.tool_calls > 0
            && !self.is_recovery_active()
            && self.turn_started_at.elapsed() >= self.turn_timeout.saturating_sub(reserve)
    }

    pub(crate) fn record_tool_call(&mut self) {
        self.tool_calls = self.tool_calls.saturating_add(1);
    }

    pub(crate) fn record_tool_call_with_warning(
        &mut self,
        threshold: f64,
    ) -> Option<ToolBudgetWarning> {
        self.record_tool_call();
        if !self.should_emit_tool_budget_warning(threshold) {
            return None;
        }

        let warning = ToolBudgetWarning {
            used: self.tool_calls,
            max: self.max_tool_calls,
            remaining: self.remaining_tool_calls(),
        };
        self.mark_tool_budget_warning_emitted();
        Some(warning)
    }

    pub(crate) fn record_tool_call_with_default_warning(&mut self) -> Option<ToolBudgetWarning> {
        self.record_tool_call_with_warning(TOOL_BUDGET_WARNING_THRESHOLD)
    }

    pub(crate) fn record_tool_budget_exhaustion_notice(
        &mut self,
    ) -> Option<ToolBudgetExhaustionNotice> {
        let exhaustion = self.tool_budget_exhaustion()?;
        let first_notice = !self.tool_budget_exhausted_emitted;
        if first_notice {
            self.mark_tool_budget_exhausted_emitted();
        }
        Some(ToolBudgetExhaustionNotice {
            exhaustion,
            first_notice,
        })
    }

    pub(crate) fn record_blocked_tool_call(&mut self) -> usize {
        self.blocked_tool_calls = self.blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls = self.consecutive_blocked_tool_calls.saturating_add(1);
        self.consecutive_blocked_tool_calls
    }

    pub(crate) fn reset_blocked_tool_call_streak(&mut self) {
        self.consecutive_blocked_tool_calls = 0;
    }

    pub(crate) fn tool_budget_usage_ratio(&self) -> f64 {
        if !self.has_tool_call_budget() {
            0.0
        } else {
            self.tool_calls as f64 / self.max_tool_calls as f64
        }
    }

    pub(crate) fn remaining_tool_calls(&self) -> usize {
        self.max_tool_calls.saturating_sub(self.tool_calls)
    }

    pub(crate) fn should_emit_tool_budget_warning(&self, threshold: f64) -> bool {
        self.has_tool_call_budget()
            && !self.tool_budget_warning_emitted
            && self.tool_budget_usage_ratio() >= threshold
    }

    pub(crate) fn mark_tool_budget_warning_emitted(&mut self) {
        self.tool_budget_warning_emitted = true;
    }

    pub(crate) fn mark_tool_budget_exhausted_emitted(&mut self) {
        self.tool_budget_exhausted_emitted = true;
    }

    pub(crate) fn activate_recovery(&mut self, reason: impl Into<String>) {
        self.activate_recovery_with_mode(reason, RecoveryMode::ToolFreeSynthesis);
    }

    pub(crate) fn activate_recovery_with_mode(
        &mut self,
        reason: impl Into<String>,
        mode: RecoveryMode,
    ) {
        if matches!(self.recovery_phase, RecoveryPhase::Inactive) {
            self.recovery_reason = Some(reason.into());
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_mode = Some(mode);
            self.recovery_retry_count = 0;
        }
    }

    pub(crate) fn is_recovery_active(&self) -> bool {
        matches!(
            self.recovery_phase,
            RecoveryPhase::Pending | RecoveryPhase::InPass
        )
    }

    pub(crate) fn recovery_reason(&self) -> Option<&str> {
        self.recovery_reason.as_deref()
    }

    pub(crate) fn recovery_pass_used(&self) -> bool {
        matches!(
            self.recovery_phase,
            RecoveryPhase::InPass | RecoveryPhase::Completed
        )
    }

    #[cfg(test)]
    pub(crate) fn recovery_mode(&self) -> Option<RecoveryMode> {
        self.recovery_mode
    }

    pub(crate) fn recovery_is_tool_free(&self) -> bool {
        matches!(self.recovery_mode, Some(RecoveryMode::ToolFreeSynthesis))
    }

    pub(crate) fn consume_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::Pending) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::InPass;
        true
    }

    pub(crate) fn finish_recovery_pass(&mut self) -> bool {
        if !matches!(self.recovery_phase, RecoveryPhase::InPass) {
            return false;
        }
        self.recovery_phase = RecoveryPhase::Completed;
        true
    }

    /// Retry the recovery pass by resetting the phase back to `Pending`
    /// so the next loop iteration re-enters tool-free recovery mode.
    /// Increments the retry counter; the caller is responsible for checking
    /// `recovery_retry_count()` against its own limit.
    /// Only works if a recovery pass has been consumed (phase is InPass or Completed).
    pub(crate) fn retry_recovery_pass(&mut self) -> bool {
        if matches!(
            self.recovery_phase,
            RecoveryPhase::InPass | RecoveryPhase::Completed
        ) {
            self.recovery_phase = RecoveryPhase::Pending;
            self.recovery_retry_count += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn recovery_retry_count(&self) -> u8 {
        self.recovery_retry_count
    }

    pub(crate) fn record_spool_chunk_read(&mut self) -> usize {
        self.consecutive_spool_chunk_reads = self.consecutive_spool_chunk_reads.saturating_add(1);
        self.consecutive_spool_chunk_reads
    }

    pub(crate) fn reset_spool_chunk_read_streak(&mut self) {
        self.consecutive_spool_chunk_reads = 0;
    }

    pub(crate) fn record_shell_command_run(&mut self, signature: String) -> usize {
        if self.last_shell_command_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_shell_command_runs =
                self.consecutive_same_shell_command_runs.saturating_add(1);
        } else {
            self.last_shell_command_signature = Some(signature);
            self.consecutive_same_shell_command_runs = 1;
        }

        self.consecutive_same_shell_command_runs
    }

    pub(crate) fn reset_shell_command_run_streak(&mut self) {
        self.last_shell_command_signature = None;
        self.consecutive_same_shell_command_runs = 0;
    }

    pub(crate) fn record_file_read_family_call(&mut self, signature: String) -> usize {
        if self.last_file_read_family_signature.as_deref() == Some(signature.as_str()) {
            self.consecutive_same_file_read_family_calls = self
                .consecutive_same_file_read_family_calls
                .saturating_add(1);
        } else {
            self.last_file_read_family_signature = Some(signature);
            self.consecutive_same_file_read_family_calls = 1;
        }

        self.consecutive_same_file_read_family_calls
    }

    pub(crate) fn reset_file_read_family_streak(&mut self) {
        self.last_file_read_family_signature = None;
        self.consecutive_same_file_read_family_calls = 0;
    }

    pub(crate) fn record_task_tracker_create_signature(&mut self, signature: String) -> bool {
        self.seen_task_tracker_create_signatures.insert(signature)
    }

    pub(crate) fn clear_task_tracker_create_signatures(&mut self) {
        self.seen_task_tracker_create_signatures.clear();
    }

    pub(crate) fn record_successful_readonly_signature(&mut self, signature: String) -> bool {
        self.seen_successful_readonly_signatures.insert(signature)
    }

    pub(crate) fn has_successful_readonly_signature(&self, signature: &str) -> bool {
        self.seen_successful_readonly_signatures.contains(signature)
    }

    pub(crate) fn remember_streamed_tool_call_items<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = (String, String)>,
    {
        self.streamed_tool_call_item_ids.extend(items);
    }

    pub(crate) fn take_streamed_tool_call_item_id(&mut self, tool_call_id: &str) -> Option<String> {
        self.streamed_tool_call_item_ids.remove(tool_call_id)
    }

    pub(crate) fn replaceable_task_tracker_count(&self) -> Option<usize> {
        let lines = self.replaceable_task_tracker_block.as_ref()?;
        vtcode_core::utils::transcript::tail_matches(lines).then_some(lines.len())
    }

    pub(crate) fn remember_task_tracker_block(&mut self, lines: Vec<String>) {
        self.replaceable_task_tracker_block = (!lines.is_empty()).then_some(lines);
    }

    pub(crate) fn set_phase(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }

    pub(crate) fn execution_snapshot(&self) -> TurnExecutionSnapshot {
        TurnExecutionSnapshot {
            run_id: self.run_id.0.clone(),
            turn_id: self.turn_id.0.clone(),
            phase: self.phase.into(),
            max_tool_calls: self.max_tool_calls,
            max_tool_wall_clock_secs: self.max_tool_wall_clock.as_secs(),
            max_tool_retries: self.max_tool_retries,
        }
    }
}

pub(crate) struct RunLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub tool_registry: &'a mut ToolRegistry,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub session_stats: &'a mut SessionStats,
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub mcp_panel_state: &'a mut McpPanelState,
    pub approval_recorder: &'a ApprovalRecorder,
    pub session: &'a mut InlineSession,
    pub safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
    pub traj: &'a TrajectoryLogger,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub auto_permission: Option<AutoPermissionRuntimeContext<'a>>,
    pub active_agent_permissions: Option<&'a AgentPermissionsConfig>,
}

pub(crate) struct AutoPermissionRuntimeContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub provider_client: &'a mut dyn uni::LLMProvider,
    pub working_history: &'a [uni::Message],
}

impl<'a> RunLoopContext<'a> {
    #[expect(clippy::too_many_arguments)]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        tool_registry: &'a mut ToolRegistry,
        _tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        session_stats: &'a mut SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        mcp_panel_state: &'a mut McpPanelState,
        approval_recorder: &'a ApprovalRecorder,
        session: &'a mut InlineSession,
        safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
    ) -> Self {
        Self::new_with_auto_permission_context(
            renderer,
            handle,
            tool_registry,
            _tools,
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            decision_ledger,
            session_stats,
            plan_session,
            mcp_panel_state,
            approval_recorder,
            session,
            safety_validator,
            traj,
            harness_state,
            harness_emitter,
            None,
        )
    }

    #[expect(clippy::too_many_arguments)]
    pub(crate) fn new_with_auto_permission_context(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        tool_registry: &'a mut ToolRegistry,
        _tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<PermissionsConfig>>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        session_stats: &'a mut SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        mcp_panel_state: &'a mut McpPanelState,
        approval_recorder: &'a ApprovalRecorder,
        session: &'a mut InlineSession,
        safety_validator: Option<&'a Arc<ToolCallSafetyValidator>>,
        traj: &'a TrajectoryLogger,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        auto_permission: Option<AutoPermissionRuntimeContext<'a>>,
    ) -> Self {
        Self {
            renderer,
            handle,
            tool_registry,
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
            decision_ledger,
            session_stats,
            plan_session,
            mcp_panel_state,
            approval_recorder,
            session,
            safety_validator,
            traj,
            harness_state,
            harness_emitter,
            auto_permission,
            active_agent_permissions: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HarnessTurnState, RecoveryMode, TOOL_BUDGET_WARNING_THRESHOLD, ToolBudgetExhaustion,
        ToolBudgetExhaustionNotice, ToolBudgetWarning, ToolWallClockExhaustion, TurnExecutionPhase,
        TurnId, TurnPhase, TurnRunId,
    };
    use std::time::{Duration, Instant};

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

        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 1/4
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 2/4
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.record_tool_call(); // 3/4 => 75%
        assert!(state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        state.mark_tool_budget_warning_emitted();
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
        assert_eq!(state.remaining_tool_calls(), 1);
    }

    #[test]
    fn harness_state_records_budget_warning_once_via_helper() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert_eq!(state.record_tool_call_with_default_warning(), None);
        assert_eq!(state.record_tool_call_with_default_warning(), None);
        assert_eq!(
            state.record_tool_call_with_default_warning(),
            Some(ToolBudgetWarning {
                used: 3,
                max: 4,
                remaining: 1,
            })
        );
        assert_eq!(state.record_tool_call_with_default_warning(), None);
    }

    #[test]
    fn tool_budget_warning_system_message_matches_contract() {
        assert_eq!(
            ToolBudgetWarning {
                used: 3,
                max: 4,
                remaining: 1,
            }
            .system_message(),
            "Tool-call budget warning: 3/4 used; 1 remaining for this turn. Use targeted extraction/batching before additional tool calls."
        );
    }

    #[test]
    fn harness_state_records_budget_exhaustion_notice_once_via_helper() {
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
        assert_eq!(
            state.record_tool_budget_exhaustion_notice(),
            Some(ToolBudgetExhaustionNotice {
                exhaustion: ToolBudgetExhaustion {
                    used: 1,
                    max: 1,
                    remaining: 0,
                },
                first_notice: true,
            })
        );
        assert!(state.tool_budget_exhausted_emitted);
        assert_eq!(
            state.record_tool_budget_exhaustion_notice(),
            Some(ToolBudgetExhaustionNotice {
                exhaustion: ToolBudgetExhaustion {
                    used: 1,
                    max: 1,
                    remaining: 0,
                },
                first_notice: false,
            })
        );
    }

    #[test]
    fn tool_budget_exhaustion_blocked_turn_reason_matches_contract() {
        assert_eq!(
            ToolBudgetExhaustion {
                used: 4,
                max: 4,
                remaining: 0,
            }
            .blocked_turn_reason(),
            "Tool-call budget exhausted for this turn (4/4). Start a new turn with \"continue\" or provide a new instruction to proceed."
        );
    }

    #[test]
    fn tool_wall_clock_exhaustion_policy_violation_message_matches_contract() {
        assert_eq!(
            ToolWallClockExhaustion { max_secs: 600 }.policy_violation_message(),
            "Policy violation: exceeded tool wall clock budget (600s)"
        );
    }

    #[test]
    fn harness_state_treats_zero_tool_budget_as_unlimited() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            0,
            10,
            1,
        );

        for _ in 0..8 {
            state.record_tool_call();
        }

        assert!(!state.has_tool_call_budget());
        assert!(!state.tool_budget_exhausted());
        assert_eq!(state.tool_budget_exhaustion(), None);
        assert!(!state.should_emit_tool_budget_warning(TOOL_BUDGET_WARNING_THRESHOLD));
    }

    #[test]
    fn harness_state_reports_wall_clock_budget_exhaustion() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert_eq!(state.wall_clock_budget_exhaustion(), None);
        state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(11)).unwrap();
        assert_eq!(
            state.wall_clock_budget_exhaustion(),
            Some(ToolWallClockExhaustion { max_secs: 10 })
        );
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

    #[test]
    fn harness_state_tracks_recovery_state() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert!(!state.is_recovery_active());
        assert!(!state.recovery_pass_used());

        state.activate_recovery("loop detector");
        assert!(state.is_recovery_active());
        assert_eq!(state.recovery_reason(), Some("loop detector"));
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolFreeSynthesis));
        assert!(state.recovery_is_tool_free());

        assert!(state.consume_recovery_pass());
        assert!(state.recovery_pass_used());
        assert!(state.finish_recovery_pass());
        assert!(!state.is_recovery_active());
    }

    #[test]
    fn harness_state_consumes_recovery_pass_once() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert!(!state.consume_recovery_pass());

        state.activate_recovery("loop detector");
        assert!(state.consume_recovery_pass());
        assert!(!state.consume_recovery_pass());
        assert!(state.recovery_pass_used());
        assert!(state.finish_recovery_pass());
        assert!(!state.finish_recovery_pass());
    }

    #[test]
    fn harness_state_force_recovery_before_turn_timeout_requires_tool_activity() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );
        state.set_turn_timeout_secs(60);
        state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(45)).unwrap();

        assert!(!state.should_force_recovery_before_turn_timeout(Duration::from_secs(20)));

        state.record_tool_call();
        assert!(state.should_force_recovery_before_turn_timeout(Duration::from_secs(20)));
    }

    #[test]
    fn harness_state_force_recovery_before_turn_timeout_skips_active_recovery() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );
        state.set_turn_timeout_secs(60);
        state.turn_started_at = Instant::now().checked_sub(Duration::from_secs(50)).unwrap();
        state.record_tool_call();
        state.activate_recovery("loop detector");

        assert!(!state.should_force_recovery_before_turn_timeout(Duration::from_secs(20)));
    }

    #[test]
    fn harness_state_supports_tool_enabled_recovery_retries() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        state.activate_recovery_with_mode("empty response", RecoveryMode::ToolEnabledRetry);

        assert!(state.is_recovery_active());
        assert_eq!(state.recovery_mode(), Some(RecoveryMode::ToolEnabledRetry));
        assert!(!state.recovery_is_tool_free());
        assert!(state.consume_recovery_pass());
        assert!(state.finish_recovery_pass());
    }

    #[test]
    fn harness_state_tracks_task_tracker_create_signatures() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert!(state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        ));
        assert!(!state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"x\"]}".to_string()
        ));
        assert!(state.record_task_tracker_create_signature(
            "task_tracker::create::{\"title\":\"A\",\"items\":[\"y\"]}".to_string()
        ));
    }

    #[test]
    fn harness_state_tracks_successful_readonly_signatures() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert!(!state.has_successful_readonly_signature("unified_file:ro:len10-fnv1234"));
        assert!(
            state.record_successful_readonly_signature("unified_file:ro:len10-fnv1234".to_string())
        );
        assert!(state.has_successful_readonly_signature("unified_file:ro:len10-fnv1234"));
        assert!(
            !state
                .record_successful_readonly_signature("unified_file:ro:len10-fnv1234".to_string())
        );
    }

    #[test]
    fn harness_state_tracks_identical_shell_command_streak() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert_eq!(
            state.record_shell_command_run("unified_exec::cargo check".to_string()),
            1
        );
        assert_eq!(
            state.record_shell_command_run("unified_exec::cargo check".to_string()),
            2
        );
        assert_eq!(
            state.record_shell_command_run("unified_exec::cargo test".to_string()),
            1
        );
        assert_eq!(
            state.last_shell_command_signature.as_deref(),
            Some("unified_exec::cargo test")
        );
    }

    #[test]
    fn harness_state_resets_shell_command_streak() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        state.record_shell_command_run("unified_exec::cargo check".to_string());
        state.reset_shell_command_run_streak();
        assert_eq!(state.consecutive_same_shell_command_runs, 0);
        assert!(state.last_shell_command_signature.is_none());
    }

    #[test]
    fn harness_state_tracks_file_read_family_streak() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-1".to_string()),
            TurnId("turn-1".to_string()),
            4,
            10,
            1,
        );

        assert_eq!(
            state.record_file_read_family_call("unified_file::read::src/lib.rs".to_string()),
            1
        );
        assert_eq!(
            state.record_file_read_family_call("unified_file::read::src/lib.rs".to_string()),
            2
        );
        assert_eq!(
            state.record_file_read_family_call("unified_file::read::src/main.rs".to_string()),
            1
        );

        state.reset_file_read_family_streak();
        assert_eq!(state.consecutive_same_file_read_family_calls, 0);
    }

    #[test]
    fn harness_state_builds_execution_snapshot() {
        let mut state = HarnessTurnState::new(
            TurnRunId("run-9".to_string()),
            TurnId("turn-3".to_string()),
            6,
            120,
            2,
        );
        state.set_phase(TurnPhase::ExecutingTools);

        let snapshot = state.execution_snapshot();
        assert_eq!(snapshot.run_id, "run-9");
        assert_eq!(snapshot.turn_id, "turn-3");
        assert_eq!(snapshot.phase, TurnExecutionPhase::ExecutingTools);
        assert_eq!(snapshot.max_tool_calls, 6);
        assert_eq!(snapshot.max_tool_wall_clock_secs, 120);
        assert_eq!(snapshot.max_tool_retries, 2);
    }
}
