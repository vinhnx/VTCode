use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::agent::error_recovery::ErrorRecoveryState;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_tui::EditingMode;

/// Default agent profile names for mode switching
pub const PLANNER_AGENT: &str = "planner";
pub const CODER_AGENT: &str = "coder";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModelPickerTarget {
    #[default]
    Main,
    SubagentDefault,
    TeamDefault,
}

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: std::collections::BTreeSet<String>,
    /// Current editing mode: Edit or Plan (legacy, for backward compatibility)
    pub editing_mode: EditingMode,
    /// Active agent profile name - the subagent config driving the main conversation
    /// This replaces EditingMode with a more flexible approach
    pub active_agent_name: String,
    /// Whether the plan mode interview has already been shown in this session
    plan_mode_interview_shown: bool,
    /// Whether the plan mode interview should be prompted after current tool work
    plan_mode_interview_pending: bool,
    /// Number of plan-mode turns observed since entering plan mode
    plan_mode_turns: usize,
    /// Autonomous mode - auto-approve safe tools with reduced HITL prompts
    pub autonomous_mode: bool,
    #[allow(dead_code)]
    pub approval_recorder: Arc<ApprovalRecorder>,
    #[allow(dead_code)]
    pub safety_validator: Arc<RwLock<ToolCallSafetyValidator>>,
    // Phase 4 Integration: Resilient execution components
    pub circuit_breaker: Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub validation_cache: Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    /// Error recovery state for circuit breaker recovery flow
    #[allow(dead_code)]
    pub error_recovery: Arc<RwLock<ErrorRecoveryState>>,

    /// Agent teams state (in-process only)
    pub team_state: Option<crate::agent::runloop::unified::team_state::TeamState>,

    /// Team context for this session (lead/teammate)
    pub team_context: Option<vtcode_core::agent_teams::TeamContext>,

    /// In-process teammate runners (persistent tokio tasks)
    pub in_process_runner: Option<vtcode_core::agent_teams::InProcessTeamRunner>,

    /// Delegate mode toggle (team coordination only)
    pub delegate_mode: bool,

    /// Target configuration for the active model picker
    pub model_picker_target: ModelPickerTarget,
    /// Count of consecutive minimal follow-up prompts (e.g. "continue", "retry")
    follow_up_prompt_streak: usize,
    /// One-shot guard to avoid classifying injected recovery prompts as user follow-ups
    suppress_next_follow_up_prompt: bool,
    /// Whether the last turn ended in a stalled state (aborted/blocked)
    turn_stalled: bool,
    /// Reason associated with the last stalled turn, when available
    turn_stall_reason: Option<String>,
    /// Whether context clear was requested after plan approval
    pending_context_clear: bool,
}

impl SessionStats {
    pub(crate) fn record_tool(&mut self, name: &str) {
        let normalized_name = match name {
            n if n == tool_names::UNIFIED_EXEC
                || n == tool_names::SHELL
                || n == tool_names::EXEC_PTY_CMD =>
            {
                tool_names::RUN_PTY_CMD
            }
            _ => name,
        };
        self.tools.insert(normalized_name.to_string());
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    pub(crate) fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }

    /// Check if currently in Plan mode (read-only)
    pub(crate) fn is_plan_mode(&self) -> bool {
        matches!(self.editing_mode, EditingMode::Plan)
    }

    /// Check if currently in autonomous mode
    pub(crate) fn is_autonomous_mode(&self) -> bool {
        self.autonomous_mode
    }

    pub(crate) fn is_delegate_mode(&self) -> bool {
        self.delegate_mode
    }

    pub(crate) fn toggle_delegate_mode(&mut self) -> bool {
        self.delegate_mode = !self.delegate_mode;
        self.delegate_mode
    }

    /// Set plan mode (for backward compatibility)
    pub(crate) fn set_plan_mode(&mut self, enabled: bool) {
        self.editing_mode = if enabled {
            EditingMode::Plan
        } else {
            EditingMode::Edit
        };
        if enabled {
            self.plan_mode_interview_shown = false;
            self.plan_mode_interview_pending = false;
            self.plan_mode_turns = 0;
            self.tools.clear();
        }
    }

    /// Set autonomous mode
    pub(crate) fn set_autonomous_mode(&mut self, enabled: bool) {
        self.autonomous_mode = enabled;
    }

