use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hashbrown::{HashMap, HashSet};
use tokio::sync::RwLock;
use vtcode_config::core::permissions::AgentPermissionsConfig;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::types::CompactStr;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolWallClockExhaustionNotice {
    pub exhaustion: ToolWallClockExhaustion,
    pub first_notice: bool,
}

pub(crate) const TOOL_BUDGET_WARNING_THRESHOLD: f64 = 0.75;

/// Model-facing guidance emitted after a user increases the per-turn tool
/// budget. Keep this shared by the normal and out-of-band provider paths so a
/// grant has the same continuation semantics regardless of transport.
pub(crate) const SESSION_LIMIT_GRANT_DIRECTIVE: &str = "Session tool-call limit increased by the user. Continue this same turn with the current agent, retry the pending call, and reuse existing tool outputs instead of repeating exploration.";
/// Session tool-call increase applied without prompting while full-auto
/// auto-grant is active. Matches the overlay default so automatic and manual
/// grants converge on the same budget.
pub(crate) const SESSION_LIMIT_AUTO_GRANT_INCREMENT: usize = 100;

/// Whether tool-loop and session tool-call limit increases may be granted
/// without an interactive prompt. Requires all three: the turn runs under
/// full-auto policy, `[automation.full_auto]` is enabled, and the opt-out
/// `auto_grant_tool_limits` flag is still on. Live config reloads can disable
/// `enabled` mid-session while the runtime bool stays true, so both sides
/// of the gate are checked.
#[inline]
pub(crate) fn full_auto_loop_grants_enabled(full_auto: bool, vt_cfg: Option<&VTCodeConfig>) -> bool {
    full_auto
        && vt_cfg.is_some_and(|cfg| cfg.automation.full_auto.enabled && cfg.automation.full_auto.auto_grant_tool_limits)
}
const TOOL_PREVIEW_METADATA_MAX_DEPTH: usize = 8;

const TOOL_PREVIEW_METADATA_STRING_LIMIT: usize = 512;
/// Do not materialize an arbitrarily large suppressed payload merely to
/// recover optional metadata. Normal tool responses are compacted before this
/// point; oversized or malformed payloads keep only the generic byte count.
const TOOL_PREVIEW_METADATA_PARSE_LIMIT_BYTES: usize = 128 * 1024;

/// Shared per-turn cap for auxiliary model-backed checks (failure diagnosis,
/// prompt-injection probes). These run invisibly between tool results, so a
/// tool-heavy turn must not multiply them without bound.
const AUX_MODEL_CALL_BUDGET: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnExecutionSnapshot {
    pub run_id: String,
    pub turn_id: String,
    pub phase: TurnPhase,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock_secs: u64,
    pub max_tool_retries: u32,
}

/// Tracks action patterns across turn boundaries to detect loops that span
/// multiple turns.  Constructed once per session and carried forward across
/// turns, unlike `HarnessTurnState` which is fresh each turn.
///
/// Each turn produces a "fingerprint" — a hash of the sorted set of tool
/// signatures used in that turn.  If the same fingerprint appears 2+ times
/// in the sliding window, a cross-turn loop is detected.
///
/// Also tracks consecutive turns with no execution progress to detect
/// "stuck" states where the agent only reads without making progress.
/// caps, and the streak/total at trip time) instead of `None`/zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
/// Telemetry captured when the blocked-tool-call fuse trips. Consumed by
/// `finalize_turn` to populate `TurnBlockedEvent` (`last_tool`, the enforced
pub(crate) struct BlockedToolRecoveryTelemetry {
    pub(crate) last_tool: String,
    pub(crate) consecutive_cap: usize,
    pub(crate) total_cap: usize,
    pub(crate) blocked_streak: usize,
    pub(crate) blocked_total: usize,
}

/// A tool call whose harness item the LLM runtime started while streaming,
/// keyed by provider call id. Entries are removed when the call is dispatched
/// (executed or rejected); leftovers at turn end never reached the pipeline
/// and are closed by teardown so the session log has no dangling items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamedToolCallItem {
    pub(crate) item_id: String,
    pub(crate) tool_name: String,
}

