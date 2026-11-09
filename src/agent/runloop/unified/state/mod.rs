mod navigation_recovery;
mod preflight_recovery;
pub(crate) use navigation_recovery::{NAVIGATION_RECOVERY_BLOCK_REASON, NavigationRecovery};
pub(crate) use preflight_recovery::{PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX, PreflightRecovery, ValidationCategory};
mod ctrl_c;
pub(crate) use ctrl_c::{CtrlCSignal, CtrlCState};
mod follow_up;
#[cfg(test)]
pub(crate) use follow_up::FollowUpPromptAction;
pub(crate) use follow_up::is_follow_up_prompt_like;
mod prompt_cache;
pub(crate) use prompt_cache::PromptCacheDiagnostics;
mod verification;
pub(crate) use verification::{VerificationFailureSummary, missing_executable_hint};
mod request_envelope;
pub(crate) use request_envelope::RequestSegmentTransition;
use request_envelope::{RequestEnvelopeIdentity, ToolCatalogObservabilityIdentity};

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use vtcode_core::compaction::PrefireState;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::core::agent::request_envelope::SessionRequestEnvelope;
use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider::{
    Message, PromptCacheProfile, ResponsesContinuationState, ResponsesContinuationStore, ToolDefinition,
};
use vtcode_core::llm::request_gap::RequestGapTracker;
use vtcode_core::llm::usage_cost;

#[derive(Debug, Clone, Default)]
pub(crate) struct AutoPermissionDenial {
    pub stage: &'static str,
    pub reason: String,
    pub matched_rule: Option<String>,
    pub matched_exception: Option<String>,
}

#[derive(Default)]
pub(crate) struct SessionStats {
    /// One-shot proof that an explicit model selection completed successfully.
    accepted_model_selection: bool,
    pub(crate) preflight_recovery: PreflightRecovery,
    pub(crate) navigation_recovery: NavigationRecovery,
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