    /// Cycle to the next mode: Edit → Plan → Edit
    pub(crate) fn cycle_mode(&mut self) -> EditingMode {
        self.editing_mode = self.editing_mode.next();
        self.sync_active_agent_from_mode();
        self.editing_mode
    }

    /// Get the active agent profile name
    pub(crate) fn active_agent(&self) -> &str {
        if self.active_agent_name.is_empty() {
            CODER_AGENT
        } else {
            &self.active_agent_name
        }
    }

    /// Set the active agent profile by name
    /// This also syncs the legacy EditingMode for backward compatibility
    pub(crate) fn set_active_agent(&mut self, name: &str) {
        self.active_agent_name = name.to_string();
        self.sync_mode_from_active_agent();
    }

    /// Sync legacy EditingMode from active agent (for backward compatibility)
    fn sync_mode_from_active_agent(&mut self) {
        self.editing_mode = if self.active_agent() == PLANNER_AGENT {
            EditingMode::Plan
        } else {
            EditingMode::Edit
        };
    }

    /// Sync active agent from legacy EditingMode (for backward compatibility)
    fn sync_active_agent_from_mode(&mut self) {
        self.active_agent_name = match self.editing_mode {
            EditingMode::Plan => PLANNER_AGENT.to_string(),
            EditingMode::Edit => CODER_AGENT.to_string(),
        };
    }

    /// Switch to planner agent (convenience method)
    pub(crate) fn switch_to_planner(&mut self) {
        self.set_active_agent(PLANNER_AGENT);
    }

    pub(crate) fn plan_mode_interview_shown(&self) -> bool {
        self.plan_mode_interview_shown
    }

    pub(crate) fn mark_plan_mode_interview_shown(&mut self) {
        self.plan_mode_interview_shown = true;
        self.plan_mode_interview_pending = false;
    }

    pub(crate) fn plan_mode_turns(&self) -> usize {
        self.plan_mode_turns
    }

    pub(crate) fn increment_plan_mode_turns(&mut self) {
        self.plan_mode_turns = self.plan_mode_turns.saturating_add(1);
    }

    pub(crate) fn plan_mode_interview_pending(&self) -> bool {
        self.plan_mode_interview_pending
    }

    pub(crate) fn mark_plan_mode_interview_pending(&mut self) {
        self.plan_mode_interview_pending = true;
    }

    /// Switch to coder agent (convenience method)
    pub(crate) fn switch_to_coder(&mut self) {
        self.set_active_agent(CODER_AGENT);
    }

    pub(crate) fn register_follow_up_prompt(&mut self, input: &str) -> bool {
        let suppression_active = self.consume_follow_up_prompt_suppression();

        let normalized = input
            .trim()
            .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
            .to_ascii_lowercase();
        let words: Vec<&str> = normalized.split_whitespace().collect();
        let is_follow_up = matches!(
            words.as_slice(),
            ["continue"]
                | ["retry"]
                | ["proceed"]
                | ["go", "on"]
                | ["go", "ahead"]
                | ["keep", "going"]
                | ["please", "continue"]
                | ["continue", "please"]
                | ["please", "retry"]
                | ["retry", "please"]
                | ["continue", "with", "recommendation"]
                | ["continue", "with", "your", "recommendation"]
        );

        if is_follow_up {
            if suppression_active {
                return false;
            }
            self.follow_up_prompt_streak = self.follow_up_prompt_streak.saturating_add(1);
        } else {
            self.follow_up_prompt_streak = 0;
            self.turn_stalled = false;
            self.turn_stall_reason = None;
        }

        let threshold = if self.turn_stalled { 1 } else { 3 };
        is_follow_up && self.follow_up_prompt_streak >= threshold
    }

    pub(crate) fn mark_turn_stalled(&mut self, stalled: bool, reason: Option<String>) {
        self.turn_stalled = stalled;
        if !stalled {
            self.follow_up_prompt_streak = 0;
            self.suppress_next_follow_up_prompt = false;
            self.turn_stall_reason = None;
        } else {
            self.turn_stall_reason = reason;
        }
    }

    pub(crate) fn turn_stalled(&self) -> bool {
        self.turn_stalled
    }

    pub(crate) fn turn_stall_reason(&self) -> Option<&str> {
        self.turn_stall_reason.as_deref()
    }