/// Identity of a failed tool call for the turn-local diagnosis memo.
/// `evidence` can be large (bounded by the diagnosis evidence cap).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DiagnosisMemoKey {
    pub tool: CompactStr,
    pub evidence: String,
}

/// Bounded diagnosis fields stored in the turn-local memo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiagnosisMemoEntry {
    pub observed: CompactStr,
    pub likely_cause: CompactStr,
    pub next_action: CompactStr,
}

pub(crate) struct HarnessTurnState {
    pub run_id: TurnRunId,
    pub turn_id: TurnId,
    pub phase: TurnPhase,
    pub turn_started_at: Instant,
    /// Time spent waiting for explicit user input or an already-running
    /// command is excluded from the ordinary per-turn harness wall-clock budget.
    wait_started_at: Option<Instant>,
    excluded_wait_duration: Duration,
    pub tool_calls: usize,
    requested_tool_calls: u32,
    admitted_tool_calls: u32,
    failed_tool_calls: u32,
    denied_tool_calls: u32,
    preflight_failures: u32,
    reused_results: u32,
    spooled_results: u32,
    raw_spooled_bytes: u64,
    model_visible_output_bytes: u64,
    model_visible_tool_preview_budget_exhausted: bool,
    suppressed_tool_previews: u32,
    /// Tool calls with a truncated preview or a legacy suppression marker.
    /// Shared identity tracking keeps diagnostics idempotent
    /// when an in-progress response is replaced by its terminal result.
    suppressed_tool_call_ids: HashSet<String>,
    recovery_activations: u32,
    pub blocked_tool_calls: usize,
    pub consecutive_blocked_tool_calls: usize,
    /// Counts consecutive malformed/schema-invalid tool calls independently
    /// from policy denials. A valid admitted call resets this streak.
    pub consecutive_preflight_failures: usize,
    /// Counts consecutive assistant *text-only* responses in this turn.
    /// Admitted tool execution resets the streak so productive progress is
    /// not mistaken for a tool-free regeneration loop. Reset every turn.
    pub consecutive_assistant_text_responses: u32,
    /// Copilot/runtime tool progress that occurs inside a provider request and
    /// therefore has no ordinary `MessageRole::Tool` entry in history.
    out_of_band_tool_progress: bool,
    /// Whether a non-empty final assistant response was rendered for this turn.
    /// This is separate from conversation history because recovery code can
    /// append a message without sending it through the user-facing renderer.
    final_response_rendered: bool,
    /// Whether the final assistant response reached the harness event stream.
    /// The stream is optional in interactive-only runs, where the state is
    /// treated as emitted once the response was rendered.
    final_response_event_emitted: bool,
    /// Whether the streaming bridge emitted assistant output for this turn.
    /// This is kept separate from the final-response flag because a streamed
    /// commentary preamble can precede tool calls and is not itself a final
    /// answer.
    streamed_response_event_emitted: bool,
    /// Whether the final response was produced by deterministic recovery
    /// fallback rather than by a successful model synthesis.
    final_response_was_fallback: bool,
    /// Whether the provider refused this turn (`FinishReason::Refusal`). A
    /// refused turn is rolled back out of model-visible history and never
    /// auto-continued, so the flag travels to the session loop.
    turn_refused: bool,
    pub consecutive_spool_chunk_reads: usize,
    pub consecutive_same_shell_command_runs: usize,
    pub last_shell_command_signature: Option<String>,
    /// Last shell command that passed the full admission gate. The repetition
    /// guard records `last_shell_command_signature` earlier, so it must not be
    /// used as evidence of execution progress by the cross-turn tracker.
    pub last_admitted_shell_command_signature: Option<String>,
    /// Failure key (`signature::err::error-signature`) of the last failed
    /// shell execution this turn. Feeds cross-turn identical-failure loop
    /// detection; reset every turn with the rest of the harness state.
    last_failed_shell_key: Option<String>,
    pub consecutive_same_file_read_family_calls: usize,
    last_file_read_family_signature: Option<String>,
    /// Per-file-path read count, independent of slice (offset/limit/raw).
    /// Catches paginated reads of the same file that the slice-aware family
    /// key lets through. Reset every turn.
    file_read_path_counts: HashMap<String, usize>,
    /// Batch validation reserves the one path-cap exception before execution.
    claimed_patch_recovery_paths: HashSet<std::path::PathBuf>,
    pub(crate) seen_successful_readonly_signatures: HashSet<String>,
    streamed_tool_call_item_ids: HashMap<String, StreamedToolCallItem>,
    /// Turn-local memo of failure diagnoses. Fix-verify loops re-hit the
    /// same failure shape; skip the extra lightweight LLM round-trip after
    /// the first diagnosis.
    failure_diagnosis_memo: HashMap<DiagnosisMemoKey, DiagnosisMemoEntry>,
    /// Cap on model-backed diagnosis calls per turn. After this, deterministic
    /// fallbacks only — failure-heavy loops must not multiply model calls.
    failure_diagnosis_model_calls: u32,
    /// Cap on model-backed prompt-injection probes per turn. Tool-heavy
    /// full-auto turns otherwise add one hidden LLM round-trip per result.
    auto_permission_probe_model_calls: u32,
    pub stop_hook_active: bool,
    pub seen_task_tracker_create_signatures: HashSet<String>,
    pub recently_written_files: HashSet<String>,
    pub tool_budget_warning_emitted: bool,
    pub tool_budget_exhausted_emitted: bool,
    /// Whether the first-notice wall-clock-exhaustion policy message has been
    /// emitted this turn. Mirrors `tool_budget_exhausted_emitted` so the full
    /// policy-violation message is sent once and subsequent rejected calls in
    /// the same batch get a compact stub instead of repeating it.
    pub wall_clock_exhausted_emitted: bool,
    /// Set when the current tool call was rejected specifically because the
    /// per-turn tool-call budget was exhausted. The Copilot adapter consumes
    /// this marker so budget rejections are not counted as permission denials.
    tool_budget_rejection_pending: bool,
    /// Set when the first wall-clock rejection fires; consumed after the tool
    /// batch by the handler to push a single "synthesize now" system directive
    /// *after* all tool responses (never interleaved between them).
    pub wall_clock_directive_pending: bool,
    /// Set when the first tool-call-budget rejection fires; consumed after the
    /// tool batch to push a single "synthesize now" system directive, mirroring
    /// `wall_clock_directive_pending`. Without this, tool-call budget
    /// exhaustion hard-broke the turn as `Blocked` with no synthesis pass, so
    /// plan mode never produced a plan (checkpoint turn_647 follow-up).
    pub tool_budget_directive_pending: bool,
    /// Set when a user grants additional session tool-call headroom. The
    /// corresponding model-facing directive is flushed after the current tool
    /// batch so it never splits an assistant tool-call/result sequence.
    session_limit_grant_directive_pending: bool,
    session_limit_granted: bool,
    /// Model-facing prompt-injection warning queued while a tool batch is
    /// executing. It is flushed after every batch result so it cannot split
    /// an assistant tool-call/result sequence on the provider wire.
    pending_auto_permission_probe_warning: Option<String>,
    pub recovery_reason: Option<String>,
    /// Reason frozen into the `[Recovery Mode]` prompt block for the current
    /// recovery activation. Updated only when a new activation starts so the
    /// system-prompt bytes stay cache-stable across recovery retries.
    recovery_prompt_reason: Option<String>,
    recovery_phase: RecoveryPhase,
    recovery_mode: Option<RecoveryMode>,
    recovery_retry_count: u8,
    /// Set for the one post-tool provider-recovery pass that must compact the
    /// older prefix before the next tool-enabled request.
    post_tool_compaction_pending: bool,
    /// Set when the provider identified the failed follow-up as a context
    /// capacity error. A failed/no-op recovery compaction must then produce a
    /// blocked handoff instead of retrying the same oversized request.
    post_tool_context_capacity_failure: bool,
    /// Set when the required recovery compaction failed or had no reducible
    /// prefix after a context-capacity rejection.
    post_tool_context_compaction_failed: bool,
    /// Explicit one-shot guard for the tool-enabled retry. This is separate
    /// from the tool-free recovery cycle counter because the two modes have
    /// different budgets and completion semantics.
    post_tool_tool_enabled_retry_used: bool,
    /// Counts how many times the post-tool follow-up failure path has
    /// scheduled a tool-free recovery pass within a single turn. Bounded by
    /// `MAX_POST_TOOL_RECOVERY_CYCLES` in the turn loop as a defense-in-depth
    /// backstop against any regression that re-triggers recovery cyclically.
    /// Resets naturally per turn because each turn constructs a fresh
    /// `HarnessTurnState`.
    post_tool_recovery_cycles: u8,
    /// Best-effort prose salvaged from a recovery synthesis response that was
    /// rejected for containing tool-call markup. Used as the final answer when
    /// all recovery retries are exhausted, instead of the canned fallback
    /// string, so gathered context is not discarded entirely.
    recovery_rejected_synthesis: Option<String>,
    /// Marks the fresh turn created by an approved plan handoff. This keeps
    /// recovery tool-enabled and lets the turn loop discard stale
    /// "tools are disabled" status responses from the planning turn.
    approved_plan_execution: bool,
    approved_plan_recovery_retries: u8,
    /// Set when the planning interview tool is permanently unavailable. The
    /// tool batch consumes this flag after all tool responses have been
    /// appended so the recovery directive is not interleaved with a batch.
    interview_denial_recovery_pending: bool,
    /// Set when the preflight validation circuit breaker trips. The tool
    /// batch consumes this flag after all tool responses (including drained
    /// skipped-call responses) have been appended, then arms a tool-free
    /// recovery pass so the model can synthesize a plain-text response
    /// instead of the turn hard-blocking and silently dropping an approved
    /// plan build.
    preflight_circuit_recovery_pending: bool,
    /// Set when the blocked-tool fuse trips. The current tool response batch
    /// drains this flag after all responses are appended, then arms one
    /// tool-free synthesis pass instead of terminating the turn immediately.
    blocked_tool_recovery_pending: bool,
    blocked_tool_recovery_reason: Option<String>,
    /// Blocked-call telemetry captured when the fuse armed or hard-broke the
    /// turn. Consumed once by `finalize_turn` for `TurnBlockedEvent` fields.
    blocked_tool_recovery_telemetry: Option<BlockedToolRecoveryTelemetry>,
    pub max_tool_calls: usize,
    pub max_tool_wall_clock: Duration,
    pub max_tool_retries: u32,
    /// Tracks consecutive relaxed continuation decisions. If this exceeds
    /// `MAX_CONSECUTIVE_RELAXED_CONTINUATIONS`, the turn ends to prevent
    /// infinite loops where the model keeps producing continuation-worthy
    /// text without making actual progress.
    pub consecutive_relaxed_continuations: u32,
    /// Last successfully observed incomplete `task_tracker` steps. Used as a
    /// fallback when the live probe fails so auto-continue is not dropped.
    incomplete_tracker_items_cache: Option<Vec<String>>,
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
    /// Whether ordinary confirmation prompts are bypassed for this turn.
    pub skip_confirmations: bool,
    /// Whether the session is operating under full-auto policy.
    pub full_auto: bool,
    pub active_agent_permissions: Option<&'a AgentPermissionsConfig>,
    /// Name of the currently active agent, if known
    pub agent_name: Option<String>,
    /// Configured execution agent used when a plan is approved from the
    /// dedicated planning agent without a previous agent to restore.
    pub default_primary_agent: Option<String>,
    /// Whether the current agent is a subagent
    pub is_subagent: bool,
}

pub(crate) struct AutoPermissionRuntimeContext<'a> {
    pub config: &'a CoreAgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub provider_client: &'a mut dyn uni::LLMProvider,
    pub working_history: &'a [uni::Message],
}

impl<'a> RunLoopContext<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
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
            false,
            false,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
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
        skip_confirmations: bool,
        full_auto: bool,
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
            skip_confirmations,
            full_auto,
            active_agent_permissions: None,
            agent_name: None,
            default_primary_agent: None,
            is_subagent: false,
        }
    }
}

pub(crate) use budget::*;
pub(crate) use cross_turn_tracker::*;

mod budget;
mod cross_turn_tracker;
mod harness_state;
use preview_bounds::*;

mod preview_bounds;

#[cfg(test)]
mod tests;
