use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_tui::app::EditingMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ModelPickerTarget {
    #[default]
    Main,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionMode {
    Edit,
    TrustedAuto,
    Plan,
}

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: std::collections::BTreeSet<String>,
    pub task_panel_visible: bool,
    /// Current editing mode: Edit or Plan.
    pub editing_mode: EditingMode,
    /// Whether the plan mode interview has already been shown in this session
    plan_mode_interview_shown: bool,
    /// Whether the plan mode interview should be prompted after current tool work
    plan_mode_interview_pending: bool,
    /// Number of plan-mode turns observed since entering plan mode
    plan_mode_turns: usize,
    /// Number of completed plan-mode interview cycles
    plan_mode_interview_cycles_completed: usize,
    /// Whether the latest plan-mode interview cycle was cancelled/incomplete
    plan_mode_last_interview_cancelled: bool,
    /// Source that triggered plan mode entry.
    plan_mode_entry_source: Option<PlanModeEntrySource>,
    /// Autonomous mode - auto-approve safe tools with reduced HITL prompts
    pub autonomous_mode: bool,
    /// Whether Vim-style prompt editing is enabled for this session.
    pub vim_mode_enabled: bool,
    // Phase 4 Integration: Resilient execution components
    pub circuit_breaker: Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub validation_cache: Arc<vtcode_core::tools::validation_cache::ValidationCache>,

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
    /// Provider-scoped previous response ID for Responses-style server-side chaining.
    previous_response_id: Option<String>,
    previous_response_provider: Option<String>,
    previous_response_model: Option<String>,
    recent_touched_files: VecDeque<String>,
}

impl SessionStats {
    pub(crate) fn record_tool(&mut self, name: &str) {
        let normalized_name =
            vtcode_core::tools::tool_intent::canonical_unified_exec_tool_name(name).unwrap_or(name);
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

    pub(crate) fn current_mode(&self) -> SessionMode {
        match (self.editing_mode, self.autonomous_mode) {
            (EditingMode::Plan, _) => SessionMode::Plan,
            (EditingMode::Edit, true) => SessionMode::TrustedAuto,
            (EditingMode::Edit, false) => SessionMode::Edit,
        }
    }

    pub(crate) fn set_autonomous_mode(&mut self, enabled: bool) {
        self.autonomous_mode = enabled && !self.is_plan_mode();
    }

    /// Set plan mode.
    pub(crate) fn set_plan_mode(&mut self, enabled: bool) {
        self.editing_mode = if enabled {
            EditingMode::Plan
        } else {
            EditingMode::Edit
        };
        self.autonomous_mode = false;
        if enabled {
            self.plan_mode_interview_shown = false;
            self.plan_mode_interview_pending = false;
            self.plan_mode_turns = 0;
            self.plan_mode_interview_cycles_completed = 0;
            self.plan_mode_last_interview_cancelled = false;
            self.tools.clear();
            self.clear_previous_response_chain();
        }
        if !enabled {
            self.plan_mode_entry_source = None;
        }
    }

    pub(crate) fn set_plan_mode_entry_source(&mut self, source: PlanModeEntrySource) {
        self.plan_mode_entry_source = Some(source);
    }

    /// Cycle to the next mode: Edit -> Trusted Auto -> Plan -> Edit
    pub(crate) fn cycle_mode(&mut self) -> SessionMode {
        match self.current_mode() {
            SessionMode::Edit => {
                self.editing_mode = EditingMode::Edit;
                self.autonomous_mode = true;
                SessionMode::TrustedAuto
            }
            SessionMode::TrustedAuto => {
                self.set_plan_mode(true);
                SessionMode::Plan
            }
            SessionMode::Plan => {
                self.set_plan_mode(false);
                SessionMode::Edit
            }
        }
    }

    #[cfg(test)]
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

    pub(crate) fn clear_plan_mode_interview_pending(&mut self) {
        self.plan_mode_interview_pending = false;
    }

    pub(crate) fn record_plan_mode_interview_result(
        &mut self,
        answered_questions: usize,
        cancelled: bool,
    ) {
        let answered_questions = answered_questions.min(3);
        self.plan_mode_last_interview_cancelled = cancelled || answered_questions == 0;
        self.plan_mode_interview_pending = false;

        if !self.plan_mode_last_interview_cancelled {
            self.plan_mode_interview_cycles_completed =
                self.plan_mode_interview_cycles_completed.saturating_add(1);
            self.plan_mode_interview_shown = true;
        } else {
            self.plan_mode_interview_shown = false;
        }
    }

    pub(crate) fn plan_mode_interview_cycles_completed(&self) -> usize {
        self.plan_mode_interview_cycles_completed
    }

    pub(crate) fn plan_mode_last_interview_cancelled(&self) -> bool {
        self.plan_mode_last_interview_cancelled
    }

    pub(crate) fn register_follow_up_prompt(&mut self, input: &str) -> bool {
        let suppression_active = self.consume_follow_up_prompt_suppression();
        let is_follow_up = is_follow_up_prompt_like(input);

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

    pub(crate) fn previous_response_id_for(&self, provider: &str, model: &str) -> Option<String> {
        if self.previous_response_provider.as_deref() == Some(provider)
            && self.previous_response_model.as_deref() == Some(model)
        {
            return self.previous_response_id.clone();
        }
        None
    }

    pub(crate) fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
    ) {
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.clear_previous_response_chain();
            return;
        };

        self.previous_response_provider = Some(provider.to_string());
        self.previous_response_model = Some(model.to_string());
        self.previous_response_id = Some(response_id.to_string());
    }