    pub(crate) fn suppress_next_follow_up_prompt(&mut self) {
        self.suppress_next_follow_up_prompt = true;
    }

    fn consume_follow_up_prompt_suppression(&mut self) -> bool {
        std::mem::take(&mut self.suppress_next_follow_up_prompt)
    }

    pub(crate) fn request_context_clear(&mut self) {
        self.pending_context_clear = true;
    }

    pub(crate) fn take_context_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_context_clear)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtrlCSignal {
    Cancel,
    Exit,
}

#[derive(Default)]
pub(crate) struct CtrlCState {
    cancel_requested: AtomicBool,
    exit_requested: AtomicBool,
    exit_armed: AtomicBool,
    last_signal_time: AtomicU64,
}

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_secs(2);

impl CtrlCState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);

        // Debounce: ignore signals within 200ms of each other
        if last > 0 && now.saturating_sub(last) < 200 {
            if self.exit_requested.load(Ordering::SeqCst) {
                return CtrlCSignal::Exit;
            }
            if self.cancel_requested.load(Ordering::SeqCst) {
                return CtrlCSignal::Cancel;
            }
        }

        let window_ms = DOUBLE_CTRL_C_WINDOW.as_millis() as u64;
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_ms;

        if (self.cancel_requested.load(Ordering::SeqCst) || self.exit_armed.load(Ordering::SeqCst))
            && is_within_window
        {
            self.exit_requested.store(true, Ordering::SeqCst);
            CtrlCSignal::Exit
        } else {
            self.cancel_requested.store(true, Ordering::SeqCst);
            self.exit_armed.store(true, Ordering::SeqCst);
            CtrlCSignal::Cancel
        }
    }

    pub(crate) fn clear_cancel(&self) {
        self.cancel_requested.store(false, Ordering::SeqCst);
        self.exit_requested.store(false, Ordering::SeqCst);
        self.exit_armed.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        self.cancel_requested.load(Ordering::Relaxed)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::Relaxed)
    }

    /// Check if cancellation or exit has been requested and return an error if so
    pub(crate) fn check_cancellation(&self) -> anyhow::Result<()> {
        if self.is_exit_requested() {
            anyhow::bail!("Exit requested");
        }
        if self.is_cancel_requested() {
            anyhow::bail!("Operation cancelled");
        }
        Ok(())
    }

    pub(crate) fn disarm_exit(&self) {
        self.exit_armed.store(false, Ordering::SeqCst);
        self.last_signal_time.store(0, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::SessionStats;
    use vtcode_core::config::constants::tools;

    #[test]
    fn record_tool_normalizes_exec_aliases() {
        let mut stats = SessionStats::default();
        stats.record_tool(tools::UNIFIED_EXEC);
        stats.record_tool(tools::SHELL);
        stats.record_tool(tools::EXEC_PTY_CMD);

        assert_eq!(stats.sorted_tools(), vec![tools::RUN_PTY_CMD.to_string()]);
    }

    #[test]
    fn follow_up_prompts_force_conclusion_after_stall() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));

        assert!(stats.register_follow_up_prompt("continue"));
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));
    }

    #[test]
    fn non_follow_up_resets_follow_up_tracking() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn aborted".to_string()));
        let _ = stats.register_follow_up_prompt("continue");
        let _ = stats.register_follow_up_prompt("continue");
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn aborted"));

        assert!(!stats.register_follow_up_prompt("run tests and summarize"));
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }

    #[test]
    fn follow_up_prompt_variants_are_detected() {
        let mut stats = SessionStats::default();
        assert!(!stats.register_follow_up_prompt("continue."));
        assert!(!stats.register_follow_up_prompt("continue with your recommendation"));
        assert!(stats.register_follow_up_prompt("please continue"));
    }

    #[test]
    fn suppressed_follow_up_prompt_is_ignored_once() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
        stats.suppress_next_follow_up_prompt();

        assert!(!stats.register_follow_up_prompt("continue"));
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));

        assert!(stats.register_follow_up_prompt("continue"));
    }

    #[test]
    fn suppressed_non_follow_up_still_clears_stall_state() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
        stats.suppress_next_follow_up_prompt();

        assert!(!stats.register_follow_up_prompt("run tests and summarize"));
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }
}
