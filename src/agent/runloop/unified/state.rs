use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider::{
    Message, PromptCacheProfile, ResponsesContinuationState, responses_continuation_key,
};
use vtcode_core::llm::request_gap::RequestGapTracker;
use vtcode_core::llm::usage_cost;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ModelPickerTarget {
    #[default]
    Main,
    Lightweight,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct AutoPermissionDenial {
    pub stage: &'static str,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub matched_exception: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FollowUpPromptAction {
    None,
    ForceConclusion,
    RecoverFromStall { stall_reason: Option<String> },
}

impl FollowUpPromptAction {
    pub(crate) const fn should_force_autonomous_response(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub(crate) const fn is_stalled_recovery(&self) -> bool {
        matches!(self, Self::RecoverFromStall { .. })
    }

    pub(crate) fn stall_reason(&self) -> Option<&str> {
        match self {
            Self::RecoverFromStall { stall_reason } => stall_reason.as_deref(),
            Self::None | Self::ForceConclusion => None,
        }
    }
}

const FOLLOW_UP_STALLED_THRESHOLD: usize = 1;
const FOLLOW_UP_DEFAULT_THRESHOLD: usize = 3;

#[derive(Default)]
pub(crate) struct SessionStats {
    tools: std::collections::BTreeSet<String>,
    pub task_panel_visible: bool,
    /// Auto permission classifier consecutive denial count.
    auto_permission_consecutive_denials: u32,
    /// Auto permission classifier total denial count.
    auto_permission_total_denials: u32,
    /// Auto permission review has fallen back to manual prompts for the rest of the session.
    auto_permission_prompt_fallback: bool,
    /// Most recent auto permission classifier denial.
    last_auto_permission_denial: Option<AutoPermissionDenial>,
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
    prompt_cache_profile: Option<PromptCacheProfile>,
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
    budget_warning_emitted: bool,
    stop_reason: Option<String>,
    budget_limit: Option<(f64, f64)>,
    total_turns: usize,
    /// Tracks the idle gap since the last dispatched LLM request, so a long
    /// enough pause can warn that the provider prompt cache has likely
    /// expired. Shared with the headless session state; see
    /// [`RequestGapTracker`].
    request_gap: RequestGapTracker,
}

impl SessionStats {
    pub(crate) fn record_tool(&mut self, name: &str) {
        let normalized_name =
            vtcode_core::tools::tool_intent::canonical_command_session_tool_name(name)
                .unwrap_or(name);
        self.tools.insert(normalized_name.to_string());
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    pub(crate) fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }

    pub(crate) fn record_usage(
        &mut self,
        provider: &str,
        usage: &Option<vtcode_core::llm::provider::Usage>,
    ) {
        let Some(usage) = usage else {
            return;
        };
        self.total_usage.add(&usage_cost::normalized_turn_usage(provider, usage));
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

    pub(crate) fn budget_warning_emitted(&self) -> bool {
        self.budget_warning_emitted
    }

    pub(crate) fn mark_budget_warning_emitted(&mut self) {
        self.budget_warning_emitted = true;
    }

    /// Records that an LLM request was just dispatched, so the next call to
    /// [`Self::cache_gap_exceeds`] can measure the idle gap since this request.
    pub(crate) fn note_request_sent(&mut self) {
        self.request_gap.note_request_sent();
    }

    /// Returns whether an LLM request has been dispatched at any point this
    /// session. More precise than inferring from accumulated token usage
    /// (which can be zero even after a request, e.g. an error response).
    pub(crate) fn has_sent_request(&self) -> bool {
        self.request_gap.has_sent_request()
    }

    /// Returns the elapsed time since the last dispatched request when it
    /// exceeds `threshold`, or `None` if there was no prior request or the gap
    /// is still within the threshold. Used to warn that the provider prompt
    /// cache has likely expired before the next request re-pays full input
    /// cost.
    pub(crate) fn cache_gap_exceeds(&self, threshold: Duration) -> Option<Duration> {
        self.request_gap.cache_gap_exceeds(threshold)
    }

    pub(crate) fn mark_budget_limit_reached(&mut self, max_budget_usd: f64, actual_cost_usd: f64) {
        self.budget_limit = Some((max_budget_usd, actual_cost_usd));
    }

    pub(crate) fn budget_limit(&self) -> Option<(f64, f64)> {
        self.budget_limit
    }

    pub(crate) fn set_prompt_cache_profile(&mut self, profile: Option<PromptCacheProfile>) {
        self.prompt_cache_profile = profile;
    }

    pub(crate) fn prompt_cache_profile(&self) -> Option<PromptCacheProfile> {
        self.prompt_cache_profile
    }

    pub(crate) fn record_turn_completed(&mut self) {
        self.total_turns = self.total_turns.saturating_add(1);
    }

    pub(crate) fn total_turns(&self) -> usize {
        self.total_turns
    }

    pub(crate) fn reset_for_planning_workflow_entry(&mut self) {
        self.reset_auto_permission_review_state();
        self.tools.clear();
        self.clear_previous_response_chain();
    }

    pub(crate) fn register_follow_up_prompt(&mut self, input: &str) -> FollowUpPromptAction {
        let suppression_active = self.consume_follow_up_prompt_suppression();
        let is_follow_up = is_follow_up_prompt_like(input);

        if is_follow_up {
            if suppression_active {
                return FollowUpPromptAction::None;
            }
            self.follow_up_prompt_streak = self.follow_up_prompt_streak.saturating_add(1);
        } else {
            self.follow_up_prompt_streak = 0;
            self.turn_stalled = false;
            self.turn_stall_reason = None;
            return FollowUpPromptAction::None;
        }

        let threshold = if self.turn_stalled {
            FOLLOW_UP_STALLED_THRESHOLD
        } else {
            FOLLOW_UP_DEFAULT_THRESHOLD
        };
        if self.follow_up_prompt_streak < threshold {
            return FollowUpPromptAction::None;
        }

        if self.turn_stalled {
            FollowUpPromptAction::RecoverFromStall { stall_reason: self.turn_stall_reason.clone() }
        } else {
            FollowUpPromptAction::ForceConclusion
        }
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

    #[cfg(test)]
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
        *self.counter_for_reason(reason) += 1;

        self.last_prompt_cache_model = Some(model.to_string());
        self.last_stable_prefix_hash = Some(stable_prefix_hash);
        self.last_tool_catalog_hash = tool_catalog_hash;
        self.last_prompt_cache_change_reason = Some(reason.to_string());

        reason
    }

    fn counter_for_reason(&mut self, reason: &str) -> &mut usize {
        match reason {
            "model" => &mut self.prompt_cache_model_changes,
            "unchanged" => &mut self.prompt_cache_unchanged,
            "stable_prefix" => &mut self.prompt_cache_stable_prefix_changes,
            "tool_catalog" => &mut self.prompt_cache_tool_catalog_changes,
            "stable_prefix+tool_catalog" => &mut self.prompt_cache_combined_changes,
            _ => &mut self.prompt_cache_unchanged,
        }
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
                messages: Arc::new(messages.to_vec()),
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

            if let Some(existing) =
                self.recent_touched_files.iter().position(|entry| entry == normalized)
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

    pub(crate) fn auto_permission_prompt_fallback_active(&self) -> bool {
        self.auto_permission_prompt_fallback
    }

    pub(crate) fn last_auto_permission_denial(&self) -> Option<&AutoPermissionDenial> {
        self.last_auto_permission_denial.as_ref()
    }

    pub(crate) fn reset_auto_permission_review_state(&mut self) {
        self.auto_permission_consecutive_denials = 0;
        self.auto_permission_total_denials = 0;
        self.auto_permission_prompt_fallback = false;
        self.last_auto_permission_denial = None;
    }

    pub(crate) fn record_auto_permission_allow(&mut self) {
        self.auto_permission_consecutive_denials = 0;
        self.last_auto_permission_denial = None;
    }

    pub(crate) fn record_auto_permission_denial(
        &mut self,
        denial: AutoPermissionDenial,
        max_consecutive_denials: u32,
        max_total_denials: u32,
    ) -> bool {
        self.auto_permission_consecutive_denials =
            self.auto_permission_consecutive_denials.saturating_add(1);
        self.auto_permission_total_denials = self.auto_permission_total_denials.saturating_add(1);
        self.last_auto_permission_denial = Some(denial);
        self.auto_permission_prompt_fallback = self.auto_permission_consecutive_denials
            >= max_consecutive_denials.max(1)
            || self.auto_permission_total_denials >= max_total_denials.max(1);
        self.auto_permission_prompt_fallback
    }
}

pub(crate) fn should_enforce_safe_mode_prompts(
    full_auto: bool,
    auto_permission_review_active: bool,
    workspace_trust_level: Option<WorkspaceTrustLevel>,
) -> bool {
    if full_auto || auto_permission_review_active {
        tracing::warn!(
            full_auto,
            auto_permission_review_active,
            "Safe-mode prompts bypassed: auto mode or permission review is active"
        );
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

/// State machine for handling Ctrl+C signals with priority guarantees.
///
/// # Priority Guarantees
///
/// This state machine ensures that Ctrl+C (SIGINT) is always the highest priority
/// and cannot be blocked by any other process. The following guarantees are enforced:
///
/// 1. **First Ctrl+C**: Immediately cancels the current operation (Cancel signal)
/// 2. **Second Ctrl+C (within 1 second)**: Escalates to exit; a 200ms debounce
///    sub-window suppresses accidental double-taps so a rapid second tap returns
///    `Cancel` (debounced) rather than `Exit` (see State Machine below)
/// 3. **Emergency exit path**: On double Ctrl+C, the program calls `std::process::exit(130)`
///    which bypasses all other operations and cleanup routines
/// 4. **No signal masking**: SIGINT is never blocked or masked anywhere in the codebase
/// 5. **Atomic operations**: All state transitions use lock-free atomic operations,
///    ensuring no mutex contention can block signal handling
///
/// # Exit Command Priority
///
/// The `/exit`, `/quit`, `exit`, and `quit` commands are processed immediately
/// and cannot be blocked by any other operation. They return
/// `InteractionOutcome::Exit { reason: SessionEndReason::Exit }` which is checked
/// at the top of every interaction loop iteration.
///
/// # State Machine
///
/// The state machine transitions through four phases:
/// - `Idle` → First Ctrl+C → `CancelRequested` (returns `CtrlCSignal::Cancel`)
/// - `CancelRequested` → Second Ctrl+C within 200ms → `CancelRequested` (returns `CtrlCSignal::Cancel`, debounced)
/// - `CancelRequested` → Second Ctrl+C between 200ms and 1s → `ExitRequested` (returns `CtrlCSignal::Exit`)
/// - `CancelRequested` → Second Ctrl+C after 1s → `CancelRequested` (returns `CtrlCSignal::Cancel`, window expired)
/// - `ExitArmed` → Second Ctrl+C within 1s → `ExitRequested` (returns `CtrlCSignal::Exit`)
/// - `ExitRequested` → Any subsequent Ctrl+C → `ExitRequested` (returns `CtrlCSignal::Exit`)
///
/// # Debounce Mechanism
///
/// A 200ms debounce window prevents rapid repeated signals from prematurely
/// escalating the state machine. This ensures that accidental double-taps
/// don't immediately exit the program. Only after 200ms has elapsed since
/// the first signal will a second signal trigger escalation to exit.
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

    /// Register a Ctrl+C signal and return the appropriate signal type.
    ///
    /// # Priority Guarantee
    ///
    /// This method is called from the signal handler and is guaranteed to:
    /// 1. Always process the signal immediately (no blocking)
    /// 2. Never be blocked by any other operation
    /// 3. Return `CtrlCSignal::Exit` on double Ctrl+C, which triggers
    ///    `emergency_terminal_cleanup()` → `std::process::exit(130)`
    ///
    /// # Debounce Behavior
    ///
    /// Rapid repeated signals (within 200ms) are debounced to prevent
    /// accidental state escalation. However, if already in `ExitArmed` or
    /// `ExitRequested` phase, rapid signals immediately escalate to exit.
    ///
    /// # Window Behavior
    ///
    /// The second Ctrl+C must arrive within 1 second of the first to trigger
    /// exit. After this window, the state machine resets to `CancelRequested`
    /// on the next signal.
    pub(crate) fn register_signal(&self) -> CtrlCSignal {
        let now =
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        let last = self.last_signal_time.swap(now, Ordering::SeqCst);
        let current_phase = self.phase();

        // Debounce repeated cancel signals, but allow an already-armed stop to
        // escalate immediately so a quick second press can still exit.
        if last > 0 && now.saturating_sub(last) < 200 {
            if matches!(current_phase, CtrlCPhase::ExitArmed | CtrlCPhase::ExitRequested) {
                self.set_phase(CtrlCPhase::ExitRequested);
                return CtrlCSignal::Exit;
            }
            return current_phase.signal();
        }

        let window_ms = DOUBLE_CTRL_C_WINDOW.as_millis() as u64;
        let is_within_window = last > 0 && now.saturating_sub(last) <= window_ms;

        if matches!(current_phase, CtrlCPhase::CancelRequested | CtrlCPhase::ExitArmed)
            && is_within_window
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
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::{
        AutoPermissionDenial, CtrlCSignal, CtrlCState, FollowUpPromptAction,
        PromptCacheDiagnostics, SessionStats, is_follow_up_prompt_like,
        should_enforce_safe_mode_prompts,
    };
    use vtcode_core::config::WorkspaceTrustLevel;
    use vtcode_core::config::constants::tools;

    #[test]
    fn record_tool_normalizes_exec_aliases() {
        let mut stats = SessionStats::default();
        stats.record_tool(tools::UNIFIED_EXEC);
        stats.record_tool("shell");
        stats.record_tool("exec_pty_cmd");
        stats.record_tool(tools::EXEC_COMMAND);

        assert_eq!(stats.sorted_tools(), vec![tools::UNIFIED_EXEC.to_string()]);
    }

    /// Thin delegation check: `SessionStats` forwards to the embedded
    /// `RequestGapTracker` correctly. Exhaustive edge-case coverage
    /// (no-prior-request, below-threshold, above-threshold) lives on
    /// `RequestGapTracker`'s own unit tests in
    /// `vtcode_core::llm::request_gap`.
    #[test]
    fn cache_gap_exceeds_delegates_to_request_gap_tracker() {
        let mut stats = SessionStats::default();
        assert!(!stats.has_sent_request());
        assert_eq!(stats.cache_gap_exceeds(Duration::from_millis(1)), None);

        stats.note_request_sent();
        assert!(stats.has_sent_request());
        thread::sleep(Duration::from_millis(15));
        let gap = stats.cache_gap_exceeds(Duration::from_millis(5));
        assert!(gap.is_some_and(|elapsed| elapsed >= Duration::from_millis(15)));
    }

    #[test]
    fn follow_up_prompts_force_conclusion_after_stall() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));

        let action = stats.register_follow_up_prompt("continue");
        assert_eq!(
            action,
            FollowUpPromptAction::RecoverFromStall {
                stall_reason: Some("turn blocked".to_string()),
            }
        );
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

        assert_eq!(
            stats.register_follow_up_prompt("run tests and summarize"),
            FollowUpPromptAction::None
        );
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }

    #[test]
    fn follow_up_prompt_variants_are_detected() {
        let mut stats = SessionStats::default();
        assert_eq!(stats.register_follow_up_prompt("continue."), FollowUpPromptAction::None);
        assert_eq!(
            stats.register_follow_up_prompt("continue with your recommendation"),
            FollowUpPromptAction::None
        );
        assert_eq!(
            stats.register_follow_up_prompt("please continue"),
            FollowUpPromptAction::ForceConclusion
        );
    }

    #[test]
    fn suppressed_follow_up_prompt_is_ignored_once() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
        stats.suppress_next_follow_up_prompt();

        assert_eq!(stats.register_follow_up_prompt("continue"), FollowUpPromptAction::None);
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));

        assert!(stats.register_follow_up_prompt("continue").is_stalled_recovery());
    }

    #[test]
    fn suppressed_non_follow_up_still_clears_stall_state() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn blocked".to_string()));
        stats.suppress_next_follow_up_prompt();

        assert_eq!(
            stats.register_follow_up_prompt("run tests and summarize"),
            FollowUpPromptAction::None
        );
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }

    #[test]
    fn follow_up_prompt_action_exposes_stall_reason() {
        let action =
            FollowUpPromptAction::RecoverFromStall { stall_reason: Some("blocked".to_string()) };

        assert!(action.should_force_autonomous_response());
        assert!(action.is_stalled_recovery());
        assert_eq!(action.stall_reason(), Some("blocked"));
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
    fn safe_mode_prompts_are_disabled_for_auto_permission() {
        assert!(!should_enforce_safe_mode_prompts(
            false,
            true,
            Some(WorkspaceTrustLevel::ToolsPolicy),
        ));
    }

    #[test]
    fn auto_permission_denials_trigger_prompt_fallback_after_threshold() {
        let mut stats = SessionStats::default();

        assert!(!stats.record_auto_permission_denial(
            AutoPermissionDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(!stats.auto_permission_prompt_fallback_active());

        assert!(!stats.record_auto_permission_denial(
            AutoPermissionDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(!stats.auto_permission_prompt_fallback_active());

        assert!(stats.record_auto_permission_denial(
            AutoPermissionDenial {
                stage: "stage2",
                reason: "blocked".to_string(),
                matched_rule: Some("rule".to_string()),
                matched_exception: None,
            },
            3,
            20,
        ));
        assert!(stats.auto_permission_prompt_fallback_active());
    }

    #[test]
    fn prompt_cache_fingerprint_reports_expected_change_reasons() {
        let mut stats = SessionStats::default();

        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)), "model");
        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22)), "unchanged");
        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(22)), "stable_prefix");
        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 33, Some(44)), "tool_catalog");
        assert_eq!(
            stats.record_prompt_cache_fingerprint("gpt-5", 55, Some(66)),
            "stable_prefix+tool_catalog"
        );
        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5-mini", 55, Some(66)), "model");

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

    #[test]
    fn ctrl_c_state_escalation_is_priority_guarantee() {
        // This test verifies the priority guarantee: double Ctrl+C always exits
        // Note: The 200ms debounce window prevents immediate escalation
        let state = CtrlCState::new();

        // First Ctrl+C - should always cancel
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        assert!(state.is_cancel_requested());
        assert!(!state.is_exit_requested());

        // Second Ctrl+C within 1 second (but after 200ms debounce) - should exit
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(!state.is_cancel_requested());
        assert!(state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_debounce_prevents_accidental_escalation() {
        let state = CtrlCState::new();

        // First Ctrl+C
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

        // Rapid second Ctrl+C (within 200ms debounce window)
        // Should still cancel, not exit
        thread::sleep(Duration::from_millis(50));
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        assert!(state.is_cancel_requested());
        assert!(!state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_exit_is_always_processed() {
        // This test verifies that exit signals are always processed
        let state = CtrlCState::new();

        // Get to exit state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));

        // Subsequent Ctrl+C should always return Exit
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_check_cancellation_returns_error_on_exit() {
        let state = CtrlCState::new();

        // Get to exit state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));

        // check_cancellation should return error
        assert!(state.check_cancellation().is_err());
    }

    #[test]
    fn ctrl_c_state_check_cancellation_returns_error_on_cancel() {
        let state = CtrlCState::new();

        // Get to cancel state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

        // check_cancellation should return error
        assert!(state.check_cancellation().is_err());
    }

    #[test]
    fn ctrl_c_state_check_cancellation_ok_when_idle() {
        let state = CtrlCState::new();

        // check_cancellation should return Ok when idle
        assert!(state.check_cancellation().is_ok());
    }

    #[test]
    fn ctrl_c_state_mark_cancel_handled_transitions_to_exit_armed() {
        let state = CtrlCState::new();

        // Get to cancel state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        assert!(state.is_cancel_requested());

        // Mark cancel handled should transition to ExitArmed
        state.mark_cancel_handled();

        // Should not be cancel requested anymore
        assert!(!state.is_cancel_requested());

        // Should not be exit requested yet
        assert!(!state.is_exit_requested());

        // Next Ctrl+C should exit
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_window_expires_after_one_second() {
        let state = CtrlCState::new();

        // First Ctrl+C
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));

        // Wait for window to expire (1.1 seconds)
        thread::sleep(Duration::from_millis(1100));

        // Second Ctrl+C should cancel again, not exit
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        assert!(state.is_cancel_requested());
        assert!(!state.is_exit_requested());
    }

    #[test]
    fn ctrl_c_state_reset_clears_all_state() {
        let state = CtrlCState::new();

        // Get to exit state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());

        // Reset should clear everything
        state.reset();

        // Should be back to idle
        assert!(!state.is_cancel_requested());
        assert!(!state.is_exit_requested());
        assert!(state.check_cancellation().is_ok());
    }

    #[test]
    fn ctrl_c_state_atomic_operations_are_thread_safe() {
        // This test verifies that atomic operations work correctly under concurrency
        let state = Arc::new(CtrlCState::new());
        let mut handles = vec![];

        // Spawn multiple threads that all try to register signals
        for _ in 0..10 {
            let state = Arc::clone(&state);
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    let _ = state.register_signal();
                }
            }));
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // The state should be consistent (either cancel or exit requested)
        // The exact state depends on timing, but it should be valid
        assert!(state.is_cancel_requested() || state.is_exit_requested());
    }
}