    pub(crate) fn clear_previous_response_chain(&mut self) {
        self.previous_response_provider = None;
        self.previous_response_model = None;
        self.previous_response_id = None;
    }

    pub(crate) fn record_touched_files<I, S>(&mut self, files: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for file in files {
            let file = file.into();
            let normalized = file.trim();
            if normalized.is_empty() {
                continue;
            }

            if let Some(existing) = self
                .recent_touched_files
                .iter()
                .position(|entry| entry == normalized)
            {
                let _ = self.recent_touched_files.remove(existing);
            }

            self.recent_touched_files.push_back(normalized.to_string());
            while self.recent_touched_files.len() > 5 {
                let _ = self.recent_touched_files.pop_front();
            }
        }
    }

    pub(crate) fn recent_touched_files(&self) -> Vec<String> {
        self.recent_touched_files.iter().cloned().collect()
    }
}

pub(crate) fn is_follow_up_prompt_like(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|c: char| c.is_ascii_whitespace() || c.is_ascii_punctuation())
        .to_ascii_lowercase();
    if normalized.starts_with("continue autonomously from the last stalled turn") {
        return true;
    }
    let words: Vec<&str> = normalized.split_whitespace().collect();
    matches!(
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
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CtrlCSignal {
    Cancel,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
enum CtrlCPhase {
    #[default]
    Idle = 0,
    CancelRequested = 1,
    ExitArmed = 2,
    ExitRequested = 3,
}

impl CtrlCPhase {
    fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::CancelRequested,
            2 => Self::ExitArmed,
            3 => Self::ExitRequested,
            _ => Self::Idle,
        }
    }

    fn signal(self) -> CtrlCSignal {
        match self {
            Self::ExitRequested => CtrlCSignal::Exit,
            Self::Idle | Self::CancelRequested | Self::ExitArmed => CtrlCSignal::Cancel,
        }
    }
}

#[derive(Default)]
pub(crate) struct CtrlCState {
    phase: AtomicU8,
    last_signal_time: AtomicU64,
}

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_secs(2);