    /// Count of consecutive minimal follow-up prompts (e.g. "continue", "retry")
    follow_up_prompt_streak: usize,
    /// One-shot guard to avoid classifying injected recovery prompts as user follow-ups
    suppress_next_follow_up_prompt: bool,
    /// Whether the last turn ended in a stalled state (aborted/blocked)
    turn_stalled: bool,
    /// Reason associated with the last stalled turn, when available
    turn_stall_reason: Option<String>,
    /// Whether successful mutations still require a verification command.
    /// This survives a blocked turn so a later `continue` cannot claim
    /// completion from inspection-only work.
    verification_pending: bool,
    /// Whether the user granted a tool-loop increase at least once this
    /// session. After that first successful grant, later tool-loop limit hits
    /// auto-grant the maximum increment without another HITL prompt. Denials
    /// leave this false so the prompt remains available.
    tool_loop_grant_preauthorized: bool,
    /// Bounded autonomous turns the session loop may schedule after a
    /// verification-blocked turn before requiring manual `continue`.
    /// Reset only by genuine fresh user requests; incremented by
    /// [`Self::record_verification_auto_recovery_turn_with_limit`]. Prevents unbounded
    /// self-retry loops on a verifier that can never pass while still letting
    /// long-running autonomous work survive transient verification misses
    /// without user intervention.
    verification_auto_recovery_turns: u8,
    /// Bounded autonomous turns scheduled after a recoverable turn end while
    /// `task_tracker` still has incomplete steps. Separate from verification
    /// and plan-mode budgets. Reset on genuine user input, when tracker work
    /// clears, and when a tracker step completes (progress-reset) — not by
    /// `reset_verification_recovery_episode`.
    tracker_continuation_turns: u8,
    /// Highest completed checklist count in the current user-request episode.
    tracker_completed_count_high_water: u32,
    /// Cached incomplete tracker items used when the live probe fails so a
    /// transient tracker read error does not drop auto-queue.
    last_incomplete_tracker_items: Option<Vec<String>>,
    /// Bounded plan-mode auto-continue turns (recoverable blocked planning
    /// ends when no plan is approval-ready). Independent of tracker budget.
    plan_continuation_turns: u8,
    /// Consecutive failed harness auto-verifications this stall episode.
    /// Incremented by [`Self::record_verification_auto_failure`], reset by
    /// [`Self::record_verification_auto_success`] and every
    /// [`Self::reset_verification_recovery_episode`] site. At the configured
    /// threshold the harness stops executing verifiers itself and escalates
    /// to a manual handoff carrying [`Self::last_verification_failure`].
    verification_consecutive_failures: u8,
    /// Bounded tail of the last failed harness auto-verification, kept for
    /// handoff enrichment. `None` when the last auto-verification succeeded
    /// or none ran this episode.
    last_verification_failure: Option<VerificationFailureSummary>,
    /// Responses-style continuation state keyed by normalized provider/model pairs.
    previous_response_chains: ResponsesContinuationStore,
    prompt_cache_profile: Option<PromptCacheProfile>,
    prompt_cache_lineage_id: Option<String>,
    stream_timeout_advisory_emitted: bool,
    last_prompt_cache_model: Option<String>,
    last_stable_prefix_hash: Option<u64>,
    last_tool_catalog_hash: Option<u64>,
    last_wire_tool_count: Option<usize>,
    last_recovery_prompt_reason: Option<String>,
    last_prompt_cache_change_reason: Option<String>,
    prompt_cache_observations: usize,
    prompt_cache_model_changes: usize,
    prompt_cache_unchanged: usize,
    prompt_cache_stable_prefix_changes: usize,
    prompt_cache_tool_catalog_changes: usize,
    prompt_cache_combined_changes: usize,
    request_envelope: Option<SessionRequestEnvelope>,
    request_envelope_identity: Option<RequestEnvelopeIdentity>,
    request_envelope_source_tools: Option<Arc<Vec<ToolDefinition>>>,
    request_segment_sequence: u64,
    pending_request_segment_id: Option<String>,
    last_tool_catalog_observability: Option<ToolCatalogObservabilityIdentity>,
    recent_touched_files: VecDeque<String>,
    total_usage: HarnessUsage,
    decisions_probe_suggestion_shown: bool,
    fast_mode_suggestion_shown: bool,
    /// Rolling prompt-cache health fed by every recorded turn. Shared
    /// `vtcode-core` monitor so thresholds and wording match the headless
    /// runloop; fires at most two session-scoped hit-rate alerts.
    prompt_cache_health: vtcode_core::core::agent::cache_health::PromptCacheHealthMonitor,
    /// Cache-aware and conservative cost totals for the whole interactive
    /// session. This must live with the persistent session statistics rather
    /// than inside one `run_turn_loop` invocation, because each user turn
    /// re-enters that loop while budget enforcement remains session-scoped.
    cost_estimate: usage_cost::SessionCostAccumulator,
    total_cost_usd: Option<f64>,
    budget_warning_emitted: bool,
    stop_reason: Option<String>,
    budget_limit: Option<(f64, f64)>,
    total_turns: usize,
    /// Tracks the idle gap since the last dispatched LLM request, so a long
    /// enough pause can warn that the provider prompt cache has likely
    /// expired. Shared with the headless session state; see
    /// [`RequestGapTracker`].
    request_gap: RequestGapTracker,
    /// Prefire two-pass state: cached NOTE₁ for background pass-1.
    pub prefire: PrefireState,
    /// Auto-compaction suppression state: `SUPPRESS_NONE` allows compaction;
    /// other values gate automatic compaction until cleared by success, model
    /// switch, or explicit `/compact`.
    pub auto_compact_suppressed: u8,
    /// Composition of the session's first assembled LLM request. Captured once
    /// so the exit summary can surface the per-call harness tax (instructions
    /// + tool schemas) without re-reading the trajectory log.
    first_call_composition: Option<FirstCallComposition>,
}

