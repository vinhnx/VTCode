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
pub(crate) use verification::VerificationFailureSummary;
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
    /// Session-lifetime latch for the MCP catalogue readiness line. Wiring
    /// state (`mcp_catalog_initialized`) resets on agent switches and manual
    /// `/mcp` refreshes, so the notice needs its own latch to stay once-per-session.
    mcp_ready_notice_shown: bool,
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

    pub(crate) fn record_usage(&mut self, provider: &str, usage: &Option<vtcode_core::llm::provider::Usage>) {
        let Some(usage) = usage else {
            return;
        };
        self.total_usage.add(&usage_cost::normalized_turn_usage(provider, usage));
    }

    pub(crate) fn record_probe_attempt(
        &mut self,
        provider: &str,
        usage: &Option<vtcode_core::llm::provider::Usage>,
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

    /// Claim the one-per-session MCP readiness line. Returns `true` the first
    /// time the catalogue is announceable, then latches so agent switches,
    /// plan handoffs, and manual `/mcp` refreshes stay silent.
    #[must_use]
    pub(crate) fn take_mcp_ready_notice(&mut self) -> bool {
        if self.mcp_ready_notice_shown {
            return false;
        }
        self.mcp_ready_notice_shown = true;
        true
    }

    /// Suppress the MCP readiness line without rendering it, for sessions that
    /// attach the catalogue before the interaction loop starts.
    pub(crate) fn mark_mcp_ready_notice_shown(&mut self) {
        self.mcp_ready_notice_shown = true;
    }

    /// Record one turn's usage for prompt-cache health and return a
    /// session-scoped hit-rate alert message the first time a degraded
    /// pattern is confirmed. Returns `None` on healthy or unmeasured turns.
    pub(crate) fn record_cache_turn_health(
        &mut self,
        provider: &str,
        usage: &Option<vtcode_core::llm::provider::Usage>,
    ) -> Option<String> {
        let usage = usage.as_ref()?;
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
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::{
        AutoPermissionDenial, CtrlCSignal, CtrlCState, FollowUpPromptAction, PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX,
        PromptCacheDiagnostics, SessionStats, ValidationCategory, is_follow_up_prompt_like,
        should_enforce_safe_mode_prompts,
    };
    use vtcode_core::config::WorkspaceTrustLevel;
    use vtcode_core::config::constants::tools;
    use vtcode_core::core::agent::request_envelope::SegmentBoundaryReason;
    use vtcode_core::llm::provider::ToolDefinition;

    fn function_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({"type": "object"}))
    }

    #[test]
    fn mcp_ready_notice_is_claimed_exactly_once_per_session() {
        let mut stats = SessionStats::default();
        assert!(stats.take_mcp_ready_notice(), "first catalogue wiring must render");
        assert!(
            !stats.take_mcp_ready_notice(),
            "agent switches and manual /mcp refreshes must not repeat the notice"
        );
    }

    #[test]
    fn mcp_ready_notice_presuppression_does_not_render_later() {
        let mut stats = SessionStats::default();
        stats.mark_mcp_ready_notice_shown();
        assert!(!stats.take_mcp_ready_notice(), "a catalogue attached before the interaction loop must stay silent");
    }

    #[test]
    fn compaction_parent_context_reuses_arcs_only_for_matching_identity() {
        let mut stats = SessionStats::default();
        assert!(stats.compaction_parent_context("openai", "model-a").is_none());
        let envelope = stats.request_envelope(
            "model-a",
            "openai",
            "build",
            "fixed instructions".into(),
            vec![function_tool("zeta"), function_tool("alpha")],
            7,
        );
        assert!(stats.compaction_parent_context("anthropic", "model-a").is_none());
        assert!(stats.compaction_parent_context("openai", "model-b").is_none());
        let parent = stats.compaction_parent_context("openai", "model-a").unwrap();
        assert!(Arc::ptr_eq(parent.system_prompt.as_ref().unwrap(), &envelope.system_prompt()));
        assert!(Arc::ptr_eq(parent.tools.as_ref().unwrap(), &envelope.ordered_tools()));
        stats.begin_request_segment(SegmentBoundaryReason::Compaction);
        assert!(stats.compaction_parent_context("openai", "model-a").is_none());
        stats.request_envelope("model-b", "openai", "build", "new instructions".into(), vec![], 7);
        let parent = stats.compaction_parent_context("openai", "model-b").unwrap();
        assert_eq!(parent.system_prompt.as_deref(), Some("new instructions"));
        assert!(parent.tools.is_none());
    }

    #[test]
    fn preflight_internal_followups_preserve_episode_and_user_input_resets_it() {
        let mut stats = SessionStats::default();
        stats.record_tracker_continuation_turn_with_limit(3);
        stats.preflight_recovery.reject(
            "task_tracker",
            ValidationCategory::Arguments,
            Some(&serde_json::json!({"action":"list","index":1})),
            6,
            false,
        );
        stats.preflight_recovery.synthesis();
        stats.preflight_recovery.begin_corrective_turn();
        stats.register_follow_up_prompt(&format!("{PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX} repair arguments"));
        stats.preflight_recovery.finish_corrective_turn();
        assert!(stats.preflight_recovery.exhausted());
        stats.register_follow_up_prompt("The task tracker still has incomplete steps: verify");
        assert!(stats.preflight_recovery.exhausted());
        assert_eq!(stats.tracker_continuation_turns(), 1);
        stats.register_follow_up_prompt("continue");
        assert!(!stats.preflight_recovery.exhausted());
        assert!(stats.preflight_recovery.is_clear());
        assert_eq!(
            stats.tracker_continuation_turns(),
            1,
            "user resume retains existing tracker no-progress accounting"
        );
    }

    #[test]
    fn decisions_probe_accounting_preserves_unknown_cost_and_does_not_feed_cache_health() {
        let mut stats = SessionStats::default();
        let usage = Some(vtcode_core::llm::provider::Usage {
            prompt_tokens: 2048,
            completion_tokens: 0,
            total_tokens: 2048,
            cached_prompt_tokens: Some(1),
            cache_read_tokens: Some(1),
            cache_creation_tokens: None,
            iterations: None,
        });
        for _ in 0..3 {
            stats.record_probe_attempt(
                "openai",
                &usage,
                usage.as_ref().map(vtcode_core::llm::usage_cost::estimate_decisions_cost),
            );
        }
        assert_eq!(stats.total_usage().input_tokens, 6144);
        assert!((stats.total_cost_usd().unwrap() - 0.0006144).abs() < 1e-12);
        assert!(stats.record_cache_turn_health("openai", &usage).is_none());
        assert!(stats.record_cache_turn_health("openai", &usage).is_none());
        assert!(stats.record_cache_turn_health("openai", &usage).is_some());
        stats.record_probe_attempt("openai", &None, None);
        assert!(stats.total_cost_usd().is_none());
        stats.record_probe_attempt(
            "openai",
            &usage,
            usage.as_ref().map(vtcode_core::llm::usage_cost::estimate_decisions_cost),
        );
        assert!(stats.total_cost_usd().is_none());
    }

    #[test]
    fn compaction_boundary_does_not_join_cache_misses_across_replay_windows() {
        let mut stats = SessionStats::default();
        let usage = |input, cached, creation| {
            Some(vtcode_core::llm::provider::Usage {
                prompt_tokens: input,
                cached_prompt_tokens: Some(cached),
                cache_read_tokens: Some(cached),
                cache_creation_tokens: Some(creation),
                ..Default::default()
            })
        };
        for sample in [usage(12_645, 0, 12_642), usage(12_669, 5_710, 5_575)] {
            stats.record_usage("openai", &sample);
            assert!(stats.record_cache_turn_health("openai", &sample).is_none());
        }
        stats.begin_request_segment(SegmentBoundaryReason::Compaction);
        let sample = usage(12_740, 0, 5_212);
        stats.record_usage("openai", &sample);
        assert!(stats.record_cache_turn_health("openai", &sample).is_none());
        assert_eq!(stats.total_usage().input_tokens, 38_054);
        assert_eq!(stats.total_usage().cached_input_tokens, 5_710);
        assert!(stats.record_cache_turn_health("openai", &sample).is_none());
        assert!(stats.record_cache_turn_health("openai", &sample).is_some());
    }

    #[test]
    fn capability_change_starts_segment_without_mutating_frozen_prompt() {
        let mut stats = SessionStats::default();
        let first = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 11);
        let same = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 11);
        assert_eq!(first.segment_id(), same.segment_id());
        let changed = stats.request_envelope_shared("model", "provider", "build", "fixed".into(), None, 7, 12);
        assert_ne!(first.segment_id(), changed.segment_id());
        assert_ne!(first.prefix_hash(), changed.prefix_hash());
        assert_eq!(first.system_prompt(), changed.system_prompt());
    }

    #[test]
    fn runtime_suffix_refreshes_without_invalidating_stable_segment() {
        let mut stats = SessionStats::default();
        let first = stats.request_envelope_shared(
            "model",
            "provider",
            "build",
            "fixed\n[Runtime Context]\n- turn: 1".into(),
            None,
            7,
            11,
        );
        let second = stats.request_envelope_shared(
            "model",
            "provider",
            "build",
            "fixed\n[Runtime Context]\n- turn: 2".into(),
            None,
            7,
            11,
        );

        assert_eq!(first.segment_id(), second.segment_id());
        assert_eq!(first.prefix_hash(), second.prefix_hash());
        assert_ne!(first.system_prompt(), second.system_prompt());
    }

    #[test]
    fn equivalent_requests_reuse_the_same_segment_bytes() {
        let mut stats = SessionStats::default();
        let first = stats.request_envelope(
            "model",
            "provider",
            "build",
            "fixed".to_string(),
            vec![function_tool("zeta"), function_tool("exec_command")],
            7,
        );
        let second = stats.request_envelope(
            "model",
            "provider",
            "build",
            "fixed".to_string(),
            vec![function_tool("exec_command"), function_tool("zeta")],
            7,
        );

        assert_eq!(first.segment_id(), second.segment_id());
        assert_eq!(first.system_prompt().as_bytes(), second.system_prompt().as_bytes());
        assert_eq!(first.catalog_hash(), second.catalog_hash());
    }

    #[test]
    fn shared_request_envelope_reuses_catalog_until_segment_boundary() {
        let mut stats = SessionStats::default();
        let tools = Arc::new(vec![function_tool("exec_command"), function_tool("zeta")]);

        let first = stats.request_envelope_shared(
            "model",
            "provider",
            "build",
            "fixed".to_string(),
            Some(Arc::clone(&tools)),
            7,
            11,
        );
        let second = stats.request_envelope_shared(
            "model",
            "provider",
            "build",
            "fixed".to_string(),
            Some(Arc::clone(&tools)),
            7,
            11,
        );

        assert_eq!(first.segment_id(), second.segment_id());
        assert!(Arc::ptr_eq(stats.request_envelope_source_tools.as_ref().expect("source catalog"), &tools));

        let transition = stats.begin_request_segment(SegmentBoundaryReason::Compaction);
        assert_eq!(transition.previous_segment_id.as_deref(), Some(first.segment_id()));
        assert!(stats.request_envelope_source_tools.is_none());

        let third = stats.request_envelope_shared(
            "model",
            "provider",
            "build",
            "fixed".to_string(),
            Some(Arc::clone(&tools)),
            7,
            11,
        );
        assert_ne!(first.segment_id(), third.segment_id());
    }

    #[test]
    fn shared_request_envelope_preserves_equivalent_distinct_catalogs() {
        let mut stats = SessionStats::default();
        let first_tools = Arc::new(vec![function_tool("zeta"), function_tool("exec_command")]);
        let second_tools = Arc::new(vec![function_tool("exec_command"), function_tool("zeta")]);

        let first =
            stats.request_envelope_shared("model", "provider", "build", "fixed".to_string(), Some(first_tools), 7, 11);
        let second =
            stats.request_envelope_shared("model", "provider", "build", "fixed".to_string(), Some(second_tools), 7, 11);

        assert_eq!(first.segment_id(), second.segment_id());
        assert_eq!(first.catalog_hash(), second.catalog_hash());
        assert_eq!(first.ordered_tools(), second.ordered_tools());
    }

    #[test]
    fn compaction_reserves_exactly_one_new_segment() {
        let mut stats = SessionStats::default();
        let first = stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
        let before_bytes = serde_json::to_vec(&(
            first.system_prompt().to_string(),
            first.ordered_tools().as_ref().clone(),
            first.instruction_digest(),
        ))
        .expect("serialize request envelope");
        let transition = stats.begin_request_segment(SegmentBoundaryReason::Compaction);
        let second = stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
        let after_bytes = serde_json::to_vec(&(
            first.system_prompt().to_string(),
            first.ordered_tools().as_ref().clone(),
            first.instruction_digest(),
        ))
        .expect("serialize request envelope");

        assert_eq!(transition.boundary_reason, SegmentBoundaryReason::Compaction);
        assert_eq!(transition.previous_segment_id.as_deref(), Some(first.segment_id()));
        assert_eq!(transition.new_segment_id, second.segment_id());
        assert_ne!(first.segment_id(), second.segment_id());
        assert_eq!(before_bytes, after_bytes);
    }

    #[test]
    fn tool_catalog_observability_is_change_only() {
        let mut stats = SessionStats::default();
        let tools = vec!["exec_command".to_string(), "search_tools".to_string()];
        let skills = vec!["rust".to_string()];

        assert!(stats.note_tool_catalog_observability_change(&tools, 5, 2, 3, &skills));
        assert!(!stats.note_tool_catalog_observability_change(&tools, 5, 2, 3, &skills));
        assert!(stats.note_tool_catalog_observability_change(&tools, 6, 2, 4, &skills));
    }

    #[test]
    fn stream_timeout_billing_advisory_fires_once() {
        let mut stats = SessionStats::default();
        let first = stats.stream_timeout_billing_advisory("merge-gateway");
        assert!(first.is_some());
        let second = stats.stream_timeout_billing_advisory("merge-gateway");
        assert!(second.is_none());
        let mut other = SessionStats::default();
        assert!(other.stream_timeout_billing_advisory("ollama").is_none());
        let mut remote = SessionStats::default();
        let openai = remote.stream_timeout_billing_advisory("openai");
        assert!(openai.is_some());
        assert!(openai.unwrap().contains("full prompt"));
        assert!(remote.stream_timeout_billing_advisory("anthropic").is_none());
    }

    #[test]
    fn model_change_advisory_fires_once_per_switch() {
        let mut stats = SessionStats::default();
        // Session start records "model" but must not advise: there is no
        // prior cache to invalidate.
        assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "model");
        assert_eq!(stats.model_change_advisory(), None);
        // Same model, stable prefix: no advisory.
        assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "unchanged");
        assert_eq!(stats.model_change_advisory(), None);
        // Genuine switch: advisory fires on the first request carrying it.
        assert_eq!(stats.record_prompt_cache_fingerprint("model-b", 1, Some(2)), "model");
        let advisory = stats.model_change_advisory().expect("switch must advise");
        assert!(advisory.contains("Model changed mid-session"));
        // Settled on the new model: advisory clears without any flag.
        assert_eq!(stats.record_prompt_cache_fingerprint("model-b", 1, Some(2)), "unchanged");
        assert_eq!(stats.model_change_advisory(), None);
        // Switching back advises again: each genuine change re-pays.
        assert_eq!(stats.record_prompt_cache_fingerprint("model-a", 1, Some(2)), "model");
        assert!(stats.model_change_advisory().is_some());
    }

    #[test]
    fn first_call_composition_captures_once() {
        use super::FirstCallComposition;
        let mut stats = SessionStats::default();
        assert!(stats.first_call_composition().is_none());
        let first = FirstCallComposition {
            system_prompt_tokens: 1_100,
            tool_schema_tokens: 2_100,
            message_history_tokens: 40,
            on_wire_tools: 4,
        };
        assert!(stats.record_first_call_composition(first));
        assert_eq!(stats.first_call_composition(), Some(first));
        // Later assemblies must not overwrite the first-call snapshot.
        let later = FirstCallComposition {
            system_prompt_tokens: 9_000,
            tool_schema_tokens: 9_000,
            message_history_tokens: 9_000,
            on_wire_tools: 40,
        };
        assert!(!stats.record_first_call_composition(later));
        assert_eq!(stats.first_call_composition(), Some(first));
        assert_eq!(first.fixed_overhead_tokens(), 3_200);
    }

    #[test]
    fn internal_continuations_cannot_reset_cross_turn_budgets() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{
            plan_mode_continue_follow_up, recoverable_blocked_continue_follow_up, tracker_continue_follow_up,
        };

        let prompts = [
            tracker_continue_follow_up(&["#1 README (blocked)".to_owned()]),
            plan_mode_continue_follow_up(),
            recoverable_blocked_continue_follow_up("preview budget exhausted"),
        ];
        for prompt in prompts {
            let mut stats = SessionStats::default();
            for turn in 1..=3 {
                assert!(stats.record_tracker_continuation_turn_with_limit(3));
                assert!(stats.record_plan_continuation_turn_with_limit(3));
                assert!(stats.record_verification_auto_recovery_turn_with_limit(3));
                assert_eq!(stats.register_follow_up_prompt(&prompt), FollowUpPromptAction::None);
                assert_eq!(stats.tracker_continuation_turns(), turn);
                assert_eq!(stats.plan_continuation_turns(), turn);
                assert_eq!(stats.verification_auto_recovery_turns(), turn);
            }
            assert!(!stats.record_tracker_continuation_turn_with_limit(3));
            assert!(!stats.record_plan_continuation_turn_with_limit(3));
            assert!(!stats.record_verification_auto_recovery_turn_with_limit(3));
            assert_eq!(stats.register_follow_up_prompt("Fix the README links"), FollowUpPromptAction::None);
            assert_eq!(stats.tracker_continuation_turns(), 0);
            assert_eq!(stats.verification_auto_recovery_turns(), 0);
        }
    }

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
        assert_eq!(action, FollowUpPromptAction::RecoverFromStall { stall_reason: Some("turn blocked".to_string()) });
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn blocked"));
    }

    #[test]
    fn verification_pending_survives_stall_recovery_until_explicitly_cleared() {
        let mut stats = SessionStats::default();
        stats.set_verification_snapshot((true, 0));
        stats.mark_turn_stalled(true, Some("verification pending".to_string()));
        stats.mark_turn_stalled(false, None);

        assert!(stats.verification_snapshot().0);
        stats.set_verification_snapshot((false, 0));
        assert!(!stats.verification_snapshot().0);
    }

    #[test]
    fn tool_loop_grant_preauthorized_latches_only_after_explicit_grant() {
        let mut stats = SessionStats::default();
        assert!(!stats.tool_loop_grant_preauthorized(), "session starts unlatched");

        stats.mark_tool_loop_grant_preauthorized();
        assert!(stats.tool_loop_grant_preauthorized());

        // Idempotent: later grants do not flip it back.
        stats.mark_tool_loop_grant_preauthorized();
        assert!(stats.tool_loop_grant_preauthorized());
    }

    #[test]
    fn tool_loop_grant_preauthorized_survives_fresh_execution_in_session() {
        let mut stats = SessionStats::default();
        stats.mark_tool_loop_grant_preauthorized();
        stats.reset_for_fresh_execution();
        assert!(
            stats.tool_loop_grant_preauthorized(),
            "the one-time HITL latch is process-session scoped, not conversation-context scoped"
        );
    }

    #[test]
    fn preflight_fresh_execution_clears_validation_without_restoring_continuations() {
        let mut stats = SessionStats::default();
        stats.record_tracker_continuation_turn_with_limit(3);
        stats.record_plan_continuation_turn_with_limit(3);
        stats
            .preflight_recovery
            .reject("task_tracker", ValidationCategory::Arguments, None, 6, false);
        stats.preflight_recovery.synthesis();
        stats.preflight_recovery.begin_corrective_turn();
        stats.preflight_recovery.finish_corrective_turn();
        assert!(stats.preflight_recovery.exhausted());

        stats.reset_for_fresh_execution();

        assert!(stats.preflight_recovery.is_clear());
        assert!(!stats.preflight_recovery.exhausted());
        assert_eq!(stats.tracker_continuation_turns(), 1);
        assert_eq!(stats.plan_continuation_turns(), 1);
    }

    #[test]
    fn verification_auto_recovery_turns_are_bounded_and_reset_on_new_request() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_TURNS;

        let mut stats = SessionStats::default();
        for _ in 0..MAX_VERIFICATION_AUTO_RECOVERY_TURNS {
            assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        }
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        assert_eq!(stats.verification_auto_recovery_turns(), MAX_VERIFICATION_AUTO_RECOVERY_TURNS);

        stats.reset_verification_recovery_episode();
        assert_eq!(stats.verification_auto_recovery_turns(), MAX_VERIFICATION_AUTO_RECOVERY_TURNS);
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));

        // A fresh non-follow-up user request resets the cross-turn budget.
        assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
    }

    #[test]
    fn verification_auto_failure_escalation_counts_and_resets_as_episode() {
        let mut stats = SessionStats::default();
        assert_eq!(stats.verification_consecutive_failures(), 0);
        assert!(stats.last_verification_failure().is_none());

        let first = stats.record_verification_auto_failure("cargo check --locked".to_string(), "error[A]: boom");
        assert_eq!(first, 1);
        let second = stats.record_verification_auto_failure("cargo check --locked".to_string(), "error[A]: boom again");
        assert_eq!(second, 2);
        let summary = stats.last_verification_failure().expect("failure tail must be kept");
        assert_eq!(summary.command, "cargo check --locked");
        assert_eq!(summary.consecutive_failures, 2);
        assert!(summary.excerpt_tail.contains("boom again"));

        // Long output is bounded to the tail at record time.
        let long = format!("x{} tail-marker", "y".repeat(5000));
        stats.record_verification_auto_failure("pytest -q".to_string(), &long);
        let bounded = stats.last_verification_failure().expect("bounded tail must be kept");
        assert!(bounded.excerpt_tail.chars().count() <= 2000);
        assert!(bounded.excerpt_tail.contains("tail-marker"));
        assert!(!bounded.excerpt_tail.starts_with('x'), "head must be elided, not kept");

        stats.record_verification_auto_success();
        assert_eq!(stats.verification_consecutive_failures(), 0);
        assert!(stats.last_verification_failure().is_none());

        stats.record_verification_auto_failure("cargo check --locked".to_string(), "boom");
        stats.reset_verification_recovery_episode();
        assert_eq!(stats.verification_consecutive_failures(), 0);
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
        assert!(stats.last_verification_failure().is_none());
    }

    #[test]
    fn verification_recovery_is_not_replenished_by_internal_progress() {
        let mut stats = SessionStats::default();
        assert!(stats.record_verification_auto_recovery_turn_with_limit(1));
        stats.record_verification_auto_failure("cargo check".into(), "{\"exit_code\":1}");
        stats.reset_verification_recovery_episode();
        stats.register_follow_up_prompt(
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::TRACKER_CONTINUE_FOLLOW_UP_PREFIX,
        );
        assert_eq!(stats.verification_auto_recovery_turns(), 1);
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(1));
        stats.register_follow_up_prompt("Please implement another request");
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
        assert!(stats.record_verification_auto_recovery_turn_with_limit(1));
    }

    #[test]
    fn non_follow_up_resets_follow_up_tracking() {
        let mut stats = SessionStats::default();
        stats.mark_turn_stalled(true, Some("turn aborted".to_string()));
        let _ = stats.register_follow_up_prompt("continue");
        let _ = stats.register_follow_up_prompt("continue");
        assert!(stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), Some("turn aborted"));

        assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }

    #[test]
    fn follow_up_prompt_variants_are_detected() {
        let mut stats = SessionStats::default();
        assert_eq!(stats.register_follow_up_prompt("continue."), FollowUpPromptAction::None);
        assert_eq!(stats.register_follow_up_prompt("continue with your recommendation"), FollowUpPromptAction::None);
        assert_eq!(stats.register_follow_up_prompt("please continue"), FollowUpPromptAction::ForceConclusion);
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

        assert_eq!(stats.register_follow_up_prompt("run tests and summarize"), FollowUpPromptAction::None);
        assert!(!stats.turn_stalled());
        assert_eq!(stats.turn_stall_reason(), None);
    }

    #[test]
    fn follow_up_prompt_action_exposes_stall_reason() {
        let action = FollowUpPromptAction::RecoverFromStall { stall_reason: Some("blocked".to_string()) };

        assert!(action.should_force_autonomous_response());
        assert!(action.is_stalled_recovery());
        assert_eq!(action.stall_reason(), Some("blocked"));
    }

    #[test]
    fn helper_detects_follow_up_variants() {
        assert!(is_follow_up_prompt_like("continue"));
        assert!(is_follow_up_prompt_like("continue."));
        assert!(is_follow_up_prompt_like("please continue"));
        assert!(is_follow_up_prompt_like("Continue autonomously from the last stalled turn. Stall reason: x."));
        assert!(!is_follow_up_prompt_like("run tests and summarize"));
    }

    #[test]
    fn safe_mode_prompts_are_disabled_for_auto_permission() {
        assert!(!should_enforce_safe_mode_prompts(false, true, Some(WorkspaceTrustLevel::ToolsPolicy),));
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
        assert_eq!(stats.record_prompt_cache_fingerprint("gpt-5", 55, Some(66)), "stable_prefix+tool_catalog");
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
    fn prompt_cache_fingerprint_attributes_tools_omitted_and_recovery_reason() {
        let mut stats = SessionStats::default();

        assert_eq!(stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), None), "model");
        assert_eq!(
            stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), None),
            "unchanged"
        );
        // Dropping every tool from the wire is its own miss cause.
        assert_eq!(
            stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(0), None),
            "tools_omitted"
        );
        // Restoring tools then rotating the frozen recovery reason is next.
        assert_eq!(
            stats.record_prompt_cache_fingerprint_with_context("gpt-6", 1, Some(2), Some(10), Some("loop detector")),
            "unchanged"
        );
        assert_eq!(
            stats.record_prompt_cache_fingerprint_with_context(
                "gpt-6",
                1,
                Some(2),
                Some(10),
                Some("blocked tool-call fuse tripped")
            ),
            "recovery_reason"
        );
    }

    #[test]
    fn fresh_execution_reset_clears_context_lineage_and_diagnostics() {
        let mut stats = SessionStats::default();
        stats.set_prompt_cache_lineage_id(Some("lineage-1".to_string()));
        stats.record_prompt_cache_fingerprint("gpt-5", 11, Some(22));
        stats.request_envelope("model", "provider", "build", "fixed".to_string(), vec![], 7);
        stats.note_request_sent();
        stats.set_stop_reason(Some("length".to_string()));

        stats.reset_for_fresh_execution();

        assert_eq!(stats.prompt_cache_lineage_id(), None);
        assert_eq!(stats.prompt_cache_diagnostics(), PromptCacheDiagnostics::default());
        assert!(!stats.has_sent_request());
        assert_eq!(stats.stop_reason(), None);
        assert!(stats.request_envelope.is_none());
        assert!(stats.request_envelope_identity.is_none());
        assert!(stats.pending_request_segment_id.is_none());
    }

    #[test]
    fn previous_response_chain_clears_only_matching_scope() {
        let mut stats = SessionStats::default();
        let openai_messages = vec![vtcode_core::llm::provider::Message::user("hello".to_string())];
        let gemini_messages = vec![vtcode_core::llm::provider::Message::user("hi".to_string())];
        stats.set_previous_response_chain("openai", "gpt-5.6-sol", Some("resp_openai"), &openai_messages);
        stats.set_previous_response_chain("gemini", "gemini-2.5-pro", Some("resp_gemini"), &gemini_messages);

        stats.clear_previous_response_chain_for("openai", "gpt-5.6-sol");

        assert_eq!(stats.previous_response_id_for("openai", "gpt-5.6-sol"), None);
        assert_eq!(stats.previous_response_chain_for("openai", "gpt-5.6-sol"), None);
        assert_eq!(stats.previous_response_id_for("gemini", "gemini-2.5-pro"), Some("resp_gemini".to_string()));
        assert_eq!(
            stats
                .previous_response_chain_for("gemini", "gemini-2.5-pro")
                .map(|chain| chain.messages.as_slice()),
            Some(gemini_messages.as_slice())
        );
    }

    #[test]
    fn shared_previous_response_chain_setter_preserves_message_arc() {
        let mut stats = SessionStats::default();
        let messages = Arc::new(vec![vtcode_core::llm::provider::Message::user("hello".to_string())]);

        stats.set_previous_response_chain_shared(
            "gemini",
            "gemini-2.5-pro",
            Some("resp_gemini"),
            Arc::clone(&messages),
        );

        let stored_messages = &stats
            .previous_response_chain_for("gemini", "gemini-2.5-pro")
            .expect("shared response chain should be recorded")
            .messages;
        assert!(Arc::ptr_eq(&messages, stored_messages));
    }

    #[test]
    fn safe_mode_prompts_follow_workspace_trust_for_edit_mode() {
        assert!(should_enforce_safe_mode_prompts(false, false, Some(WorkspaceTrustLevel::ToolsPolicy),));
        assert!(!should_enforce_safe_mode_prompts(false, false, Some(WorkspaceTrustLevel::FullAuto),));
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
    fn ctrl_c_state_local_cancel_does_not_arm_exit_window() {
        let state = CtrlCState::new();

        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        state.mark_cancel_handled();
        state.request_local_cancel();

        assert!(state.is_cancel_requested());
        assert!(!state.is_exit_requested());
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
    fn ctrl_c_state_reset_preserves_exit() {
        let state = CtrlCState::new();

        // Get to exit state
        assert!(matches!(state.register_signal(), CtrlCSignal::Cancel));
        thread::sleep(Duration::from_millis(250));
        assert!(matches!(state.register_signal(), CtrlCSignal::Exit));
        assert!(state.is_exit_requested());

        // Only a new session may clear exit, by creating a new state.
        state.reset();

        assert!(!state.is_cancel_requested());
        assert!(state.is_exit_requested());
        assert!(state.check_cancellation().is_err());
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
