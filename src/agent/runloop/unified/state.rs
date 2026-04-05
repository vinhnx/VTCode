use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider::{Message, ResponsesContinuationState, responses_continuation_key};
use vtcode_tui::app::EditingMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ModelPickerTarget {
    #[default]
    Main,
    Lightweight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionMode {
    Edit,
    Auto,
    Plan,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutoModeDenial {
    pub stage: &'static str,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub matched_exception: Option<String>,
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
    /// Auto-mode classifier consecutive denial count.
    auto_mode_consecutive_denials: u32,
    /// Auto-mode classifier total denial count.
    auto_mode_total_denials: u32,
    /// Auto mode has fallen back to manual prompts for the rest of the session.
    auto_mode_prompt_fallback: bool,
    /// Most recent auto-mode classifier denial.
    last_auto_mode_denial: Option<AutoModeDenial>,
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
    /// Responses-style continuation state keyed by normalized provider/model pairs.
    previous_response_chains: HashMap<(String, String), ResponsesContinuationState>,
    prompt_cache_lineage_id: Option<String>,
    last_prompt_cache_model: Option<String>,
    last_stable_prefix_hash: Option<u64>,
    last_tool_catalog_hash: Option<u64>,
    last_prompt_cache_change_reason: Option<String>,
    prompt_cache_observations: usize,
    prompt_cache_model_changes: usize,
    prompt_cache_unchanged: usize,
    prompt_cache_stable_prefix_changes: usize,
    prompt_cache_tool_catalog_changes: usize,
    prompt_cache_combined_changes: usize,
    recent_touched_files: VecDeque<String>,
    total_usage: HarnessUsage,
    total_cost_usd: Option<f64>,
    cost_warning_emitted: bool,
    stop_reason: Option<String>,
    budget_limit: Option<(f64, f64)>,
    total_turns: usize,
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

    pub(crate) fn record_usage(&mut self, usage: &Option<vtcode_core::llm::provider::Usage>) {
        let Some(usage) = usage else {
            return;
        };

        self.total_usage.input_tokens = self
            .total_usage
            .input_tokens
            .saturating_add(usage.prompt_tokens as u64);
        self.total_usage.cached_input_tokens = self
            .total_usage
            .cached_input_tokens
            .saturating_add(usage.cache_read_tokens_or_fallback() as u64);
        self.total_usage.cache_creation_tokens = self
            .total_usage
            .cache_creation_tokens
            .saturating_add(usage.cache_creation_tokens_or_zero() as u64);
        self.total_usage.output_tokens = self
            .total_usage
            .output_tokens
            .saturating_add(usage.completion_tokens as u64);
    }

    pub(crate) fn total_usage(&self) -> HarnessUsage {
        self.total_usage.clone()
    }

    pub(crate) fn set_total_cost_usd(&mut self, cost: Option<f64>) {
        self.total_cost_usd = cost;
    }

    pub(crate) fn total_cost_usd(&self) -> Option<f64> {
        self.total_cost_usd
    }

    pub(crate) fn set_stop_reason(&mut self, reason: Option<String>) {
        self.stop_reason = reason;
    }

    pub(crate) fn stop_reason(&self) -> Option<&str> {
        self.stop_reason.as_deref()
    }

    pub(crate) fn cost_warning_emitted(&self) -> bool {
        self.cost_warning_emitted
    }

    pub(crate) fn mark_cost_warning_emitted(&mut self) {
        self.cost_warning_emitted = true;
    }

    pub(crate) fn mark_budget_limit_reached(&mut self, max_budget_usd: f64, actual_cost_usd: f64) {
        self.budget_limit = Some((max_budget_usd, actual_cost_usd));
    }

    pub(crate) fn budget_limit(&self) -> Option<(f64, f64)> {
        self.budget_limit
    }

    pub(crate) fn record_turn_completed(&mut self) {
        self.total_turns = self.total_turns.saturating_add(1);
    }

    pub(crate) fn total_turns(&self) -> usize {
        self.total_turns
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
            (EditingMode::Edit, true) => SessionMode::Auto,
            (EditingMode::Edit, false) => SessionMode::Edit,
        }
    }

    pub(crate) fn set_autonomous_mode(&mut self, enabled: bool) {
        self.autonomous_mode = enabled && !self.is_plan_mode();
        if !self.autonomous_mode {
            self.reset_auto_mode_review_state();
        }
    }

    /// Set plan mode.
    pub(crate) fn set_plan_mode(&mut self, enabled: bool) {
        self.editing_mode = if enabled {
            EditingMode::Plan
        } else {
            EditingMode::Edit
        };
        self.autonomous_mode = false;
        self.reset_auto_mode_review_state();
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

    /// Cycle to the next mode: Edit -> Auto -> Plan -> Edit
    #[cfg(test)]
    pub(crate) fn cycle_mode(&mut self) -> SessionMode {
        match self.current_mode() {
            SessionMode::Edit => {
                self.editing_mode = EditingMode::Edit;
                self.autonomous_mode = true;
                SessionMode::Auto
            }
            SessionMode::Auto => {
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
        self.previous_response_chain_for(provider, model)
            .map(|chain| chain.response_id.clone())
    }

    pub(crate) fn previous_response_chain_for(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<&ResponsesContinuationState> {
        responses_continuation_key(provider, model)
            .and_then(|key| self.previous_response_chains.get(&key))
    }

    pub(crate) fn set_prompt_cache_lineage_id(&mut self, lineage_id: Option<String>) {
        self.prompt_cache_lineage_id = lineage_id;
    }

    pub(crate) fn prompt_cache_lineage_id(&self) -> Option<&str> {
        self.prompt_cache_lineage_id.as_deref()
    }

    pub(crate) fn prompt_cache_diagnostics(&self) -> PromptCacheDiagnostics {
        PromptCacheDiagnostics {
            observations: self.prompt_cache_observations,
            model_changes: self.prompt_cache_model_changes,
            unchanged: self.prompt_cache_unchanged,
            stable_prefix_changes: self.prompt_cache_stable_prefix_changes,
            tool_catalog_changes: self.prompt_cache_tool_catalog_changes,
            combined_changes: self.prompt_cache_combined_changes,
            last_change_reason: self.last_prompt_cache_change_reason.clone(),
            last_stable_prefix_hash: self.last_stable_prefix_hash,
            last_tool_catalog_hash: self.last_tool_catalog_hash,
        }
    }

    pub(crate) fn record_prompt_cache_fingerprint(
        &mut self,
        model: &str,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
    ) -> &'static str {
        let reason = if self.last_prompt_cache_model.as_deref() != Some(model) {
            "model"
        } else {
            match (
                self.last_stable_prefix_hash == Some(stable_prefix_hash),
                self.last_tool_catalog_hash == tool_catalog_hash,
            ) {
                (true, true) => "unchanged",
                (false, true) => "stable_prefix",
                (true, false) => "tool_catalog",
                (false, false) => "stable_prefix+tool_catalog",
            }
        };

        self.prompt_cache_observations = self.prompt_cache_observations.saturating_add(1);
        match reason {
            "model" => {
                self.prompt_cache_model_changes = self.prompt_cache_model_changes.saturating_add(1);
            }
            "unchanged" => {
                self.prompt_cache_unchanged = self.prompt_cache_unchanged.saturating_add(1);
            }
            "stable_prefix" => {
                self.prompt_cache_stable_prefix_changes =
                    self.prompt_cache_stable_prefix_changes.saturating_add(1);
            }
            "tool_catalog" => {
                self.prompt_cache_tool_catalog_changes =
                    self.prompt_cache_tool_catalog_changes.saturating_add(1);
            }
            "stable_prefix+tool_catalog" => {
                self.prompt_cache_combined_changes =
                    self.prompt_cache_combined_changes.saturating_add(1);
            }
            _ => {}
        }

        self.last_prompt_cache_model = Some(model.to_string());
        self.last_stable_prefix_hash = Some(stable_prefix_hash);
        self.last_tool_catalog_hash = tool_catalog_hash;
        self.last_prompt_cache_change_reason = Some(reason.to_string());

        reason
    }

    pub(crate) fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: &[Message],
    ) {
        let Some(key) = responses_continuation_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.previous_response_chains.remove(&key);
            return;
        };

        self.previous_response_chains.insert(
            key,
            ResponsesContinuationState {
                response_id: response_id.to_string(),
                messages: messages.to_vec(),
            },
        );
    }

    pub(crate) fn clear_previous_response_chain_for(&mut self, provider: &str, model: &str) {
        if let Some(key) = responses_continuation_key(provider, model) {
            self.previous_response_chains.remove(&key);
        }
    }

    pub(crate) fn clear_previous_response_chain(&mut self) {
        self.previous_response_chains.clear();
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

    pub(crate) fn auto_mode_prompt_fallback_active(&self) -> bool {
        self.auto_mode_prompt_fallback
    }

    pub(crate) fn last_auto_mode_denial(&self) -> Option<&AutoModeDenial> {
        self.last_auto_mode_denial.as_ref()
    }

    pub(crate) fn reset_auto_mode_review_state(&mut self) {
        self.auto_mode_consecutive_denials = 0;
        self.auto_mode_total_denials = 0;
        self.auto_mode_prompt_fallback = false;
        self.last_auto_mode_denial = None;
    }

    pub(crate) fn record_auto_mode_allow(&mut self) {
        self.auto_mode_consecutive_denials = 0;
        self.last_auto_mode_denial = None;
    }

    pub(crate) fn record_auto_mode_denial(
        &mut self,
        denial: AutoModeDenial,
        max_consecutive_denials: u32,
        max_total_denials: u32,
    ) -> bool {
        self.auto_mode_consecutive_denials = self.auto_mode_consecutive_denials.saturating_add(1);
        self.auto_mode_total_denials = self.auto_mode_total_denials.saturating_add(1);
        self.last_auto_mode_denial = Some(denial);
        self.auto_mode_prompt_fallback = self.auto_mode_consecutive_denials
            >= max_consecutive_denials.max(1)
            || self.auto_mode_total_denials >= max_total_denials.max(1);
        self.auto_mode_prompt_fallback
    }
}

pub(crate) fn should_enforce_safe_mode_prompts(
    full_auto: bool,
    autonomous_mode: bool,
    workspace_trust_level: Option<WorkspaceTrustLevel>,
) -> bool {
    if full_auto || autonomous_mode {
        return false;
    }

    !matches!(workspace_trust_level, Some(WorkspaceTrustLevel::FullAuto))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PromptCacheDiagnostics {
    pub observations: usize,
    pub model_changes: usize,
    pub unchanged: usize,
    pub stable_prefix_changes: usize,
    pub tool_catalog_changes: usize,
    pub combined_changes: usize,
    pub last_change_reason: Option<String>,
    pub last_stable_prefix_hash: Option<u64>,
    pub last_tool_catalog_hash: Option<u64>,
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

const DOUBLE_CTRL_C_WINDOW: Duration = Duration::from_millis(1000);

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

    use super::{
        AutoModeDenial, CtrlCSignal, CtrlCState, PromptCacheDiagnostics, SessionMode, SessionStats,
        is_follow_up_prompt_like, should_enforce_safe_mode_prompts,
    };
    use vtcode_core::config::WorkspaceTrustLevel;
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
    fn cycle_mode_rotates_edit_auto_plan() {
        let mut stats = SessionStats::default();

        assert_eq!(stats.current_mode(), SessionMode::Edit);
        assert_eq!(stats.cycle_mode(), SessionMode::Auto);
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
    fn safe_mode_prompts_are_disabled_for_auto_mode() {
        assert!(!should_enforce_safe_mode_prompts(
            false,
            true,
            Some(WorkspaceTrustLevel::ToolsPolicy),
        ));
    }

    #[test]
    fn auto_mode_denials_trigger_prompt_fallback_after_threshold() {
        let mut stats = SessionStats::default();
        stats.set_autonomous_mode(true);

        assert!(!stats.record_auto_mode_denial(
            AutoModeDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(!stats.auto_mode_prompt_fallback_active());

        assert!(!stats.record_auto_mode_denial(
            AutoModeDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(!stats.auto_mode_prompt_fallback_active());

        assert!(stats.record_auto_mode_denial(
            AutoModeDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(stats.auto_mode_prompt_fallback_active());
    }

    #[test]
    fn prompt_cache_fingerprint_reports_expected_change_reasons() {
        let mut stats = SessionStats::default();

        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)),
            "model"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)),
            "unchanged"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(22)),
            "stable_prefix"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(44)),
            "tool_catalog"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 55, Some(66)),
            "stable_prefix+tool_catalog"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5-mini", 55, Some(66)),
            "model"
        );

        assert_eq!(
            stats.prompt_cache_diagnostics(),
            PromptCacheDiagnostics {
                observations: 6,
                model_changes: 2,
                unchanged: 1,
                stable_prefix_changes: 1,
                tool_catalog_changes: 1,
                combined_changes: 1,
                last_change_reason: Some("model".to_string()),
                last_stable_prefix_hash: Some(55),
                last_tool_catalog_hash: Some(66),
            }
        );
    }

    #[test]
    fn previous_response_chain_clears_only_matching_scope() {
        let mut stats = SessionStats::default();
        let openai_messages = vec![vtcode_core::llm::provider::Message::user(
            "hello".to_string(),
        )];
        let gemini_messages = vec![vtcode_core::llm::provider::Message::user("hi".to_string())];
        stats.set_previous_response_chain(
            "openai",
            "gpt-5.4",
            Some("resp_openai"),
            &openai_messages,
        );
        stats.set_previous_response_chain(
            "gemini",
            "gemini-2.5-pro",
            Some("resp_gemini"),
            &gemini_messages,
        );

        stats.clear_previous_response_chain_for("openai", "gpt-5.4");

        assert_eq!(stats.previous_response_id_for("openai", "gpt-5.4"), None);
        assert_eq!(stats.previous_response_chain_for("openai", "gpt-5.4"), None);
        assert_eq!(
            stats.previous_response_id_for("gemini", "gemini-2.5-pro"),
            Some("resp_gemini".to_string())
        );
        assert_eq!(
            stats
                .previous_response_chain_for("gemini", "gemini-2.5-pro")
                .map(|chain| chain.messages.as_slice()),
            Some(gemini_messages.as_slice())
        );
    }

    #[test]
    fn safe_mode_prompts_follow_workspace_trust_for_edit_mode() {
        assert!(should_enforce_safe_mode_prompts(
            false,
            false,
            Some(WorkspaceTrustLevel::ToolsPolicy),
        ));
        assert!(!should_enforce_safe_mode_prompts(
            false,
            false,
            Some(WorkspaceTrustLevel::FullAuto),
        ));
        assert!(should_enforce_safe_mode_prompts(false, false, None));
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