/// First-request token composition (HarnessTax-style harness-tax breakdown).
///
/// `fixed_overhead_tokens` is the per-call harness tax paid before the task
/// prompt: system instructions plus on-wire tool schemas. On the session's
/// first request this is the paper's "initial harness context" excluding the
/// task itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FirstCallComposition {
    pub system_prompt_tokens: usize,
    pub tool_schema_tokens: usize,
    pub message_history_tokens: usize,
    pub on_wire_tools: usize,
}

impl FirstCallComposition {
    pub(crate) fn fixed_overhead_tokens(&self) -> usize {
        self.system_prompt_tokens.saturating_add(self.tool_schema_tokens)
    }
}

impl SessionStats {
    pub(crate) fn record_accepted_model_selection(&mut self) {
        self.accepted_model_selection = true;
    }

    pub(crate) fn take_accepted_model_selection(&mut self) -> bool {
        std::mem::take(&mut self.accepted_model_selection)
    }

    pub(crate) fn record_tool(&mut self, name: &str) {
        let normalized_name =
            vtcode_core::tools::tool_intent::canonical_command_session_tool_name(name).unwrap_or(name);
        self.tools.insert(normalized_name.to_string());
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.tools.contains(name)
    }

    pub(crate) fn sorted_tools(&self) -> Vec<String> {
        self.tools.iter().cloned().collect()
    }

    pub(crate) fn record_usage(&mut self, provider: &str, usage: Option<&vtcode_core::llm::provider::Usage>) {
        let Some(usage) = usage else {
            return;
        };
        self.total_usage.add(&usage_cost::normalized_turn_usage(provider, usage));
    }

    pub(crate) fn record_probe_attempt(
        &mut self,
        provider: &str,
        usage: Option<&vtcode_core::llm::provider::Usage>,
        cost: Option<usage_cost::SessionCostEstimate>,
    ) {
        // Auxiliary usage contributes to session totals, never cache health.
        self.record_usage(provider, usage);
        self.record_cost(cost);
    }

    /// Called only at idle boundaries; eligibility may change during a session.
    pub(crate) fn take_decisions_probe_suggestion(&mut self, applicable: bool) -> bool {
        if self.decisions_probe_suggestion_shown || !applicable {
            return false;
        }
        self.decisions_probe_suggestion_shown = true;
        true
    }

    pub(crate) fn take_fast_mode_suggestion(&mut self, applicable: bool) -> bool {
        if self.fast_mode_suggestion_shown || !applicable {
            return false;
        }
        self.fast_mode_suggestion_shown = true;
        true
    }

    pub(crate) fn dismiss_fast_mode_suggestion(&mut self) {
        self.fast_mode_suggestion_shown = true;
    }

    /// Record one turn's usage for prompt-cache health and return a
    /// session-scoped hit-rate alert message the first time a degraded
    /// pattern is confirmed. Returns `None` on healthy or unmeasured turns.
    pub(crate) fn record_cache_turn_health(
        &mut self,
        provider: &str,
        usage: Option<&vtcode_core::llm::provider::Usage>,
    ) -> Option<String> {
        let usage = usage?;
        let normalized = usage_cost::normalized_turn_usage(provider, usage);
        self.prompt_cache_health.record_turn(&normalized).map(|alert| alert.message())
    }

    pub(crate) fn total_usage(&self) -> HarnessUsage {
        self.total_usage.clone()
    }

    /// Capture the session's first assembled request composition. Returns
    /// `true` when this call stored the value (first capture wins; later
    /// builds leave the original composition untouched).
    pub(crate) fn record_first_call_composition(&mut self, composition: FirstCallComposition) -> bool {
        if self.first_call_composition.is_some() {
            return false;
        }
        self.first_call_composition = Some(composition);
        true
    }

    /// The captured first-call composition, when a request has been assembled.
    pub(crate) fn first_call_composition(&self) -> Option<FirstCallComposition> {
        self.first_call_composition
    }