impl CtrlCState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn phase(&self) -> CtrlCPhase {
        CtrlCPhase::from_raw(self.phase.load(Ordering::SeqCst))
    }

    fn set_phase(&self, phase: CtrlCPhase) {
        self.phase.store(phase as u8, Ordering::SeqCst);
    }

    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);
        let current_phase = self.phase();

        // Debounce repeated cancel signals, but allow an already-armed stop to
        // escalate immediately so a quick second press can still exit.
        if last > 0 && now.saturating_sub(last) < 200 {
            if matches!(
                current_phase,
                CtrlCPhase::ExitArmed | CtrlCPhase::ExitRequested
            ) {
                self.set_phase(CtrlCPhase::ExitRequested);
                return CtrlCSignal::Exit;
            }
            return current_phase.signal();
        }

        let window_ms = DOUBLE_CTRL_C_WINDOW.as_millis() as u64;
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_ms;

        if matches!(
            current_phase,
            CtrlCPhase::CancelRequested | CtrlCPhase::ExitArmed
        ) && is_within_window
        {
            self.set_phase(CtrlCPhase::ExitRequested);
            return CtrlCSignal::Exit;
        }

        if matches!(current_phase, CtrlCPhase::ExitRequested) {
            return CtrlCSignal::Exit;
        }

        self.set_phase(CtrlCPhase::CancelRequested);
        CtrlCSignal::Cancel
    }

    pub(crate) fn reset(&self) {
        self.set_phase(CtrlCPhase::Idle);
        self.last_signal_time.store(0, Ordering::SeqCst);
    }

    pub(crate) fn mark_cancel_handled(&self) {
        if matches!(self.phase(), CtrlCPhase::CancelRequested) {
            self.set_phase(CtrlCPhase::ExitArmed);
        }
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::CancelRequested)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::ExitRequested)
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
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::{CtrlCSignal, CtrlCState, SessionMode, SessionStats, is_follow_up_prompt_like};
    use vtcode_core::config::constants::tools;
    use vtcode_tui::app::EditingMode;

    #[test]
    fn record_tool_normalizes_exec_aliases() {
        let mut stats = SessionStats::default();
        stats.record_tool(tools::UNIFIED_EXEC);
        stats.record_tool("shell");
        stats.record_tool("exec_pty_cmd");
        stats.record_tool(tools::EXEC_COMMAND);

        assert_eq!(stats.sorted_tools(), vec![tools::UNIFIED_EXEC.to_string()]);
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

    #[test]
    fn helper_detects_follow_up_variants() {
        assert!(is_follow_up_prompt_like("continue"));
        assert!(is_follow_up_prompt_like("continue."));
        assert!(is_follow_up_prompt_like("please continue"));
        assert!(is_follow_up_prompt_like(
            "Continue autonomously from the last stalled turn. Stall reason: x."
        ));
        assert!(!is_follow_up_prompt_like("run tests and summarize"));
    }

    #[test]
    fn plan_mode_interview_result_updates_cycle_metrics() {
        let mut stats = SessionStats::default();
        stats.set_plan_mode(true);

        stats.record_plan_mode_interview_result(2, false);
        assert_eq!(stats.plan_mode_interview_cycles_completed(), 1);
        assert!(!stats.plan_mode_last_interview_cancelled());

        stats.record_plan_mode_interview_result(0, true);
        assert_eq!(stats.plan_mode_interview_cycles_completed(), 1);
        assert!(stats.plan_mode_last_interview_cancelled());
    }

    #[test]
    fn entering_plan_mode_resets_interview_cycle_metrics() {
        let mut stats = SessionStats::default();
        stats.set_plan_mode(true);
        stats.record_plan_mode_interview_result(1, false);
        assert_eq!(stats.plan_mode_interview_cycles_completed(), 1);

        stats.set_plan_mode(false);
        stats.set_plan_mode(true);
        assert_eq!(stats.plan_mode_interview_cycles_completed(), 0);
        assert!(!stats.plan_mode_last_interview_cancelled());
    }

    #[test]
    fn cycle_mode_rotates_edit_trusted_auto_plan() {
        let mut stats = SessionStats::default();

        assert_eq!(stats.current_mode(), SessionMode::Edit);
        assert_eq!(stats.cycle_mode(), SessionMode::TrustedAuto);
        assert!(stats.is_autonomous_mode());
        assert_eq!(stats.editing_mode, EditingMode::Edit);

        assert_eq!(stats.cycle_mode(), SessionMode::Plan);
        assert!(stats.is_plan_mode());
        assert!(!stats.is_autonomous_mode());

        assert_eq!(stats.cycle_mode(), SessionMode::Edit);
        assert_eq!(stats.current_mode(), SessionMode::Edit);
        assert!(!stats.is_autonomous_mode());
    }

    #[test]
    fn ctrl_c_state_escalates_to_exit_within_window() {
        let state = CtrlCState::new();

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
    }

    #[test]
    fn ctrl_c_state_reset_clears_exit_window() {
        let state = CtrlCState::new();

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        state.reset();
        thread::sleep(Duration::from_millis(250));

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        assert!(state.is_cancel_requested());
        assert!(!state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_mark_cancel_handled_keeps_exit_window_armed() {
        let state = CtrlCState::new();

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        state.mark_cancel_handled();
        thread::sleep(Duration::from_millis(250));

        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_allows_immediate_exit_after_cancel_handled() {
        let state = CtrlCState::new();

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        state.mark_cancel_handled();

        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());
    }
}
