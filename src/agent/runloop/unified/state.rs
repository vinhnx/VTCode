use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn is_local_llm_provider(provider_name: &str) -> bool {
    matches!(
        provider_name.trim().to_ascii_lowercase().as_str(),
        "ollama" | "lmstudio" | "llamacpp" | "llama.cpp" | "local" | "local_server"
    )
}

use vtcode_core::compaction::PrefireState;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::core::agent::harness_kernel::hash_value;
use vtcode_core::core::agent::request_envelope::{SegmentBoundaryReason, SessionRequestEnvelope};
use vtcode_core::exec::events::Usage as HarnessUsage;
use vtcode_core::llm::provider::{
    Message, PromptCacheProfile, ResponsesContinuationState, ToolDefinition, responses_continuation_key,
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
    /// Bounded fix-up edits remaining while `verification_pending` is true.
    /// Granted by a failed verifier so a broken build can be repaired across
    /// `continue` turns; consumed by successful fix-up mutations.
    verification_fix_remaining: u8,
    /// Bounded autonomous turns the session loop may schedule after a
    /// verification-blocked turn before requiring manual `continue`.
    /// Reset on every `Completed` turn and on fresh execution; incremented by
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
    /// Consecutive plan-mode turns that ended with the deterministic empty
    /// fallback (`PLANNING_COMPLETED_FALLBACK_RESPONSE`: no LLM synthesis, no
    /// tool activity). Breaks the self-loop observed in
    /// session-vtcode-20260921T045723Z where 32 empty turns each re-queued
    /// auto-continue and ended `Blocked` with zero progress. Reset on any
    /// turn with LLM text or tool activity; capped by
    /// `MAX_PLAN_EMPTY_FALLBACK_AUTO_CONTINUE`.
    consecutive_plan_empty_fallbacks: u8,
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
    previous_response_chains: HashMap<(String, String), ResponsesContinuationState>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RequestEnvelopeIdentity {
    model: String,
    provider: String,
    mode: String,
    /// Full prompt identity used to refresh dynamic suffix bytes in-place.
    /// This is deliberately excluded from the stable segment identity below.
    system_prompt_hash: u64,
    prefix_hash: u64,
    catalog_hash: Option<u64>,
    instruction_digest: u64,
    stable_prompt_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolCatalogObservabilityIdentity {
    ordered_wire_tool_names: Vec<String>,
    catalog_tool_count: usize,
    wire_tool_count: usize,
    deferred_tool_count: usize,
    active_loaded_skill_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestSegmentTransition {
    pub boundary_reason: SegmentBoundaryReason,
    pub previous_segment_id: Option<String>,
    pub new_segment_id: String,
    pub previous_prefix_hash: Option<String>,
    pub previous_catalog_hash: Option<String>,
}

impl SessionStats {
    pub(crate) fn begin_request_segment(&mut self, boundary_reason: SegmentBoundaryReason) -> RequestSegmentTransition {
        let previous_segment_id = self.request_envelope.as_ref().map(|envelope| envelope.segment_id().to_string());
        let previous_prefix_hash = self
            .request_envelope
            .as_ref()
            .map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let previous_catalog_hash = self
            .request_envelope
            .as_ref()
            .and_then(SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
        let new_segment_id = format!("segment-{:08}", self.request_segment_sequence);
        self.pending_request_segment_id = Some(new_segment_id.clone());
        self.request_envelope = None;
        self.request_envelope_identity = None;
        self.request_envelope_source_tools = None;
        RequestSegmentTransition {
            boundary_reason,
            previous_segment_id,
            new_segment_id,
            previous_prefix_hash,
            previous_catalog_hash,
        }
    }

    #[allow(dead_code, reason = "retained as a Vec-based compatibility wrapper for state tests")]
    pub(crate) fn request_envelope(
        &mut self,
        model: &str,
        provider: &str,
        mode: &str,
        system_prompt: String,
        tools: Vec<ToolDefinition>,
        instruction_digest: u64,
    ) -> SessionRequestEnvelope {
        self.request_envelope_shared(
            model,
            provider,
            mode,
            system_prompt,
            Some(Arc::new(tools)),
            instruction_digest,
            instruction_digest,
        )
    }

    pub(crate) fn request_envelope_shared(
        &mut self,
        model: &str,
        provider: &str,
        mode: &str,
        system_prompt: String,
        tools: Option<Arc<Vec<ToolDefinition>>>,
        instruction_digest: u64,
        stable_prompt_hash: u64,
    ) -> SessionRequestEnvelope {
        let system_prompt_hash = hash_value(&system_prompt);
        let source_matches = match (&self.request_envelope_source_tools, &tools) {
            (None, None) => true,
            (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
            _ => false,
        };
        let identity_matches_without_catalog = self.request_envelope_identity.as_ref().is_some_and(|identity| {
            identity.model == model
                && identity.provider == provider
                && identity.mode == mode
                && identity.system_prompt_hash == system_prompt_hash
                && identity.instruction_digest == instruction_digest
                && identity.stable_prompt_hash == stable_prompt_hash
        });
        if source_matches
            && identity_matches_without_catalog
            && let Some(envelope) = self.request_envelope.as_ref()
        {
            return envelope.clone();
        }

        let stable_prefix_hash = hash_value(&(instruction_digest, stable_prompt_hash));
        let candidate = SessionRequestEnvelope::with_prefix_hash(
            "candidate",
            system_prompt,
            tools.as_ref().map_or_else(Vec::new, |tools| tools.as_ref().clone()),
            instruction_digest,
            stable_prefix_hash,
        );
        let identity = RequestEnvelopeIdentity {
            model: model.to_string(),
            provider: provider.to_string(),
            mode: mode.to_string(),
            system_prompt_hash,
            prefix_hash: candidate.prefix_hash(),
            catalog_hash: candidate.catalog_hash(),
            instruction_digest,
            stable_prompt_hash,
        };
        if self.request_envelope_identity.as_ref() == Some(&identity)
            && let Some(envelope) = self.request_envelope.as_ref()
        {
            self.request_envelope_source_tools = tools;
            return envelope.clone();
        }

        let same_stable_segment = self.request_envelope_identity.as_ref().is_some_and(|previous| {
            previous.model == identity.model
                && previous.provider == identity.provider
                && previous.mode == identity.mode
                && previous.prefix_hash == identity.prefix_hash
                && previous.catalog_hash == identity.catalog_hash
                && previous.instruction_digest == identity.instruction_digest
                && previous.stable_prompt_hash == identity.stable_prompt_hash
        });
        if let Some(previous_identity) = self.request_envelope_identity.as_ref()
            && !same_stable_segment
        {
            let boundary_reason = request_identity_boundary_reason(previous_identity, &identity);
            self.begin_request_segment(boundary_reason);
        }

        let segment_id = if same_stable_segment {
            self.request_envelope
                .as_ref()
                .map(|envelope| envelope.segment_id().to_owned())
                .or_else(|| self.pending_request_segment_id.take())
                .unwrap_or_else(|| {
                    self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
                    format!("segment-{:08}", self.request_segment_sequence)
                })
        } else if let Some(pending_segment_id) = self.pending_request_segment_id.take() {
            pending_segment_id
        } else {
            self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
            format!("segment-{:08}", self.request_segment_sequence)
        };
        // Reuse the candidate's shared `Arc<str>`/`Arc<Vec<ToolDefinition>>`
        // owners and only swap the segment id. Rebuilding via
        // `with_prefix_hash` would deep-clone the whole tool catalog and
        // re-sort/re-hash it, even though `candidate` already holds the
        // canonicalized, hashed form. `begin_segment` keeps the frozen prompt,
        // catalog, and hashes byte-identical while cloning only Arcs.
        let envelope = candidate.begin_segment(segment_id);
        self.request_envelope_identity = Some(identity);
        self.request_envelope_source_tools = tools;
        self.request_envelope = Some(envelope.clone());
        envelope
    }

    pub(crate) fn note_tool_catalog_observability_change(
        &mut self,
        ordered_wire_tool_names: &[String],
        catalog_tool_count: usize,
        wire_tool_count: usize,
        deferred_tool_count: usize,
        active_loaded_skill_names: &[String],
    ) -> bool {
        let identity = ToolCatalogObservabilityIdentity {
            ordered_wire_tool_names: ordered_wire_tool_names.to_vec(),
            catalog_tool_count,
            wire_tool_count,
            deferred_tool_count,
            active_loaded_skill_names: active_loaded_skill_names.to_vec(),
        };
        if self.last_tool_catalog_observability.as_ref() == Some(&identity) {
            return false;
        }
        self.last_tool_catalog_observability = Some(identity);
        true
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

    pub(crate) fn register_follow_up_prompt(&mut self, input: &str) -> FollowUpPromptAction {
        // Internal continuations belong to the existing recovery episode.
        // Treating their verbose text as fresh user input resets the budget
        // on every retry and makes the cross-turn limit unreachable.
        if crate::agent::runloop::unified::turn::is_internal_harness_follow_up(input) {
            return FollowUpPromptAction::None;
        }
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
            self.reset_verification_recovery_episode();
            self.reset_tracker_continuation_budget();
            self.tracker_completed_count_high_water = 0;
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

    /// Bundle counterpart to `LoopTracker::verification_snapshot`. Turn setup
    /// and persistence must move both halves together; threading the tuple
    /// through one call keeps a pending gate from losing its fix window.
    pub(crate) fn verification_snapshot(&self) -> (bool, u8) {
        (self.verification_pending, self.verification_fix_remaining)
    }

    pub(crate) fn set_verification_snapshot(&mut self, snapshot: (bool, u8)) {
        self.verification_pending = snapshot.0;
        self.verification_fix_remaining = if snapshot.0 { snapshot.1 } else { 0 };
    }

    /// Config-aware cross-turn recovery recorder; prefer it at call sites with
    /// workspace config access so `[agent.harness.verification].cross_turn_turns`
    /// is honored. Returns `true` when budget remained (the caller should
    /// queue a recovery turn instead of writing a blocked handoff); `false`
    /// once exhausted and manual `continue` is required.
    pub(crate) fn record_verification_auto_recovery_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.verification_auto_recovery_turns >= max_turns {
            return false;
        }
        self.verification_auto_recovery_turns = self.verification_auto_recovery_turns.saturating_add(1);
        true
    }

    pub(crate) fn verification_auto_recovery_turns(&self) -> u8 {
        self.verification_auto_recovery_turns
    }

    /// Record one tracker auto-continue turn. Returns `true` when budget
    /// remained (`max_turns`); `false` once exhausted.
    pub(crate) fn record_tracker_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.tracker_continuation_turns >= max_turns {
            return false;
        }
        self.tracker_continuation_turns = self.tracker_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn tracker_continuation_turns(&self) -> u8 {
        self.tracker_continuation_turns
    }

    pub(crate) fn reset_tracker_continuation_budget(&mut self) {
        self.tracker_continuation_turns = 0;
    }

    /// Apply a live tracker probe to the incomplete-items cache.
    ///
    /// Successful `Complete` probes **clear** the cache so auto-continue stops
    /// after the tracker finishes. `Unavailable` keeps the last value.
    pub(crate) fn apply_tracker_probe(
        &mut self,
        probe: crate::agent::runloop::unified::turn::tool_outcomes::helpers::TrackerProbeOutcome,
    ) -> Option<&[String]> {
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::apply_tracker_probe_to_cache(
            &mut self.last_incomplete_tracker_items,
            probe,
        )
    }

    /// Only new completion progress restores the auto-continue episode.
    /// Recreating a checklist or toggling statuses must not refresh retries.
    pub(crate) fn note_tracker_completed_count(&mut self, completed: u32) -> bool {
        let progressed = completed > self.tracker_completed_count_high_water;
        if progressed {
            self.tracker_completed_count_high_water = completed;
        }
        progressed
    }

    /// Record one plan-mode auto-continue turn against its own budget.
    pub(crate) fn record_plan_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.plan_continuation_turns >= max_turns {
            return false;
        }
        self.plan_continuation_turns = self.plan_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn plan_continuation_turns(&self) -> u8 {
        self.plan_continuation_turns
    }

    pub(crate) fn reset_plan_continuation_budget(&mut self) {
        self.plan_continuation_turns = 0;
        self.consecutive_plan_empty_fallbacks = 0;
    }

    pub(crate) fn consecutive_plan_empty_fallbacks(&self) -> u8 {
        self.consecutive_plan_empty_fallbacks
    }

    pub(crate) fn record_plan_empty_fallback(&mut self) -> u8 {
        self.consecutive_plan_empty_fallbacks = self.consecutive_plan_empty_fallbacks.saturating_add(1);
        self.consecutive_plan_empty_fallbacks
    }

    pub(crate) fn reset_plan_empty_fallbacks(&mut self) {
        self.consecutive_plan_empty_fallbacks = 0;
    }

    /// Record a failed harness auto-verification of `command`, keeping a
    /// bounded tail of its output for the escalated handoff. Returns the new
    /// consecutive-failure count so callers can compare against the
    /// configured escalation threshold without a second accessor call.
    pub(crate) fn record_verification_auto_failure(&mut self, command: String, excerpt: &str) -> u8 {
        const BOUND: usize =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::VERIFICATION_FAILURE_EXCERPT_CHARS;
        // Keep the last BOUND chars (the tail usually holds the error
        // summary) without materializing the full char vector.
        let start = excerpt
            .char_indices()
            .rev()
            .nth(BOUND.saturating_sub(1))
            .map_or(0, |(idx, _)| idx);
        let tail = excerpt[start..].to_string();
        self.verification_consecutive_failures = self.verification_consecutive_failures.saturating_add(1);
        let consecutive_failures = self.verification_consecutive_failures;
        self.last_verification_failure =
            Some(VerificationFailureSummary { command, excerpt_tail: tail, consecutive_failures });
        consecutive_failures
    }

    /// Record a successful harness auto-verification: the suite is green, so
    /// the consecutive-failure count and any stored failure tail are dropped.
    pub(crate) fn record_verification_auto_success(&mut self) {
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
    }

    pub(crate) fn verification_consecutive_failures(&self) -> u8 {
        self.verification_consecutive_failures
    }

    pub(crate) fn last_verification_failure(&self) -> Option<&VerificationFailureSummary> {
        self.last_verification_failure.as_ref()
    }

    /// Reset the whole verification-recovery episode: cross-turn budget,
    /// consecutive-failure count, and stored failure tail. Call on
    /// `Completed` turns, cancellation/exit, fresh execution, and fresh user
    /// instructions — every point where past verification struggles stop
    /// being relevant to future work.
    pub(crate) fn reset_verification_recovery_episode(&mut self) {
        self.verification_auto_recovery_turns = 0;
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
        // Tracker/plan continuation budgets are independent episode counters;
        // do not wipe them here (Completed-turn verification resets must not
        // silently restore full tracker auto-continue budgets).
    }

    /// Reset only the cross-turn auto-recovery turn budget, preserving the
    /// consecutive-failure escalation counter. Called when tracker progress
    /// proves long-running work is advancing: fresh verification misses for
    /// new work get a new bounded recovery episode, while a genuinely
    /// never-passing suite still escalates via the preserved failure count.
    pub(crate) fn reset_verification_auto_recovery_turns(&mut self) {
        self.verification_auto_recovery_turns = 0;
    }

    #[cfg(test)]
    fn turn_stalled(&self) -> bool {
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
        responses_continuation_key(provider, model).and_then(|key| self.previous_response_chains.get(&key))
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

    #[cfg(test)]
    pub(crate) fn record_prompt_cache_fingerprint(
        &mut self,
        model: &str,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
    ) -> &'static str {
        self.record_prompt_cache_fingerprint_with_context(model, stable_prefix_hash, tool_catalog_hash, None, None)
    }

    /// Fingerprint with recovery/wire context so cache misses can be
    /// attributed to tool-catalog omission or `[Recovery Mode]` reason churn
    /// instead of a generic stable-prefix change.
    pub(crate) fn record_prompt_cache_fingerprint_with_context(
        &mut self,
        model: &str,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
        wire_tool_count: Option<usize>,
        recovery_prompt_reason: Option<&str>,
    ) -> &'static str {
        let reason = if self.last_prompt_cache_model.as_deref() != Some(model) {
            "model"
        } else if matches!(
            (self.last_wire_tool_count, wire_tool_count),
            (Some(prev), Some(curr)) if prev > 0 && curr == 0
        ) {
            "tools_omitted"
        } else if self.last_recovery_prompt_reason.as_deref() != recovery_prompt_reason
            && recovery_prompt_reason.is_some()
            && self.last_recovery_prompt_reason.is_some()
        {
            "recovery_reason"
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
        self.last_wire_tool_count = wire_tool_count;
        self.last_recovery_prompt_reason = recovery_prompt_reason.map(str::to_string);
        self.last_prompt_cache_change_reason = Some(reason.to_string());

        reason
    }

    /// Advisory for mid-session model switches, derived from the last
    /// recorded fingerprint. Prompt caches are unique per model, so a switch
    /// rebuilds the cache at full input cost even when the rest of the prefix
    /// is unchanged. Returns `Some` exactly on the first request carrying a
    /// new model (the session-start recording is excluded via the
    /// observations guard); callers should surface the message and keep
    /// going — no state is consumed.
    pub(crate) fn model_change_advisory(&self) -> Option<String> {
        (self.prompt_cache_observations > 1
            && self.last_prompt_cache_change_reason.as_deref() == Some("model"))
        .then(|| {
            "Model changed mid-session; provider prompt cache will be invalidated and the next request re-pays full \
             input cost. Prefer resolving the model up front; expect one full-price request before hits resume."
                .to_string()
        })
    }

    /// One-shot advisory when a remote non-streaming-capable provider falls
    /// back after stream first-token timeout: abandoned work may still be
    /// billed and the retry re-sends the full prompt. Local providers are
    /// excluded (no remote bill).
    pub(crate) fn stream_timeout_billing_advisory(&mut self, provider_name: &str) -> Option<String> {
        if self.stream_timeout_advisory_emitted || is_local_llm_provider(provider_name) {
            return None;
        }
        self.stream_timeout_advisory_emitted = true;
        let merge = provider_name.eq_ignore_ascii_case("merge-gateway");
        if merge {
            Some(
                "Merge Gateway stream timed out before first token; falling back to non-streaming. Abandoned provider \
                 streams may still be drained and billed, and the retry re-sends the full prompt."
                    .to_string(),
            )
        } else {
            Some(format!(
                "LLM stream timed out on {provider_name} before first token; falling back to non-streaming. \
                 Abandoned provider work may still be billed, and the retry re-sends the full prompt."
            ))
        }
    }

    fn counter_for_reason(&mut self, reason: &str) -> &mut usize {
        match reason {
            "model" => &mut self.prompt_cache_model_changes,
            "unchanged" => &mut self.prompt_cache_unchanged,
            "stable_prefix" => &mut self.prompt_cache_stable_prefix_changes,
            "tool_catalog" => &mut self.prompt_cache_tool_catalog_changes,
            "stable_prefix+tool_catalog" => &mut self.prompt_cache_combined_changes,
            // Attributed miss causes: tool definitions dropped from the wire,
            // or the frozen `[Recovery Mode]` reason rotated mid-activation.
            "tools_omitted" | "recovery_reason" => &mut self.prompt_cache_stable_prefix_changes,
            _ => &mut self.prompt_cache_unchanged,
        }
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
        let Some(key) = responses_continuation_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.previous_response_chains.remove(&key);
            return;
        };

        self.previous_response_chains
            .insert(key, ResponsesContinuationState { response_id: response_id.to_string(), messages });
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

fn request_identity_boundary_reason(
    previous: &RequestEnvelopeIdentity,
    current: &RequestEnvelopeIdentity,
) -> SegmentBoundaryReason {
    if previous.model != current.model {
        SegmentBoundaryReason::Model
    } else if previous.provider != current.provider {
        SegmentBoundaryReason::Provider
    } else if previous.mode != current.mode {
        SegmentBoundaryReason::Mode
    } else if previous.instruction_digest != current.instruction_digest
        || previous.stable_prompt_hash != current.stable_prompt_hash
        || previous.prefix_hash != current.prefix_hash
    {
        SegmentBoundaryReason::Instructions
    } else if previous.catalog_hash != current.catalog_hash {
        SegmentBoundaryReason::ToolCatalogEpoch
    } else {
        SegmentBoundaryReason::PrimaryAgent
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

/// Bounded record of a failed harness auto-verification, kept for handoff
/// enrichment so the eventual manual blocker names the failing command and
/// shows its output tail instead of repeating the generic "run a verifier"
/// recipe.
#[derive(Debug, Clone, Default)]
pub(crate) struct VerificationFailureSummary {
    /// Verifier command that failed (e.g. `cargo check --locked`).
    pub command: String,
    /// Tail excerpt of the tool output, bounded to
    /// `VERIFICATION_FAILURE_EXCERPT_CHARS` chars at record time.
    pub excerpt_tail: String,
    /// Consecutive failure count including this failure.
    pub consecutive_failures: u8,
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
    exit_started: std::sync::OnceLock<std::time::Instant>,
    last_signal_time: AtomicU64,
    /// Shared event-delivery counter: incremented by the UI event callback
    /// each time a Steer input is accepted by the live steering channel,
    /// decremented by the runloop Steer handler. Callback and handler run
    /// sequentially per event (callback first, then the channel), so an
    /// undelivered steer falls through to the durable queue instead of
    /// vanishing when the agent is temporarily unavailable (no steering
    /// sender, closed channel). A counter — not a flag — so a burst of
    /// steers delivered before the runloop drains does not double-queue the
    /// second and later messages.
    steer_delivered: AtomicUsize,
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
        if phase == CtrlCPhase::ExitRequested {
            self.exit_started.get_or_init(std::time::Instant::now);
        }
        // Exit is terminal for this session, including when a UI reset races
        // the callback that accepts exit.
        let _ = self.phase.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            (current != CtrlCPhase::ExitRequested as u8).then_some(phase as u8)
        });
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
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
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

        if matches!(current_phase, CtrlCPhase::CancelRequested | CtrlCPhase::ExitArmed) && is_within_window {
            self.set_phase(CtrlCPhase::ExitRequested);
            return CtrlCSignal::Exit;
        }

        if matches!(current_phase, CtrlCPhase::ExitRequested) {
            return CtrlCSignal::Exit;
        }

        self.set_phase(CtrlCPhase::CancelRequested);
        CtrlCSignal::Cancel
    }

    /// Request cancellation from the local TUI without arming the emergency
    /// double-signal exit path used by the OS signal handler.
    pub(crate) fn request_local_cancel(&self) {
        if !matches!(self.phase(), CtrlCPhase::ExitRequested) {
            self.set_phase(CtrlCPhase::CancelRequested);
        }
        self.last_signal_time.store(0, Ordering::SeqCst);
    }

    pub(crate) fn request_exit(&self) {
        self.exit_started.get_or_init(std::time::Instant::now);
        self.phase.store(CtrlCPhase::ExitRequested as u8, Ordering::SeqCst);
    }

    pub(crate) fn exit_deadline(&self) -> Option<tokio::time::Instant> {
        self.exit_started
            .get()
            .map(|started| tokio::time::Instant::from_std(*started + Duration::from_millis(1500)))
    }

    pub(crate) fn exit_elapsed_ms(&self) -> Option<u64> {
        self.exit_started
            .get()
            .map(|started| started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
    }

    pub(crate) fn reset(&self) {
        self.set_phase(CtrlCPhase::Idle);
        self.last_signal_time.store(0, Ordering::SeqCst);
    }

    /// Record that the UI event callback accepted a Steer input on the live
    /// steering channel. Called from the callback before the event reaches
    /// the runloop. Safe to call once per delivered steer in a burst.
    pub(crate) fn mark_steer_delivered(&self) {
        self.steer_delivered.fetch_add(1, Ordering::SeqCst);
    }

    /// Consume one steer-delivery credit. `true` means the matching Steer
    /// event was already handed to the steering channel and must not be
    /// queued again; `false` means it fell through and should be queued so
    /// the message is processed once the agent is ready.
    pub(crate) fn take_steer_delivered(&self) -> bool {
        self.steer_delivered
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |pending| Some(pending.saturating_sub(1)))
            .is_ok_and(|prev| prev > 0)
    }

    pub(crate) fn mark_cancel_handled(&self) {
        let _ = self.phase.compare_exchange(
            CtrlCPhase::CancelRequested as u8,
            CtrlCPhase::ExitArmed as u8,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
    }

    pub(crate) fn is_cancel_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::CancelRequested)
    }

    pub(crate) fn is_exit_requested(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::ExitRequested)
    }

    pub(crate) fn is_cancel_handled(&self) -> bool {
        matches!(self.phase(), CtrlCPhase::ExitArmed)
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
        AutoPermissionDenial, CtrlCSignal, CtrlCState, FollowUpPromptAction, PromptCacheDiagnostics, SessionStats,
        is_follow_up_prompt_like, should_enforce_safe_mode_prompts,
    };
    use vtcode_core::config::WorkspaceTrustLevel;
    use vtcode_core::config::constants::tools;
    use vtcode_core::core::agent::request_envelope::SegmentBoundaryReason;
    use vtcode_core::llm::provider::ToolDefinition;

    fn function_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({"type": "object"}))
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
    fn verification_auto_recovery_turns_are_bounded_and_reset_on_new_request() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_TURNS;

        let mut stats = SessionStats::default();
        for _ in 0..MAX_VERIFICATION_AUTO_RECOVERY_TURNS {
            assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        }
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        assert_eq!(stats.verification_auto_recovery_turns(), MAX_VERIFICATION_AUTO_RECOVERY_TURNS);

        stats.reset_verification_recovery_episode();
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
        assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));

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
    fn verification_turn_reset_on_tracker_progress_preserves_failure_escalation() {
        use crate::agent::runloop::unified::turn::tool_outcomes::helpers::MAX_VERIFICATION_AUTO_RECOVERY_TURNS;

        let mut stats = SessionStats::default();
        // Exhaust the cross-turn budget with one consecutive failure recorded.
        for _ in 0..MAX_VERIFICATION_AUTO_RECOVERY_TURNS {
            assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        }
        assert!(!stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
        stats.record_verification_auto_failure("cargo check --locked".to_string(), "boom");

        // Tracker progress grants a fresh turn budget for new work but must
        // not hide a never-passing suite: failures survive.
        stats.reset_verification_auto_recovery_turns();
        assert_eq!(stats.verification_auto_recovery_turns(), 0);
        assert_eq!(stats.verification_consecutive_failures(), 1);
        assert!(stats.last_verification_failure().is_some());
        assert!(stats.record_verification_auto_recovery_turn_with_limit(MAX_VERIFICATION_AUTO_RECOVERY_TURNS));
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