    /// Add one turn's cost to the session total and update the display value.
    /// An unpriced turn makes the complete session total unknown and keeps it
    /// unknown for subsequent turns, so callers cannot accidentally present a
    /// partial total as the session spend.
    pub(crate) fn record_cost(
        &mut self,
        estimate: Option<usage_cost::SessionCostEstimate>,
    ) -> Option<usage_cost::SessionCostEstimate> {
        let total = self.cost_estimate.record(estimate);
        self.total_cost_usd = total.map(|cost| cost.effective_usd);
        total
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

    pub(crate) fn reset_for_fresh_execution(&mut self) {
        // Preserve aggregate usage/cost and the configured cache profile, but
        // discard every lineage, fingerprint, request-envelope, and response
        // diagnostic that belongs to the cleared conversational context.
        self.tools.clear();
        self.preflight_recovery = PreflightRecovery::default();
        self.reset_auto_permission_review_state();
        self.follow_up_prompt_streak = 0;
        self.suppress_next_follow_up_prompt = false;
        self.turn_stalled = false;
        self.turn_stall_reason = None;
        self.reset_verification_recovery_episode();
        self.clear_previous_response_chain();
        self.prompt_cache_lineage_id = None;
        self.last_prompt_cache_model = None;
        self.last_stable_prefix_hash = None;
        self.last_tool_catalog_hash = None;
        self.last_wire_tool_count = None;
        self.last_recovery_prompt_reason = None;
        self.last_prompt_cache_change_reason = None;
        self.prompt_cache_observations = 0;
        self.prompt_cache_model_changes = 0;
        self.prompt_cache_unchanged = 0;
        self.prompt_cache_stable_prefix_changes = 0;
        self.prompt_cache_tool_catalog_changes = 0;
        self.prompt_cache_combined_changes = 0;
        self.request_envelope = None;
        self.request_envelope_identity = None;
        self.request_envelope_source_tools = None;
        self.pending_request_segment_id = None;
        self.last_tool_catalog_observability = None;
        self.recent_touched_files.clear();
        self.stop_reason = None;
        self.request_gap = RequestGapTracker::default();
        self.prefire = PrefireState::default();
        self.auto_compact_suppressed = vtcode_core::compaction::SUPPRESS_NONE;
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
        self.previous_response_chains.get(provider, model)
    }

    #[allow(
        dead_code,
        reason = "retained as the allocation-owning compatibility setter for tests and callers"
    )]
    pub(crate) fn set_previous_response_chain(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: &[Message],
    ) {
        self.set_previous_response_chain_shared(provider, model, response_id, Arc::new(messages.to_vec()));
    }

    pub(crate) fn set_previous_response_chain_shared(
        &mut self,
        provider: &str,
        model: &str,
        response_id: Option<&str>,
        messages: Arc<Vec<Message>>,
    ) {
        self.previous_response_chains.set(provider, model, response_id, messages);
    }

    pub(crate) fn clear_previous_response_chain_for(&mut self, provider: &str, model: &str) {
        self.previous_response_chains.clear_for(provider, model);
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

            if let Some(existing) = self.recent_touched_files.iter().position(|entry| entry == normalized) {
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

    /// Whether later tool-loop limit hits may skip the HITL prompt and
    /// auto-grant the maximum increment. Set only after the first successful
    /// interactive grant in this process session.
    pub(crate) fn tool_loop_grant_preauthorized(&self) -> bool {
        self.tool_loop_grant_preauthorized
    }

    /// Latch session-preauthorized tool-loop auto-grants after a successful
    /// interactive grant. Idempotent; denials never call this.
    pub(crate) fn mark_tool_loop_grant_preauthorized(&mut self) {
        self.tool_loop_grant_preauthorized = true;
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
        self.auto_permission_consecutive_denials = self.auto_permission_consecutive_denials.saturating_add(1);
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

#[cfg(test)]
mod tests;
