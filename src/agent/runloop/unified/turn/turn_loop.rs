//! Agent Legibility:
//! - Entrypoint: `run_turn_loop` coordinates the per-turn request, recovery, tool execution, and completion flow.
//! - Common changes:
//!   - Main loop policy and break/continue rules stay in this root.
//!   - Post-tool recovery, usage accounting, and completion notification helpers live in `turn_loop/` support modules.
//! - Constraints: Preserve turn-phase transitions and recovery semantics when moving helpers out of the root.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode turn_loop`

use std::collections::BTreeSet;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::agent::runloop::unified::session_settings::SessionSettingsControl;
use crate::agent::runloop::welcome::SessionBootstrap;
use anyhow::Result;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::events::{tool_invocation_completed_event, tool_output_completed_event};
use vtcode_core::core::agent::runtime::RuntimeSteering;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::exec::events::{ToolCallStatus, ToolOutcome, Usage as HarnessUsage};
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry, ToolResultCache};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHeaderContext;
use vtcode_ui::tui::app::{InlineHandle, InlineSession};

use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, harness_event, turn_blocked_event, turn_completed_event, turn_completed_event_with_sessions,
    turn_failed_event, turn_started_event,
};
use crate::agent::runloop::unified::planning_workflow::maybe_handle_planning_exit_trigger;
use crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, RunLoopContext, TurnPhase};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    ToolLoopLimitAction, extract_turn_config, handle_steering_messages, initial_tool_loop_limit,
    is_stale_approved_plan_pause_response, maybe_handle_planning_enter_trigger, maybe_handle_tool_loop_limit,
    resolve_safety_tool_call_limits,
};

#[path = "turn_loop/notifications.rs"]
mod notifications;
#[path = "turn_loop/post_tool_recovery.rs"]
mod post_tool_recovery;
#[path = "turn_loop/recovery_compaction.rs"]
mod recovery_compaction;
#[path = "turn_loop/responses.rs"]
mod responses;

// Response-text surface extracted to `turn_loop/responses.rs`; re-exported so
// `result_handler`, timeline views, orchestration, `post_tool_recovery`, and
// `turn_loop/tests` keep their existing paths.
use responses::{
    APPROVED_PLAN_STALE_PAUSE_RECOVERY_DIRECTIVE, COMPLETED_TURN_NO_RESPONSE_REASON,
    MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES, PLAN_RECOVERY_EXHAUSTED_REASON, RECOVERY_TOOL_CALL_RETRY_DIRECTIVE,
    STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON, completed_recovery_fallback_reason, completed_turn_requires_final_response,
    ensure_blocked_turn_response, ensure_completed_turn_response, is_primary_agent_handoff,
    promote_latest_commentary_to_final, provider_stop_outcome,
};
pub(crate) use responses::{
    ASSISTANT_TEXT_RESPONSE_CAP_REASON, MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN, PENDING_VERIFICATION_BLOCK_REASON,
    POST_TOOL_RECOVERY_REASON, POST_TOOL_RECOVERY_REASON_PLAN_MODE, POST_TOOL_RESUME_DIRECTIVE,
    POST_TOOL_TOOL_ENABLED_RETRY_DIRECTIVE, RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE,
    UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT, budget_recovery_final_response, is_plan_entry_handoff,
};
/// Test-only response surface for `turn_loop/tests` (unused in normal builds).
#[cfg(test)]
pub(crate) use responses::{
    COMPLETED_TURN_FALLBACK_RESPONSE, PLANNING_COMPLETED_FALLBACK_RESPONSE, blocked_turn_final_response,
    completed_fallback_reason, format_blocked_turn_final_response,
};
#[path = "turn_loop/settings.rs"]
mod settings;
#[path = "turn_loop/usage_accounting.rs"]
mod usage_accounting;

// Using `tool_output_handler::handle_pipeline_output_from_turn_ctx` adapter where needed

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::{ANTI_BLIND_EDITING_DIRECTIVE, LoopTracker};
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, error_message_for_user};
use notifications::emit_turn_outcome_notification;
#[cfg(test)]
use post_tool_recovery::PostToolFailureRecovery;
#[cfg(test)]
use post_tool_recovery::maybe_recover_after_post_tool_llm_failure;
#[cfg(test)]
use post_tool_recovery::normalize_tool_free_recovery_break_outcome;
use post_tool_recovery::{
    POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON, PlanRecoveryEventContext, PostToolFailureAction,
    PostToolRecoveryContext, dispatch_post_tool_failure, ensure_post_tool_resume_directive,
    normalize_tool_free_recovery_break_outcome_with_events,
};
#[cfg(test)]
use recovery_compaction::current_turn_preserve_index;
use recovery_compaction::{RecoveryCompactionRequest, compact_before_tool_enabled_retry};
use settings::apply_pending_session_settings;
use usage_accounting::{accumulate_turn_usage, estimate_session_costs, has_turn_usage, stop_reason_from_finish_reason};
use vtcode_core::config::types::AgentConfig;
use vtcode_core::core::agent::error_recovery::ErrorType;
use vtcode_core::primary_agent::ActivePrimaryAgentState;

/// Max completion tokens for the tool-free recovery synthesis pass.
///
/// Raised from 1024 → 4096: recovery synthesis must summarize the entire
/// turn's tool outputs (often dozens of file reads and searches). 1024 tokens
/// truncated substantive answers — observed in checkpoint turn_621 where a
/// launch-time analysis over ~60 messages could not fit and the pass
/// destabilized into emitting tool-call markup instead of prose.
const RECOVERY_SYNTHESIS_MAX_TOKENS: u32 = 4096;
/// Maximum number of times the recovery pass is retried when the model
/// returns tool calls (discarded) instead of text during tool-free recovery.
///
/// Raised from 2 → 3: a single transient post-tool follow-up failure (e.g. an
/// LLM stream timeout on a large context) no longer terminates the turn and
/// forces the user to nudge "continue". The extra pass only fires when the
/// model keeps emitting tool calls during a tool-free recovery window.
const MAX_RECOVERY_RETRIES: u8 = 3;
/// Outer budget for terminating exec sessions during turn-level Exit/Cancelled
/// teardown. With `Immediate` PTY mode a healthy close is milliseconds; the cap
/// only fires on a wedged backend close. On timeout the session-end backstop
/// re-runs (2 s cap) and the OS reaps any remainder at process exit — never
/// park the fullscreen TUI waiting for a child.
const TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
/// Hard cap on consecutive assistant text-only responses. Without this, the
/// recovery / continuation logic can loop forever when the model has already
/// produced a substantive final answer but the system keeps re-prompting it
/// (e.g. tool-free recovery activates on a transient LLM stream timeout, the
/// model re-summarizes the same outline 4+ times, wasting context and tokens).
/// Admitted tool execution resets the streak. Blocked and malformed attempts
/// retain it because they are not productive progress; their dedicated
/// safeguards still bound those failure paths.
///
/// Observed in checkpoint turn_594: a simple "what functions/structs are
// Stays in the root (not `responses.rs`): turn-wide `pub(super)` visibility
// required by `result_handler` and `post_tool_recovery` cannot survive a move.
/// Reason set on `TurnLoopResult::Blocked` when the model emits tool calls or
/// textual tool-call markup during a tool-free recovery pass.  Shared between
/// `result_handler` (producer) and `post_tool_recovery` (consumer).
pub(super) const RECOVERY_CONTRACT_VIOLATION_REASON: &str =
    "Recovery mode requested a final tool-free synthesis pass, but the model attempted more tool calls.";

pub(crate) struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub turn_modified_files: BTreeSet<PathBuf>,
    pub turn_touched_files: BTreeSet<PathBuf>,
    pub turn_diagnostics: vtcode_core::core::agent::snapshots::SnapshotTurnDiagnostics,
    /// When set, the interaction loop should switch the active primary agent
    /// to this name after the turn completes.
    pub pending_primary_agent: Option<String>,
    /// Typed destination and policy selected at the plan approval boundary.
    pub pending_plan_execution_target: Option<crate::agent::runloop::unified::planning_workflow::PlanExecutionTarget>,
    /// When true, the plan was approved via the inline confirmation overlay
    /// inside the turn loop and the session loop must push an execution
    /// directive before starting the next turn so the model begins
    /// implementation immediately.
    pub plan_approved_execution_pending: bool,
    /// Whether the turn's final response came from deterministic recovery
    /// fallback rather than a confirmed model synthesis.
    pub final_response_was_fallback: bool,
    /// Whether the provider refused this turn's request. The session loop
    /// rolls a refused turn out of model-visible history and skips every
    /// auto-continue and recovery path for it.
    pub refused: bool,
}

pub(crate) fn effective_vt_cfg<'a>(
    fallback: Option<&'a VTCodeConfig>,
    live: &'a Option<&mut Option<VTCodeConfig>>,
) -> Option<&'a VTCodeConfig> {
    live.as_ref().and_then(|cfg| cfg.as_ref()).or(fallback)
}

pub(crate) struct ActiveSettingsContext<'a> {
    pub receiver: &'a mut UnboundedReceiver<SessionSettingsControl>,
    pub header_context: &'a mut InlineHeaderContext,
    pub session_bootstrap: &'a SessionBootstrap,
    pub thread_id: &'a str,
    pub thread_handle: &'a vtcode_core::core::threads::ThreadRuntimeHandle,
}

pub(crate) struct TurnLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut InlineSession,
    pub session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
    pub plan_session: &'a mut PlanningWorkflowSessionState,
    pub auto_finish_planning_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<crate::agent::runloop::unified::tool_catalog::ToolCatalogState>,
    pub ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
    pub ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
    pub safety_validator: &'a Arc<ToolCallSafetyValidator>,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery: &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub config: &'a mut AgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub live_vt_cfg: Option<&'a mut Option<VTCodeConfig>>,
    pub settings: Option<ActiveSettingsContext<'a>>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub traj: &'a TrajectoryLogger,
    pub active_primary_agent: &'a ActivePrimaryAgentState,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub runtime_steering: &'a mut RuntimeSteering,
}

impl<'a> TurnLoopContext<'a> {
    #[expect(
        clippy::too_many_arguments,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        session: &'a mut InlineSession,
        session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
        plan_session: &'a mut PlanningWorkflowSessionState,
        auto_finish_planning_attempted: &'a mut bool,
        mcp_panel_state: &'a mut mcp_events::McpPanelState,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        approval_recorder: &'a Arc<ApprovalRecorder>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        tool_registry: &'a mut ToolRegistry,
        tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_catalog: &'a Arc<crate::agent::runloop::unified::tool_catalog::ToolCatalogState>,
        ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
        ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
        context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
        last_forced_redraw: &'a mut Instant,
        input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
        lifecycle_hooks: Option<&'a LifecycleHookEngine>,
        default_placeholder: &'a Option<String>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
        safety_validator: &'a Arc<ToolCallSafetyValidator>,
        circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
        tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
        rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
        telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
        autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
        error_recovery: &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        config: &'a mut AgentConfig,
        vt_cfg: Option<&'a VTCodeConfig>,
        turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        traj: &'a TrajectoryLogger,
        active_primary_agent: &'a ActivePrimaryAgentState,
        skip_confirmations: bool,
        full_auto: bool,
        runtime_steering: &'a mut RuntimeSteering,
    ) -> Self {
        Self {
            renderer,
            handle,
            session,
            session_stats,
            plan_session,
            auto_finish_planning_attempted,
            mcp_panel_state,
            tool_result_cache,
            approval_recorder,
            decision_ledger,
            tool_registry,
            tools,
            tool_catalog,
            ctrl_c_state,
            ctrl_c_notify,
            context_manager,
            last_forced_redraw,
            input_status_state,
            lifecycle_hooks,
            default_placeholder,
            tool_permission_cache,
            permissions_state,
            safety_validator,
            circuit_breaker,
            tool_health_tracker,
            rate_limiter,
            telemetry,
            autonomous_executor,
            error_recovery,
            harness_state,
            harness_emitter,
            config,
            vt_cfg,
            live_vt_cfg: None,
            settings: None,
            turn_metadata_cache,
            provider_client,
            traj,
            active_primary_agent,
            skip_confirmations,
            full_auto,
            runtime_steering,
        }
    }

    pub(crate) fn as_run_loop_context(&mut self) -> RunLoopContext<'_> {
        let auto_permission = Some(crate::agent::runloop::unified::run_loop_context::AutoPermissionRuntimeContext {
            config: self.config,
            vt_cfg: effective_vt_cfg(self.vt_cfg, &self.live_vt_cfg),
            provider_client: self.provider_client.as_mut(),
            working_history: &[],
        });

        let mut ctx = RunLoopContext::new_with_auto_permission_context(
            self.renderer,
            self.handle,
            self.tool_registry,
            self.tools,
            self.tool_result_cache,
            self.tool_permission_cache,
            self.permissions_state,
            self.decision_ledger,
            self.session_stats,
            self.plan_session,
            self.mcp_panel_state,
            self.approval_recorder,
            self.session,
            Some(self.safety_validator),
            self.traj,
            self.harness_state,
            self.harness_emitter,
            auto_permission,
            self.skip_confirmations,
            self.full_auto,
        );
        ctx.active_agent_permissions = effective_vt_cfg(self.vt_cfg, &self.live_vt_cfg)
            .and_then(|cfg| cfg.runtime_agent_permissions.as_ref())
            .or(Some(&self.active_primary_agent.active().permissions));
        ctx.agent_name = Some(self.active_primary_agent.active().identity.name.clone());
        ctx.default_primary_agent =
            effective_vt_cfg(self.vt_cfg, &self.live_vt_cfg).map(|cfg| cfg.default_primary_agent.clone());
        // The primary agent loop is always for the primary agent, not a subagent
        ctx.is_subagent = false;
        ctx
    }

    pub(crate) fn as_turn_processing_context<'b>(
        &'b mut self,
        working_history: &'b mut Vec<uni::Message>,
    ) -> crate::agent::runloop::unified::turn::context::TurnProcessingContext<'b> {
        let tool = crate::agent::runloop::unified::turn::context::ToolContext {
            tool_result_cache: self.tool_result_cache,
            approval_recorder: self.approval_recorder,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_catalog: self.tool_catalog,
            tool_permission_cache: self.tool_permission_cache,
            permissions_state: self.permissions_state,
            safety_validator: self.safety_validator,
            circuit_breaker: self.circuit_breaker,
            tool_health_tracker: self.tool_health_tracker,
            rate_limiter: self.rate_limiter,
            telemetry: self.telemetry,
            autonomous_executor: self.autonomous_executor,
            error_recovery: self.error_recovery,
        };
        let llm = crate::agent::runloop::unified::turn::context::LLMContext {
            provider_client: self.provider_client,
            config: self.config,
            vt_cfg: effective_vt_cfg(self.vt_cfg, &self.live_vt_cfg),
            context_manager: self.context_manager,
            active_primary_agent: self.active_primary_agent,
            decision_ledger: self.decision_ledger,
            traj: self.traj,
        };
        let ui = crate::agent::runloop::unified::turn::context::UIContext {
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            active_thread_label: "main",
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state: self.input_status_state,
        };
        let state = crate::agent::runloop::unified::turn::context::TurnProcessingState {
            session_stats: self.session_stats,
            plan_session: self.plan_session,
            auto_finish_planning_attempted: self.auto_finish_planning_attempted,
            mcp_panel_state: self.mcp_panel_state,
            working_history,
            turn_metadata_cache: self.turn_metadata_cache,
            skip_confirmations: self.skip_confirmations,
            full_auto: self.full_auto,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
            runtime_steering: self.runtime_steering,
        };

        crate::agent::runloop::unified::turn::context::TurnProcessingContext::from_parts(
            crate::agent::runloop::unified::turn::context::TurnProcessingContextParts { tool, llm, ui, state },
        )
    }

    pub(crate) fn is_planning_active(&self) -> bool {
        self.tool_registry.is_planning_active()
    }

    pub(crate) fn set_phase(&mut self, phase: TurnPhase) {
        self.harness_state.set_phase(phase);
    }
}

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

pub(crate) async fn run_turn_loop(
    working_history: &mut Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingResult};
    use crate::agent::runloop::unified::turn::guards::run_proactive_guards;
    use crate::agent::runloop::unified::turn::turn_processing::{
        HandleTurnProcessingResultParams, PendingVerificationTextOutcome, execute_harness_auto_verification,
        execute_llm_request, handle_turn_processing_result, maybe_force_planning_workflow_interview,
        process_llm_response, resolve_effective_request_model,
    };

    // Approval starts implementation with usable validation state, including
    // current-context handoffs. Keep the consumed continuation budgets intact.
    if ctx.harness_state.is_approved_plan_execution() {
        ctx.session_stats.preflight_recovery = Default::default();
    }

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
    let mut turn_modified_files = BTreeSet::new();
    // Snapshot the bounded session touched window so the end-of-turn
    // checkpoint records only files touched during this turn (plus every
    // modified file below), not stale entries from previous turns.
    let touched_at_turn_start: std::collections::HashSet<String> =
        ctx.session_stats.recent_touched_files().into_iter().collect();
    let mut pending_primary_agent: Option<String> = None;
    let mut pending_plan_execution_target = None;
    *ctx.auto_finish_planning_attempted = false;

    // Compact command rows are a presentation-only active tail. A new turn
    // must start a fresh contiguous group even when the renderer is reused by
    // the session loop.
    ctx.renderer.flush_compact_command_group();
    ctx.set_phase(TurnPhase::Preparing);
    super::turn_loop_helpers::restore_fresh_turn_tool_guidance(
        working_history,
        ctx.harness_state.is_recovery_active()
            || (ctx.is_planning_active() && ctx.plan_session.recovery.research_closed),
    );
    if let Some(Err(e)) = ctx.harness_emitter.map(|e| e.emit(turn_started_event())) {
        tracing::debug!(error = %e, "harness turn_started event emission failed");
    }

    // Optimization: Extract all frequently accessed config values once
    let mut turn_config = extract_turn_config(
        effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
        ctx.is_planning_active(),
        ctx.renderer.supports_inline_ui(),
    );
    if ctx.is_planning_active() {
        ctx.plan_session.start_turn();
        // Planning work starts now: the user request exists, so promote to
        // the Planning stage and show the researching indicator once per
        // turn. Mode entry alone stays Idle to avoid a premature "Planning..."
        // footer before any request.
        crate::agent::runloop::unified::planning_workflow_state::mark_planning_turn_started(ctx.renderer, ctx.handle);
    }
    // After a permanent `request_user_input` denial, suppress the tool for the
    // rest of the session so the model stops retrying it across turns.
    if ctx.plan_session.is_interview_denied() {
        turn_config.request_user_input_enabled = false;
    }

    let mut step_count = 0;
    let mut current_max_tool_loops =
        initial_tool_loop_limit(turn_config.max_tool_loops, ctx.harness_state.is_approved_plan_execution());
    let mut turn_history_start_len = working_history.len();
    // Bounded condense-retry counter for truncated planning syntheses (see
    // the re-prompt block after generation). Prevents an oversized plan from
    // looping the turn.
    let mut plan_condense_attempts: u8 = 0;
    let mut turn_usage = HarnessUsage::default();
    // Optimization: Interned signatures with exponential backoff for loop detection
    let mut repeated_tool_attempts = LoopTracker::with_verification_snapshot(ctx.session_stats.verification_snapshot());
    if ctx.session_stats.verification_snapshot().0 {
        ctx.tool_registry.restore_completion_verification_debt();
    }
    if repeated_tool_attempts.verification_is_pending() {
        repeated_tool_attempts.verification_warning_emitted = true;
        let has_recent_verification_directive = working_history.iter().rev().take(8).any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains(ANTI_BLIND_EDITING_DIRECTIVE)
        });
        if !has_recent_verification_directive {
            working_history.push(uni::Message::system(ANTI_BLIND_EDITING_DIRECTIVE.to_string()));
        }
    }

    // Reset per-turn counters while preserving only the explicit session
    // headroom granted by the user. The validator reapplies that fuse inside
    // `set_limits`, while ordinary runtime config changes (including a lower
    // configured limit) remain authoritative when no grant exists.
    {
        let (max_per_turn, max_per_session) = resolve_safety_tool_call_limits(
            ctx.harness_state.max_tool_calls,
            turn_config.max_session_turns,
            ctx.is_planning_active(),
        );
        ctx.safety_validator.set_limits(max_per_turn, max_per_session);
        ctx.safety_validator.start_turn();
    }
    // Tracks the planning flag from the previous loop iteration so Build↔Plan
    // auto-switches mid-turn re-apply budgets in *both* directions. Raising
    // only `max(limit, floor)` is not enough: the prior mode may have already
    // consumed that cap, so the new mode needs a full floor of remaining
    // headroom from now (user report: explicit Plan works, auto-switch hits
    // the tool-call limit).
    let mut last_planning_active = ctx.is_planning_active();

    loop {
        if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state) {
            result = stop;
            break;
        }
        // Mid-turn model/effort switches must land BEFORE fresh steering input.
        // Model-switch compaction summarizes history, so a prompt steered
        // before compaction would be summarized away and the new model would
        // never see it verbatim (the TUI "Steered..." echo would then lie).
        // Applying settings first guarantees fresh input is appended after
        // compaction plus the resume note and reaches the next request verbatim.
        let history_len_before_settings = working_history.len();
        let settings_applied = apply_pending_session_settings(&mut ctx, working_history).await?;
        if settings_applied {
            // A model or effort switch changes provider capabilities, context
            // budget, and prompt shaping for the request about to be built.
            // Re-derive the cached turn config so the next request uses the
            // effective settings; the in-flight request already finished with
            // its original settings.
            turn_config = extract_turn_config(
                effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
                ctx.is_planning_active(),
                ctx.renderer.supports_inline_ui(),
            );
            // Compaction may have shrunk history and injected a resume note;
            // rebase the turn baseline so later slicing (final response,
            // verification, TUI) cannot index into compacted-away messages.
            turn_history_start_len = turn_history_start_len.min(working_history.len());
            tracing::info!(
                history_before = history_len_before_settings,
                history_after = working_history.len(),
                turn_history_start_len,
                "Rebased turn baseline after mid-turn settings/model switch"
            );
        }
        if handle_steering_messages(&mut ctx, working_history, &mut result).await? {
            break;
        }
        if ctx.session_stats.navigation_recovery.blocked() {
            result = TurnLoopResult::Blocked {
                reason: Some(crate::agent::runloop::unified::state::NAVIGATION_RECOVERY_BLOCK_REASON.to_string()),
            };
            break;
        }
        if settings_applied {
            // Double-check the new model receives the updated prompt verbatim
            // before the next LLM request is built: fresh steered input must
            // sit at the tail after compaction plus resume, not summarized away.
            verify_mid_turn_prompt_received(working_history, turn_history_start_len);
        }

        // A permanent interview denial can happen after this turn's initial
        // config snapshot (for example when the model's plan response
        // triggers the inline interview). Keep the parser and request builder
        // on the same capability set immediately; otherwise this turn can
        // still inject another `request_user_input` call even though the
        // catalog has already removed it.
        if ctx.plan_session.is_interview_denied() {
            turn_config.request_user_input_enabled = false;
        }

        step_count += 1;
        ctx.telemetry.record_turn();

        if maybe_handle_planning_enter_trigger(&mut ctx, working_history, step_count, &mut result).await? {
            break;
        }

        // Build↔Plan auto-switch mid-turn: re-derive turn config and grant a
        // full *remaining* mode floor so the new phase is not starved by the
        // prior phase's consumption (checkpoint turn_804 + user Build→Plan
        // tool-call-limit report).
        let planning_now = ctx.is_planning_active();
        if planning_now != last_planning_active {
            last_planning_active = planning_now;
            if planning_now {
                ctx.plan_session.start_turn();
                // Planning just became active inside a running turn: promote to
                // the stage and show the researching row once, since work is live.
                crate::agent::runloop::unified::planning_workflow_state::mark_planning_turn_started(
                    ctx.renderer,
                    ctx.handle,
                );
            }
            turn_config = extract_turn_config(
                effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
                planning_now,
                ctx.renderer.supports_inline_ui(),
            );
            if ctx.plan_session.is_interview_denied() {
                turn_config.request_user_input_enabled = false;
            }
            super::turn_loop_helpers::apply_mode_switch_remaining_tool_call_floor(
                &mut ctx.harness_state.max_tool_calls,
                ctx.harness_state.tool_calls,
                planning_now,
            );
            super::turn_loop_helpers::apply_mode_switch_remaining_tool_loop_floor(
                &mut current_max_tool_loops,
                step_count,
                planning_now,
            );
            let (max_per_turn, max_per_session) = resolve_safety_tool_call_limits(
                ctx.harness_state.max_tool_calls,
                turn_config.max_session_turns,
                planning_now,
            );
            ctx.safety_validator.set_limits(max_per_turn, max_per_session);
        }

        // The settings application and Build↔Plan mode switch above are the
        // only points that can change the request identity mid-iteration:
        // nothing after this line mutates `config.model`, the active primary
        // agent, or the live config, so derive the effective model, context
        // budget, and provider identity once and reuse them for the planning
        // exit, budget check, compaction, and telemetry below. `vt_cfg` is
        // re-read where needed: it borrows `ctx`, which is taken `&mut` by
        // the tool-loop-limit handler and request-context construction.
        let active_model = resolve_effective_request_model(&ctx.config.model, ctx.active_primary_agent.active());
        let context_budget = vtcode_core::compaction::effective_context_budget(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            ctx.provider_client.as_ref(),
            &active_model,
        );
        let provider_name = ctx.provider_client.name().to_string();

        let transition = maybe_handle_planning_exit_trigger(
            ctx.renderer,
            ctx.tool_registry,
            ctx.plan_session,
            ctx.handle,
            working_history,
            ctx.auto_finish_planning_attempted,
            crate::agent::runloop::unified::planning_workflow::PlanningExitContext {
                session: ctx.session,
                ctrl_c_state: ctx.ctrl_c_state,
                ctrl_c_notify: ctx.ctrl_c_notify,
                vt_cfg: effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
                skip_confirmations: ctx.skip_confirmations,
                full_auto: ctx.full_auto,
                context_usage_percent: ctx.context_manager.context_usage_percent(context_budget),
                telemetry: crate::agent::runloop::unified::planning_workflow::PlanApprovalTelemetryContext {
                    emitter: ctx.harness_emitter,
                    thread_id: &ctx.harness_state.run_id.0,
                    turn_id: &ctx.harness_state.turn_id.0,
                },
            },
        )
        .await?;

        if transition.should_break() {
            let (loop_result, target) = transition.into_result_and_target();
            result = loop_result;
            pending_plan_execution_target = target;
            break;
        }

        match maybe_handle_tool_loop_limit(&mut ctx, step_count, &mut current_max_tool_loops).await? {
            ToolLoopLimitAction::Proceed => {}
            ToolLoopLimitAction::ContinueLoop => continue,
            ToolLoopLimitAction::BreakLoop => {
                result = TurnLoopResult::Blocked {
                    reason: Some(format!(
                        "Tool loop budget exhausted ({step_count}/{current_max_tool_loops}); no additional synthesis pass is available. The current plan and task state were retained."
                    )),
                };
                break;
            }
        }

        // Shared config view for the budget check and compaction decisions
        // below; the borrow ends before the request context is constructed.
        let vt_cfg = effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg);
        // A configured monetary budget is an enforcement contract. Validate
        // pricing before any compaction path because native compaction can
        // itself dispatch a provider request.
        if let Some(max_budget_usd) = vt_cfg.and_then(|cfg| cfg.agent.harness.max_budget_usd)
            && let Err(error) = vtcode_core::llm::usage_cost::require_budget_pricing(
                ctx.provider_client.name(),
                &active_model,
                Some(max_budget_usd),
            )
        {
            result = TurnLoopResult::Blocked { reason: Some(error.to_string()) };
            break;
        }
        let harness_snapshot = ctx.tool_registry.harness_context_snapshot();
        let steering_update = vtcode_core::compaction::memory_envelope::SessionMemoryEnvelopeUpdate {
            pending_intents: Some(ctx.runtime_steering.pending_follow_up_intents_snapshot()),
            applied_intent_ids: ctx.runtime_steering.applied_follow_up_intent_ids().iter().cloned().collect(),
            ..Default::default()
        };
        let recovery_compaction_requested = ctx.harness_state.take_post_tool_compaction_pending();
        if recovery_compaction_requested {
            let context_capacity_failure = ctx.harness_state.post_tool_context_capacity_failure();
            // Async UI stays in sync: spinner ticks while the engine compacts the
            // older prefix; the final line is rendered from the engine outcome.
            let progress = crate::agent::runloop::unified::turn::compaction::CompactionProgressGuard::start(
                ctx.handle,
                ctx.input_status_state,
            );
            let recovery_result = compact_before_tool_enabled_retry(RecoveryCompactionRequest {
                history: working_history,
                turn_history_start_len: &mut turn_history_start_len,
                compaction_context: crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                    ctx.provider_client.as_ref(),
                    &active_model,
                    &harness_snapshot.session_id,
                    &ctx.harness_state.run_id.0,
                    &ctx.config.workspace,
                    vt_cfg,
                    ctx.lifecycle_hooks,
                    ctx.harness_emitter,
                ),
                session_stats: ctx.session_stats,
                context_manager: ctx.context_manager,
                steering_update,
            })
            .await;
            let recovery_elapsed = progress.finish();
            match recovery_result {
                Ok(Some(outcome)) => {
                    repeated_tool_attempts.clear_navigation_evidence();
                    let label = crate::agent::runloop::unified::turn::compaction::format_compacted_summary(
                        outcome.original_len,
                        outcome.compacted_len,
                        outcome.mode.as_str(),
                        recovery_elapsed,
                    );
                    let _ = ctx.renderer.line(MessageStyle::Info, &label);
                }
                Ok(None) if context_capacity_failure => {
                    ctx.harness_state.mark_post_tool_context_compaction_failed();
                    ensure_post_tool_resume_directive(working_history);
                    result = TurnLoopResult::Blocked {
                        reason: Some(POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()),
                    };
                    break;
                }
                Ok(None) => {}
                Err(err) => {
                    if context_capacity_failure {
                        ctx.harness_state.mark_post_tool_context_compaction_failed();
                        ensure_post_tool_resume_directive(working_history);
                        result = TurnLoopResult::Blocked {
                            reason: Some(POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON.to_string()),
                        };
                        break;
                    }
                    tracing::warn!(
                        error = %crate::agent::runloop::unified::turn::compaction::format_compaction_error_details(&err),
                        "Post-tool recovery compaction failed; preserving the existing history for the bounded retry"
                    );
                    let prefix = crate::agent::runloop::unified::turn::compaction::format_compaction_failed(
                        recovery_elapsed,
                        &err,
                    );
                    let _ = ctx.renderer.line(
                        MessageStyle::Warning,
                        &format!("{prefix} Continuing with full history for the bounded retry."),
                    );
                }
            }
        } else {
            tracing::info!(
                model = %active_model,
                context_budget,
                prompt_tokens = ctx.context_manager.current_token_usage(),
                "Resolved per-turn context budget denominator"
            );
            // Async UI: only show the spinner when compaction will actually run.
            // `maybe_auto_compact_history` returns `None` fast below threshold,
            // when disabled, or when suppressed; starting a spinner
            // unconditionally would flicker every turn.
            let auto_start = Instant::now();
            let auto_compaction_allowed = vt_cfg.is_some_and(|cfg| cfg.agent.harness.auto_compaction_enabled)
                && ctx.session_stats.auto_compact_suppressed == vtcode_core::compaction::SUPPRESS_NONE;
            let auto_threshold = crate::agent::runloop::unified::turn::compaction::effective_compaction_threshold(
                vt_cfg,
                ctx.provider_client.as_ref(),
                &active_model,
            );
            let auto_likely = auto_compaction_allowed
                && (ctx.context_manager.compaction_pending()
                    || auto_threshold.is_some_and(|threshold| ctx.context_manager.current_token_usage() >= threshold));
            let auto_progress = auto_likely.then(|| {
                crate::agent::runloop::unified::turn::compaction::CompactionProgressGuard::start(
                    ctx.handle,
                    ctx.input_status_state,
                )
            });
            let auto_result = crate::agent::runloop::unified::turn::compaction::maybe_auto_compact_history(
                crate::agent::runloop::unified::turn::compaction::CompactionContext::new(
                    ctx.provider_client.as_ref(),
                    &active_model,
                    &harness_snapshot.session_id,
                    &ctx.harness_state.run_id.0,
                    &ctx.config.workspace,
                    vt_cfg,
                    ctx.lifecycle_hooks,
                    ctx.harness_emitter,
                ),
                crate::agent::runloop::unified::turn::compaction::CompactionState::new(
                    working_history,
                    ctx.session_stats,
                    ctx.context_manager,
                )
                .with_steering_update(steering_update),
            )
            .await;
            let auto_elapsed = auto_progress
                .map(|guard| guard.finish())
                .unwrap_or_else(|| auto_start.elapsed());
            match auto_result {
                Ok(Some(outcome)) => {
                    repeated_tool_attempts.clear_navigation_evidence();
                    turn_history_start_len = outcome.compacted_len;
                    tracing::info!(
                        original_len = outcome.original_len,
                        compacted_len = outcome.compacted_len,
                        turn_history_start_len,
                        "Applied local fallback compaction before the next turn request"
                    );
                    let label = crate::agent::runloop::unified::turn::compaction::format_compacted_summary(
                        outcome.original_len,
                        outcome.compacted_len,
                        outcome.mode.as_str(),
                        auto_elapsed,
                    );
                    let _ = ctx.renderer.line(MessageStyle::Info, &label);
                    // Continuation: do not break. The loop falls through to the
                    // next LLM request using the compacted handoff (summary +
                    // envelope + continuity tail) so the task resumes.
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "Local fallback compaction failed");
                }
            }
        }

        // Clone validation cache arc to avoid borrow conflict
        let validation_cache = ctx.session_stats.validation_cache.clone();

        // Anti-runaway guard: if the model has already emitted
        // MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN consecutive text-only
        // responses without admitted tool progress, the recovery / continuation
        // loop is regenerating the same answer. Conclude the turn with the
        // last response stored in working_history and emit a warning so users
        // can see what happened. See MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN for
        // the observed failure mode (checkpoint turn_594: 4 identical
        // 6500-token outline responses, ~90s of wasted context).
        // Harness state is authoritative. History cannot represent Copilot's
        // inline VTCode tool calls, while compaction does not discard this
        // turn-scoped counter.
        let text_response_streak = ctx.harness_state.consecutive_assistant_text_responses;
        // A pending bounded planning retry (validation repair, denied-interview
        // synthesis, pseudo-tool-call reprompt) is a deliberate extra candidate,
        // so allow the next request through the generic text-response cap. This
        // allowance is queued independently of the response count because a
        // retry can be scheduled after an ordinary planning response has
        // already used that budget.
        let bounded_planning_follow_up = ctx.is_planning_active()
            && (ctx.plan_session.bounded_planning_follow_up_allowed() || ctx.plan_session.recovery.research_closed);
        if text_response_streak >= MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN && !bounded_planning_follow_up {
            tracing::warn!(
                text_response_streak,
                cap = MAX_ASSISTANT_TEXT_RESPONSES_PER_TURN,
                "Assistant text-response cap reached; ending turn to prevent runaway regeneration loop"
            );
            let _ = ctx.renderer.line(
                MessageStyle::Warning,
                "Recovery loop detected: capping repeated assistant responses to avoid wasted context.",
            );
            let promoted_final = promote_latest_commentary_to_final(working_history, turn_history_start_len);
            if promoted_final {
                // Commentary has already crossed the renderer surface. Avoid
                // rendering it a second time while still allowing the normal
                // blocked-turn finalizer to publish the canonical event.
                ctx.harness_state.mark_final_response_rendered();
                if ctx.harness_state.streamed_response_event_emitted() {
                    // The streaming lifecycle already emitted the assistant
                    // item; publishing another AgentMessage would duplicate it.
                    ctx.harness_state.mark_final_response_event_emitted();
                }
            }
            // A preserved final answer with a clear verification gate is a
            // completed turn: the model concluded and the loop merely
            // re-prompted past its answer. Blocked/handoff semantics stay
            // reserved for unverified work (checkpoint session-vtcode-20260912
            // T083718Z: a verified recap was continued into the cap and the
            // finished turn was published as Blocked).
            let cap_ends_completed =
                promoted_final && !ctx.tool_registry.completion_verification_pending() && !ctx.is_planning_active();
            if cap_ends_completed {
                result = TurnLoopResult::Completed { plan_approved_execution_pending: false };
            } else {
                result = TurnLoopResult::Blocked {
                    reason: Some(ASSISTANT_TEXT_RESPONSE_CAP_REASON.to_string()),
                };
            }
            break;
        }
        if bounded_planning_follow_up {
            ctx.plan_session.consume_bounded_planning_follow_up();
        }

        // Prepare turn processing context
        // Capture input status state before constructing the context, which
        // mutably borrows the turn loop context for the request lifetime.
        let restore_status_left = ctx.input_status_state.left.clone();
        let restore_status_right = ctx.input_status_state.right.clone();
        let mut turn_processing_ctx = ctx.as_turn_processing_context(working_history);

        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        run_proactive_guards(&mut turn_processing_ctx, step_count).await?;

        match turn_processing_ctx.admit_planning_recovery_request(&active_model).await {
            Ok(true) => {}
            Ok(false) => {
                match turn_processing_ctx
                    .await_planning_recovery_input("The planning recovery allowance is exhausted.")
                    .await?
                {
                    TurnHandlerOutcome::Continue => continue,
                    TurnHandlerOutcome::Break(outcome) => {
                        result = outcome;
                        break;
                    }
                    _ => {
                        result = TurnLoopResult::Blocked { reason: Some("planning awaiting input".into()) };
                        break;
                    }
                }
            }
            Err(error) => {
                turn_processing_ctx.plan_session.recovery.outcome =
                    vtcode_core::exec::events::planning::PlanningOutcome::AwaitingInput;
                turn_processing_ctx.renderer.line(
                    MessageStyle::Error,
                    &format!(
                        "Planning could not save recovery state. Retry after fixing persistence. Details: {error}"
                    ),
                )?;
                result = TurnLoopResult::Blocked {
                    reason: Some("planning persistence failed; awaiting input".into()),
                };
                break;
            }
        }

        // Execute the LLM request
        turn_processing_ctx.set_phase(TurnPhase::Requesting);
        // `active_model` is the per-iteration resolution from above; the
        // proactive guards between there and here cannot change the model.
        let recovery_pass = turn_processing_ctx.consume_recovery_pass();

        let tool_free_recovery = recovery_pass && turn_processing_ctx.recovery_is_tool_free();
        // Capture input status state for potential restoration after a
        // pre-request block or provider failure.
        // Cache-gap advisory (Phase E1): warn once per gap when the user
        // paused long enough for the provider prompt cache to have expired,
        // so this request may unexpectedly re-pay full input cost.
        if let Some(threshold) = turn_processing_ctx.vt_cfg.and_then(|cfg| {
            cfg.prompt_cache.gap_threshold_secs_for_route(
                &provider_name,
                &active_model,
                turn_processing_ctx.provider_client.requested_prompt_cache_ttl(&active_model),
            )
        }) {
            let threshold = Duration::from_secs(threshold);
            if turn_processing_ctx.session_stats.total_usage().cached_input_tokens > 0
                && let Some(elapsed) = turn_processing_ctx.session_stats.cache_gap_exceeds(threshold)
            {
                let _ = turn_processing_ctx.renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "~{} since the last request; the provider prompt cache may have expired, so this request may rebuild cached input.",
                        vtcode_core::llm::request_gap::format_gap(elapsed)
                    ),
                );
            }
        }
        turn_processing_ctx.session_stats.note_request_sent();

        let suppress_unverified_output = turn_processing_ctx.tool_registry.completion_verification_pending();
        let request_result = if suppress_unverified_output
            || turn_processing_ctx
                .config
                .provider
                .eq_ignore_ascii_case(vtcode_core::copilot::COPILOT_PROVIDER_KEY)
        {
            turn_processing_ctx
                .execute_llm_request_with_options(
                    step_count,
                    &active_model,
                    tool_free_recovery.then_some(RECOVERY_SYNTHESIS_MAX_TOKENS),
                    tool_free_recovery,
                    None, // parallel_cfg_opt
                    suppress_unverified_output,
                    Some(&mut repeated_tool_attempts),
                )
                .await
        } else {
            execute_llm_request(
                &mut turn_processing_ctx,
                step_count,
                &active_model,
                tool_free_recovery.then_some(RECOVERY_SYNTHESIS_MAX_TOKENS),
                tool_free_recovery,
                None, // parallel_cfg_opt
            )
            .await
        };
        let (response, response_streamed) = match request_result {
            Ok(val) => val,
            Err(err) => {
                if let Some(stop) = provider_stop_outcome(turn_processing_ctx.ctrl_c_state, &err) {
                    turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);
                    result = stop;
                    break;
                }
                if err
                    .downcast_ref::<vtcode_core::llm::reasoning_effort::ReasoningEffortUnsupported>()
                    .is_some()
                {
                    turn_processing_ctx.restore_input_status(restore_status_left.clone(), restore_status_right.clone());
                    result = TurnLoopResult::Blocked { reason: Some(err.to_string()) };
                    break;
                }
                // Record the error in the recovery state for diagnostics
                turn_processing_ctx
                    .record_recovery_error("llm_request", &err, ErrorType::Other)
                    .await;

                // execute_llm_request already performs retry/backoff for retryable provider errors.
                // Avoid a second retry layer here, which can consume turn budget and cause timeouts.
                // Restore input status on request failure to clear loading/shimmer state.
                turn_processing_ctx.restore_input_status(restore_status_left.clone(), restore_status_right.clone());

                let planning = turn_processing_ctx.is_planning_active();
                if planning && turn_processing_ctx.plan_session.recovery.research_closed {
                    turn_processing_ctx.plan_session.recovery.response_class =
                        Some(vtcode_core::exec::events::planning::PlanningResponseClass::ProviderFailure);
                    turn_processing_ctx.persist_planning_recovery().await?;
                    match turn_processing_ctx
                        .recover_planning_failure(&error_message_for_user(&err), None)
                        .await?
                    {
                        TurnHandlerOutcome::Continue => continue,
                        TurnHandlerOutcome::Break(outcome) => {
                            result = outcome;
                            break;
                        }
                        _ => {}
                    }
                }
                match dispatch_post_tool_failure(PostToolRecoveryContext {
                    renderer: &mut *turn_processing_ctx.renderer,
                    working_history: &mut *turn_processing_ctx.working_history,
                    harness_state: &mut *turn_processing_ctx.harness_state,
                    harness_emitter: turn_processing_ctx.harness_emitter,
                    plan_session: planning.then_some(&mut *turn_processing_ctx.plan_session),
                    plan_state: planning.then_some(&turn_processing_ctx.tool_registry.planning_workflow_state()),
                    err: &err,
                    step_count,
                    turn_history_start_len,
                    stage: "execute_llm_request",
                    tool_free_recovery,
                })
                .await?
                {
                    PostToolFailureAction::Continue => continue,
                    PostToolFailureAction::Break(r) => {
                        result = r;
                        break;
                    }
                    PostToolFailureAction::Fallthrough => {}
                }

                display_error(turn_processing_ctx.renderer, "LLM request failed", &err)?;
                // Show recovery hints derived from the canonical error category
                {
                    let err_cat = vtcode_commons::classify_anyhow_error(&err);
                    if matches!(err_cat, vtcode_commons::ErrorCategory::Authentication) {
                        // For auth errors, show actionable provider-specific guidance
                        // that distinguishes "no key stored" from "stored key rejected".
                        let (provider_label, provider_key, is_managed_auth, has_stored_credential) =
                            turn_processing_ctx
                                .config
                                .provider
                                .parse::<vtcode_core::config::models::Provider>()
                                .map(|p| {
                                    let storage_mode = turn_processing_ctx
                                        .vt_cfg
                                        .map(|cfg| cfg.agent.credential_storage_mode)
                                        .unwrap_or_default();
                                    let has =
                                        vtcode_config::api_keys::provider_credential_detail_with_mode(p, storage_mode)
                                            .is_some();
                                    (p.label().to_string(), p.as_ref().to_string(), p.uses_managed_auth(), has)
                                })
                                .unwrap_or_else(|_| {
                                    (
                                        turn_processing_ctx.config.provider.clone(),
                                        turn_processing_ctx.config.provider.clone(),
                                        false,
                                        false,
                                    )
                                });
                        let guidance = err_cat.auth_recovery_guidance(
                            &provider_label,
                            &provider_key,
                            is_managed_auth,
                            has_stored_credential,
                        );
                        for line in &guidance {
                            turn_processing_ctx.renderer.line(MessageStyle::Info, line)?;
                        }
                    } else {
                        let suggestions = err_cat.recovery_suggestions();
                        if !suggestions.is_empty() {
                            let hint = suggestions.join("; ");
                            turn_processing_ctx
                                .renderer
                                .line(MessageStyle::Info, &format!("Hint: {hint}"))?;
                        }
                    }
                }
                // Log error via tracing instead of polluting conversation history
                // Adding error messages as assistant content can poison future turns
                let error_message = error_message_for_user(&err);
                tracing::error!(error = %error_message, step = step_count, "LLM request failed");
                // Do NOT add error message to working_history - this prevents the model
                // from learning spurious error patterns and keeps the conversation clean
                result = TurnLoopResult::Aborted;
                break;
            }
        };

        turn_processing_ctx.record_planning_response(&response).await?;

        // Track turn usage and context pressure before later processing borrows `response`.
        let response_usage = response.usage.clone();
        // Usage normalization and pricing must follow the provider instance
        // that will serve the request. Configuration may contain an alias or
        // an inferred route, while the provider trait is authoritative. The
        // per-iteration `provider_name` binding is that provider's identity.
        accumulate_turn_usage(&provider_name, &mut turn_usage, &response_usage);
        turn_processing_ctx.session_stats.record_usage(&provider_name, &response_usage);
        // Record prompt pressure immediately so mid-turn readers (e.g. the
        // plan-approval modal's `Context: N% used` subtitle) see the current
        // response's prompt tokens instead of the previous turn's stale value.
        // The later `update_token_usage` before continue/break is idempotent.
        turn_processing_ctx.context_manager.update_token_usage(&response_usage);
        // SEV-style prompt-cache health: a sustained hit-rate collapse warns
        // once per session instead of silently re-paying full input cost.
        if let Some(message) = turn_processing_ctx
            .session_stats
            .record_cache_turn_health(&provider_name, &response_usage)
        {
            tracing::warn!("{message}");
            let _ = turn_processing_ctx.renderer.line(MessageStyle::Warning, &message);
        }
        turn_processing_ctx
            .session_stats
            .set_stop_reason(Some(stop_reason_from_finish_reason(&response.finish_reason)));
        let max_budget_usd = turn_processing_ctx.vt_cfg.and_then(|cfg| cfg.agent.harness.max_budget_usd);
        let turn_cost = response_usage.as_ref().and_then(|usage| {
            let normalized = vtcode_core::llm::usage_cost::normalized_turn_usage(&provider_name, usage);
            estimate_session_costs(&provider_name, &active_model, &normalized)
        });
        match turn_processing_ctx.session_stats.record_cost(turn_cost) {
            Some(estimate) => {
                // Keep the conservative raw figure for enforcement while the
                // cache-aware effective figure is shown to users.
                let threshold = turn_processing_ctx
                    .vt_cfg
                    .map(|cfg| cfg.agent.harness.budget_warning_threshold)
                    .unwrap_or(vtcode_core::llm::usage_cost::DEFAULT_BUDGET_WARNING_RATIO);
                match vtcode_core::llm::usage_cost::BudgetStatus::classify(estimate.raw_usd, max_budget_usd, threshold)
                {
                    vtcode_core::llm::usage_cost::BudgetStatus::Exceeded { max, .. } => {
                        turn_processing_ctx
                            .session_stats
                            .mark_budget_limit_reached(max, estimate.raw_usd);
                        turn_processing_ctx.context_manager.update_token_usage(&response_usage);
                        #[cfg(debug_assertions)]
                        turn_processing_ctx.context_manager.validate_token_tracking(&response_usage);
                        // In planning mode, preserve the session and use the same
                        // user-facing recovery fallback as post-tool failures.
                        // Returning a model-only directive here would leave no
                        // synthesis call to consume it and would dead-end the turn.
                        if turn_processing_ctx.is_planning_active() {
                            turn_processing_ctx.plan_session.mark_budget_exhausted();
                            let plan_state = turn_processing_ctx.tool_registry.planning_workflow_state();
                            let event_thread_id = turn_processing_ctx.harness_state.run_id.0.clone();
                            let event_turn_id = turn_processing_ctx.harness_state.turn_id.0.clone();
                            let event_context = PlanRecoveryEventContext {
                                emitter: turn_processing_ctx.harness_emitter,
                                thread_id: &event_thread_id,
                                turn_id: &event_turn_id,
                            };
                            result = post_tool_recovery::complete_turn_after_failed_tool_free_recovery_with_events(
                                turn_processing_ctx.working_history,
                                "turn_loop.budget_limit_planning",
                                None,
                                None,
                                Some(turn_processing_ctx.plan_session),
                                Some(&plan_state),
                                Some(event_context),
                                Some(&mut *turn_processing_ctx.harness_state),
                            )
                            .await;
                            let _ = turn_processing_ctx.renderer.line(
                                MessageStyle::Warning,
                                "Budget exhausted during planning workflow. The plan draft is preserved for approval or revision.",
                            );
                        } else {
                            result = TurnLoopResult::Blocked {
                                reason: Some(format!(
                                    "Stopped after reaching budget limit (max: ${max:.4}, spent: ${:.4}, cache-adjusted: ${:.4}).",
                                    estimate.raw_usd, estimate.effective_usd
                                )),
                            };
                        }
                        break;
                    }
                    vtcode_core::llm::usage_cost::BudgetStatus::Warning { max, .. }
                        if !turn_processing_ctx.session_stats.budget_warning_emitted() =>
                    {
                        turn_processing_ctx.session_stats.mark_budget_warning_emitted();
                        let _ = turn_processing_ctx.renderer.line(
                            MessageStyle::Info,
                            &format!(
                                "Session cost ${:.4} has reached {:.0}% of the ${max:.2} budget. {}",
                                estimate.raw_usd,
                                threshold * 100.0,
                                turn_processing_ctx.session_stats.total_usage().cache_summary(),
                            ),
                        );
                    }
                    _ => {}
                }
            }
            None => {
                if max_budget_usd.is_some() {
                    let reason = format!(
                        "Cannot enforce session USD budget for `{provider_name}/{active_model}`: complete valid pricing metadata is unavailable"
                    );
                    result = TurnLoopResult::Blocked { reason: Some(reason) };
                    break;
                }
            }
        }
        if !response.tool_references.is_empty() {
            turn_processing_ctx
                .tool_catalog
                .note_tool_references(turn_processing_ctx.tools, &response.tool_references)
                .await;
        }

        {
            if turn_processing_ctx.is_planning_active() {
                turn_processing_ctx.plan_session.increment_turns();
            }
        }

        // Plan-mode robustness: if the planning synthesis was truncated at the
        // model's output token limit, the emitted plan is incomplete ("cut off
        // mid-flight"). Rather than accept a partial plan or re-enter the
        // recovery path (which previously looped forever), ask for a tighter
        // spec and retry once. Bounded so a genuinely oversized plan cannot
        // loop. The policy lives in the planning-workflow facade so this loop
        // stays free of plan-mode specifics.
        let planning_active = turn_processing_ctx.is_planning_active();
        if crate::agent::runloop::unified::planning_workflow::maybe_condense_truncated_plan(
            &mut *turn_processing_ctx.working_history,
            &mut *turn_processing_ctx.renderer,
            planning_active,
            tool_free_recovery,
            &mut plan_condense_attempts,
            &response,
        ) {
            continue;
        }

        // A successful provider response may race the user's stop. Never admit
        // its tools or planning continuation after that stop is accepted.
        if let Some(stop) =
            crate::agent::runloop::unified::stop_requests::stop_outcome(turn_processing_ctx.ctrl_c_state)
        {
            result = stop;
            break;
        }
        // Process the LLM response
        let processing_result_outcome = {
            let allow_plan_interview = turn_processing_ctx.is_planning_active()
                && turn_config.request_user_input_enabled
                && crate::agent::runloop::unified::turn::turn_processing::planning_workflow_interview_ready(
                    turn_processing_ctx.session_stats,
                    turn_processing_ctx.plan_session,
                );
            process_llm_response(
                &response,
                turn_processing_ctx.renderer,
                turn_processing_ctx.working_history.len(),
                turn_processing_ctx.is_planning_active(),
                allow_plan_interview,
                turn_config.request_user_input_enabled,
                !tool_free_recovery,
                Some(&validation_cache),
                Some(turn_processing_ctx.tool_registry),
            )
        };
        let mut processing_result = match processing_result_outcome {
            Ok(result) => result,
            Err(err) => {
                let err_cat = vtcode_commons::classify_anyhow_error(&err);
                if err_cat.is_retryable() {
                    tracing::warn!(
                        error = %err,
                        step = step_count,
                        category = ?err_cat,
                        "Response parse failed with transient error; skipping extra request retry"
                    );
                }

                {
                    let mut recovery = turn_processing_ctx.error_recovery.write().await;
                    recovery.record_error("llm_response_parse", format!("{err:#}"), ErrorType::Other);
                }
                let tool_free_recovery =
                    turn_processing_ctx.recovery_pass_used() && turn_processing_ctx.recovery_is_tool_free();
                let planning = turn_processing_ctx.is_planning_active();

                // Restore the input status/UI before dispatching recovery handling
                // so the bottom-line is not left in a loading state if recovery
                // ultimately ends the turn or returns an error.
                turn_processing_ctx.restore_input_status(restore_status_left.clone(), restore_status_right.clone());

                match dispatch_post_tool_failure(PostToolRecoveryContext {
                    renderer: &mut *turn_processing_ctx.renderer,
                    working_history: &mut *turn_processing_ctx.working_history,
                    harness_state: &mut *turn_processing_ctx.harness_state,
                    harness_emitter: turn_processing_ctx.harness_emitter,
                    plan_session: planning.then_some(&mut *turn_processing_ctx.plan_session),
                    plan_state: planning.then_some(&turn_processing_ctx.tool_registry.planning_workflow_state()),
                    err: &err,
                    step_count,
                    turn_history_start_len,
                    stage: "process_llm_response",
                    tool_free_recovery,
                })
                .await?
                {
                    PostToolFailureAction::Continue => continue,
                    PostToolFailureAction::Break(r) => {
                        result = r;
                        break;
                    }
                    PostToolFailureAction::Fallthrough => {}
                }
                return Err(err);
            }
        };
        // When in tool-free recovery and the model returns no text (e.g. producing
        // tool calls that get discarded), retry with a more explicit directive
        // rather than immediately falling back to the deterministic final answer.
        if tool_free_recovery
            && !turn_processing_ctx.is_planning_active()
            && matches!(processing_result, TurnProcessingResult::Empty { .. })
            && response.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty())
            && turn_processing_ctx.recovery_retry_count() < MAX_RECOVERY_RETRIES
        {
            let directive = if turn_processing_ctx.is_planning_active() {
                RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE
            } else {
                RECOVERY_TOOL_CALL_RETRY_DIRECTIVE
            };
            turn_processing_ctx
                .working_history
                .push(uni::Message::system(directive.to_string()));
            turn_processing_ctx.retry_recovery_pass();
            continue;
        }
        // During the tool-free recovery pass, tools are disabled at the API
        // level, so an injected `request_user_input` interview call can never
        // be executed — it only trips the recovery contract guard and collapses
        // the turn to a dead-end fallback. Skip interview synthesis/forcing
        // here; the recovery synthesis can still produce a valid text answer.
        if turn_config.request_user_input_enabled && !tool_free_recovery && turn_processing_ctx.is_planning_active() {
            processing_result = maybe_force_planning_workflow_interview(
                processing_result,
                response.content.as_deref(),
                turn_processing_ctx.session_stats,
                turn_processing_ctx.plan_session,
                turn_processing_ctx.working_history.len(),
            );
        }

        // A recovery directive from the planning turn can leak into the first
        // approved-plan build request. If the model echoes that stale state as
        // its answer, do not commit it to the transcript or end execution;
        // give the fresh write-capable turn a bounded, explicit retry instead.
        let stale_approved_plan_pause = matches!(
            &processing_result,
            TurnProcessingResult::TextResponse { text, .. }
                if turn_processing_ctx.is_approved_plan_execution()
                    && is_stale_approved_plan_pause_response(text)
        );
        if stale_approved_plan_pause
            && turn_processing_ctx.tool_registry.completion_verification_pending()
            && !turn_processing_ctx.in_tool_free_recovery_synthesis()
        {
            // The stale text was discarded (never committed), so no content
            // is available for completion-claim detection; pass empty text.
            match turn_processing_ctx.handle_pending_verification_text_response(&mut repeated_tool_attempts, "")? {
                PendingVerificationTextOutcome::Block { reason } => {
                    result = TurnLoopResult::Blocked { reason: Some(reason) };
                    break;
                }
                // The stale pause was discarded, so the normal text path never
                // sees it: execute the verifier here rather than looping on
                // another pause echo.
                PendingVerificationTextOutcome::AutoVerify { command } => {
                    match execute_harness_auto_verification(
                        &mut turn_processing_ctx,
                        &mut repeated_tool_attempts,
                        &mut turn_modified_files,
                        step_count,
                        current_max_tool_loops,
                        turn_config.tool_repeat_limit,
                        command,
                    )
                    .await?
                    {
                        TurnHandlerOutcome::Continue => continue,
                        TurnHandlerOutcome::Break(outcome_result) => {
                            result = outcome_result;
                            break;
                        }
                        // Agent-switch/policy outcomes have no producer for a lone
                        // verifier execution today (`pending_primary_agent` is
                        // never set by exec paths); keep the turn alive on
                        // them, loudly, so a future producer surfaces in logs
                        // instead of vanishing.
                        TurnHandlerOutcome::SwitchPrimaryAgent(_)
                        | TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { .. }
                        | TurnHandlerOutcome::BreakWithPolicy { .. } => {
                            tracing::warn!(
                                "harness auto-verification produced an unexpected agent-switch/policy outcome; keeping the turn alive"
                            );
                            continue;
                        }
                    }
                }
            }
        }
        if stale_approved_plan_pause {
            // The stale pause text is discarded (never committed to history),
            // so it must not consume the generic text-response budget; that
            // budget is recorded exactly once when the replacement response
            // is processed. Retries stay bounded by
            // `MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES`, and the generic cap at
            // the top of the loop remains responsible for runaway prose.
            // Once the retries are exhausted the turn ends Blocked without
            // storing the stale text, so it can never become a successful
            // final answer.
            if turn_processing_ctx.harness_state.approved_plan_recovery_retries()
                < MAX_APPROVED_PLAN_STALE_PAUSE_RETRIES
            {
                turn_processing_ctx.harness_state.record_approved_plan_recovery_retry();
                turn_processing_ctx
                    .working_history
                    .push(uni::Message::system(APPROVED_PLAN_STALE_PAUSE_RECOVERY_DIRECTIVE.to_string()));
                let _ = turn_processing_ctx
                    .renderer
                    .line(MessageStyle::Info, "Approved-plan execution resumed after clearing stale recovery state.");
                continue;
            }
            result = TurnLoopResult::Blocked {
                reason: Some(STALE_APPROVED_PLAN_PAUSE_BLOCK_REASON.to_string()),
            };
            break;
        }

        // Restore input status if there are no tool calls (turn is completing)
        // This handles the case where defer_restore was set but no tool spinners will take over
        let has_tool_calls = matches!(processing_result, TurnProcessingResult::ToolCalls { .. });
        if !has_tool_calls {
            turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);
        }

        if has_tool_calls {
            turn_processing_ctx.set_phase(TurnPhase::ExecutingTools);
        } else {
            turn_processing_ctx.set_phase(TurnPhase::Finalizing);
        }

        // Handle the turn processing result (dispatch tool calls or finish turn)
        let turn_outcome_result = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut turn_processing_ctx,
            processing_result,
            response_streamed,
            step_count,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: current_max_tool_loops,
            tool_repeat_limit: turn_config.tool_repeat_limit,
        })
        .await;
        let turn_outcome = match turn_outcome_result {
            Ok(outcome) => outcome,
            Err(err) => {
                // Record result-handler errors for diagnostics (mirrors llm_request recording)
                ctx.error_recovery.write().await.record_error(
                    "turn_result_handler",
                    format!("{err:#}"),
                    ErrorType::ToolExecution,
                );
                let tool_free_recovery =
                    ctx.harness_state.recovery_pass_used() && ctx.harness_state.recovery_is_tool_free();
                let planning = ctx.is_planning_active();

                // Ensure the inline input and status line are not left showing a
                // loading or shimmer state when the result handler fails and the
                // turn may exit early. Keep the terminal state synchronized.
                crate::agent::runloop::unified::display::reset_inline_input(
                    ctx.handle,
                    ctx.default_placeholder.clone(),
                );
                crate::agent::runloop::unified::status_line::clear_input_status(ctx.handle, ctx.input_status_state);

                match dispatch_post_tool_failure(PostToolRecoveryContext {
                    renderer: &mut *ctx.renderer,
                    working_history: &mut *working_history,
                    harness_state: &mut *ctx.harness_state,
                    harness_emitter: ctx.harness_emitter,
                    plan_session: planning.then_some(&mut *ctx.plan_session),
                    plan_state: planning.then_some(&ctx.tool_registry.planning_workflow_state()),
                    err: &err,
                    step_count,
                    turn_history_start_len,
                    stage: "handle_turn_processing_result",
                    tool_free_recovery,
                })
                .await?
                {
                    PostToolFailureAction::Continue => continue,
                    PostToolFailureAction::Break(r) => {
                        result = r;
                        break;
                    }
                    PostToolFailureAction::Fallthrough => {}
                }
                return Err(err);
            }
        };
        // Record token usage before continuing or breaking
        ctx.context_manager.update_token_usage(&response_usage);
        #[cfg(debug_assertions)]
        ctx.context_manager.validate_token_tracking(&response_usage);

        match turn_outcome {
            TurnHandlerOutcome::Continue => continue,
            TurnHandlerOutcome::SwitchPrimaryAgent(agent) => {
                // Primary-agent handoff after the turn. Plan *entry* selects the
                // plan agent without claiming an approved-plan execution turn;
                // plan→build/auto still uses the approved-plan path below.
                let plan_entry = is_plan_entry_handoff(&agent);
                pending_primary_agent = Some(agent);
                result = TurnLoopResult::Completed { plan_approved_execution_pending: !plan_entry };
                break;
            }
            TurnHandlerOutcome::SwitchPrimaryAgentWithPolicy { target } => {
                pending_plan_execution_target = Some(target);
                result = TurnLoopResult::Completed { plan_approved_execution_pending: true };
                break;
            }
            TurnHandlerOutcome::BreakWithPolicy { result: outcome_result, target } => {
                pending_plan_execution_target = Some(target);
                result = outcome_result;
                break;
            }
            TurnHandlerOutcome::Break(outcome_result) => {
                // When the model violates the tool-free recovery contract
                // (emits tool calls or textual tool-call markup instead of a
                // final answer), retry the synthesis pass with a corrective
                // directive instead of immediately concluding with the
                // deterministic fallback answer. Mirrors the Empty+tool_calls
                // retry path above. Observed in checkpoint turn_621: a single
                // textual `<tool_call>` block in the recovery response
                // terminated the turn and discarded ~60 messages of gathered
                // context.
                let contract_violation = matches!(
                    outcome_result,
                    TurnLoopResult::Blocked {
                        reason: Some(ref reason)
                    } if reason == RECOVERY_CONTRACT_VIOLATION_REASON
                );
                if tool_free_recovery
                    && !ctx.is_planning_active()
                    && contract_violation
                    && ctx.harness_state.recovery_retry_count() < MAX_RECOVERY_RETRIES
                    && ctx.harness_state.retry_recovery_pass()
                {
                    tracing::warn!(
                        retry = ctx.harness_state.recovery_retry_count(),
                        max = MAX_RECOVERY_RETRIES,
                        "Recovery contract violation; retrying tool-free synthesis pass"
                    );
                    let directive = if ctx.is_planning_active() {
                        RECOVERY_TOOL_CALL_RETRY_DIRECTIVE_PLAN_MODE
                    } else {
                        RECOVERY_TOOL_CALL_RETRY_DIRECTIVE
                    };
                    working_history.push(uni::Message::system(directive.to_string()));
                    continue;
                }
                // Wall-clock-exhausted planning turns must finalize instead of
                // re-forcing the interview (see `dispatch_post_tool_failure`).
                if tool_free_recovery
                    && (ctx.harness_state.wall_clock_exhausted_emitted
                        || ctx.harness_state.wall_clock_exhausted()
                        || ctx.harness_state.tool_budget_exhausted_emitted)
                    && ctx.is_planning_active()
                {
                    ctx.plan_session.mark_recovery_exhausted();
                }
                let salvaged = ctx.harness_state.take_recovery_rejected_synthesis();
                let planning = ctx.is_planning_active();
                let plan_session_opt = planning.then_some(&mut *ctx.plan_session);
                let plan_state = ctx.tool_registry.planning_workflow_state();
                let plan_state_opt = planning.then_some(&plan_state);
                let event_thread_id = ctx.harness_state.run_id.0.clone();
                let event_turn_id = ctx.harness_state.turn_id.0.clone();
                let event_context = PlanRecoveryEventContext {
                    emitter: ctx.harness_emitter,
                    thread_id: &event_thread_id,
                    turn_id: &event_turn_id,
                };
                result = normalize_tool_free_recovery_break_outcome_with_events(
                    working_history,
                    outcome_result,
                    tool_free_recovery,
                    salvaged,
                    plan_session_opt,
                    plan_state_opt,
                    Some(event_context),
                    Some(&mut *ctx.harness_state),
                )
                .await;
                break;
            }
        }
    }

    if let Some(stop) = crate::agent::runloop::unified::stop_requests::stop_outcome(ctx.ctrl_c_state) {
        result = stop;
        pending_primary_agent = None;
        pending_plan_execution_target = None;
    }
    if !ctx.is_planning_active()
        && matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: false })
        && ctx.tool_registry.completion_verification_pending()
    {
        repeated_tool_attempts.mark_verification_pending();
        ctx.session_stats
            .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());
        result = TurnLoopResult::Blocked {
            reason: Some(ctx.tool_registry.unverified_completion_reason()),
        };
    }
    // A terminal or failed synthesis cannot leave a stale resume grant for an internal follow-up.
    ctx.session_stats.navigation_recovery.consume();
    ctx.session_stats.preflight_recovery.finish_corrective_turn();
    if ctx.session_stats.preflight_recovery.exhausted()
        && !ctx.harness_state.turn_refused()
        && matches!(result, TurnLoopResult::Completed { plan_approved_execution_pending: false })
        && pending_primary_agent.is_none()
        && pending_plan_execution_target.is_none()
    {
        result = TurnLoopResult::Blocked {
            reason: Some("Automatic preflight validation recovery stopped after one corrective turn without a successful correction. Type continue to resume with corrected arguments.".to_string()),
        };
    }
    if let TurnLoopResult::Blocked { reason } = &result {
        ensure_blocked_turn_response(
            &mut ctx,
            working_history,
            turn_history_start_len,
            reason.as_deref().unwrap_or("blocked"),
        )?;
    }

    // An approved-plan handoff or plan-entry handoff is a completed control-
    // flow turn, not a user-visible assistant turn. Its follow-up request is
    // constructed by the outer session loop, so requiring a final assistant
    // response here would convert the handoff into `Blocked` before the
    // implementation/planning turn can start.
    //
    // Consume the deferred plan-entry switch *before* the guard: mid-turn
    // `start_planning` queues it without breaking the turn, and the handoff
    // must be exempt like an explicit `SwitchPrimaryAgent("plan")`.
    let deferred_plan_entry_switch = ctx.plan_session.take_plan_entry_agent_switch()
        && !matches!(result, TurnLoopResult::Cancelled | TurnLoopResult::Exit);
    let primary_agent_handoff =
        is_primary_agent_handoff(&pending_primary_agent, &pending_plan_execution_target, deferred_plan_entry_switch);
    let final_response_was_fallback = if completed_turn_requires_final_response(&result, primary_agent_handoff) {
        ensure_completed_turn_response(&mut ctx, working_history, turn_history_start_len)?
    } else {
        ctx.harness_state.final_response_was_fallback()
    };
    if completed_turn_requires_final_response(&result, primary_agent_handoff) {
        if final_response_was_fallback {
            let reason = completed_recovery_fallback_reason(ctx.is_planning_active(), ctx.harness_state);
            // Diagnostic for false-Blocked reports (e.g. simple requests ending
            // with COMPLETED_TURN_FALLBACK_REASON despite a visible answer):
            // records whether a final was present in this turn's slice and on
            // which surfaces it was published, so the next recurrence is
            // triaged from logs instead of checkpoint archaeology.
            tracing::warn!(
                turn_history_start_len,
                working_history_len = working_history.len(),
                rendered = ctx.harness_state.final_response_rendered(),
                event_emitted = ctx.harness_state.final_response_event_emitted(),
                planning_active = ctx.is_planning_active(),
                "Completed turn had fallback final response; converting to Blocked"
            );
            result = TurnLoopResult::Blocked { reason: Some(reason.to_string()) };
        } else if !ctx.harness_state.final_response_rendered() || !ctx.harness_state.final_response_event_emitted() {
            tracing::warn!(
                turn_history_start_len,
                working_history_len = working_history.len(),
                rendered = ctx.harness_state.final_response_rendered(),
                event_emitted = ctx.harness_state.final_response_event_emitted(),
                "Completed turn lacked a published final response; converting to Blocked"
            );
            result = TurnLoopResult::Blocked {
                reason: Some(COMPLETED_TURN_NO_RESPONSE_REASON.to_string()),
            };
        } else if ctx.plan_session.is_recovery_exhausted() {
            result = TurnLoopResult::Blocked {
                reason: Some(PLAN_RECOVERY_EXHAUSTED_REASON.to_string()),
            };
        }
    }

    // Apply the deferred plan-entry agent switch at this turn boundary unless
    // a stronger handoff (approved-plan policy target / explicit switch) owns
    // the boundary. Apply on Blocked too: plan mode often blocks tools in the
    // entry turn, and discarding here would leave the build agent selected
    // forever while planning stays active.
    if deferred_plan_entry_switch {
        if pending_primary_agent.is_none() && pending_plan_execution_target.is_none() {
            pending_primary_agent =
                Some(crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME.to_string());
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                agent = %crate::agent::runloop::unified::planning_workflow_state::PLAN_PRIMARY_AGENT_NAME,
                "Applying deferred plan primary-agent switch after turn boundary"
            );
        } else {
            tracing::info!(
                target: "vtcode.planning_workflow",
                switch_path = "plan_entry",
                has_primary_agent = pending_primary_agent.is_some(),
                has_plan_execution_target = pending_plan_execution_target.is_some(),
                "Discarding deferred plan primary-agent switch; stronger handoff owns the boundary"
            );
        }
    }

    ctx.renderer.flush_compact_command_group();
    ctx.set_phase(TurnPhase::Finalizing);
    finalize_turn(&mut ctx, working_history, &result, &turn_usage).await;

    // Final outcome with the correct result status
    ctx.session_stats.record_turn_completed();
    let plan_approved_execution_pending =
        matches!(&result, TurnLoopResult::Completed { plan_approved_execution_pending: true });
    ctx.session_stats
        .set_verification_snapshot(repeated_tool_attempts.verification_snapshot());
    let in_progress_sessions = if matches!(result, TurnLoopResult::Cancelled | TurnLoopResult::Exit) {
        Vec::new()
    } else {
        crate::agent::runloop::unified::stop_requests::await_with_stop(
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            ctx.tool_registry.in_progress_exec_sessions(4),
        )
        .await
        .unwrap_or_default()
    };
    let turn_diagnostics = ctx
        .harness_state
        .snapshot_turn_diagnostics(turn_usage.clone(), repeated_tool_attempts.low_signal_tool_calls)
        .with_in_progress_exec_sessions(in_progress_sessions);
    // Touched files include reads/searches recorded during this turn plus
    // every modified file, so checkpoint replay shows evidence even for
    // read-only turns without snapshotting file contents. `recent_touched_files`
    // is a bounded session window (last 5), so turns touching more than 5 new
    // files keep only the most recent; that is an intentional replay-context
    // bound, not a per-turn isolation guarantee.
    let mut turn_touched_files: BTreeSet<PathBuf> = turn_modified_files.clone();
    for touched in ctx.session_stats.recent_touched_files() {
        let trimmed = touched.trim();
        if !trimmed.is_empty() && !touched_at_turn_start.contains(trimmed) {
            turn_touched_files.insert(PathBuf::from(trimmed));
        }
    }
    Ok(TurnLoopOutcome {
        result,
        turn_modified_files,
        turn_touched_files,
        turn_diagnostics,
        pending_primary_agent,
        pending_plan_execution_target,
        plan_approved_execution_pending,
        final_response_was_fallback,
        refused: ctx.harness_state.turn_refused(),
    })
}

/// Verify the new model actually receives the updated mid-turn prompt verbatim.
///
/// Called right after a mid-turn settings/model switch plus steering delivery,
/// before the next LLM request is built. Fresh steered user input must sit at
/// the history tail after compaction plus the resume note — never summarized
/// away. Logs the outcome for trajectory diagnostics; the TUI echo for the
/// steered prompt already happened in `handle_steering_messages`, so this stays
/// log-only to avoid duplicate transcript noise.
fn verify_mid_turn_prompt_received(working_history: &[uni::Message], turn_history_start_len: usize) {
    let start = turn_history_start_len.min(working_history.len());
    let tail = working_history.get(start..).unwrap_or(&[]);
    if let Some(latest_user) = tail.iter().rev().find(|m| m.role == uni::MessageRole::User) {
        let preview: String = latest_user.content.as_text().chars().take(120).collect();
        tracing::info!(
            prompt_preview = %preview,
            tail_len = tail.len(),
            history_len = working_history.len(),
            "Verified mid-turn prompt reaches new model verbatim"
        );
    } else {
        tracing::info!(
            history_len = working_history.len(),
            "Model switched mid-turn with no fresh user prompt in current turn; compacted context plus resume note retained"
        );
    }
}

/// Finalize the turn: terminate sessions if needed, emit outcome events,
/// and send notifications.
async fn finalize_turn(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &[uni::Message],
    result: &TurnLoopResult,
    turn_usage: &HarnessUsage,
) {
    use vtcode_commons::program_status::{ProgramState, ProgramStatusUpdate};
    let state = match result {
        TurnLoopResult::Completed { .. } => ProgramState::Done,
        TurnLoopResult::Aborted => ProgramState::Error,
        TurnLoopResult::Blocked { .. } => ProgramState::Blocked,
        TurnLoopResult::Cancelled | TurnLoopResult::Exit => ProgramState::Idle,
    };
    ctx.handle.program_status(ProgramStatusUpdate::Outcome(state));
    if let TurnLoopResult::Blocked { reason } = result {
        let reason_text = reason.as_deref().unwrap_or("turn blocked");
        let outcome = if reason_text.contains(ASSISTANT_TEXT_RESPONSE_CAP_REASON) {
            "safety_cap_handoff"
        } else {
            "blocked_handoff"
        };
        tracing::info!(
            target: "vtcode.turn.metrics",
            metric = "turn_handoff",
            outcome,
            reason = reason_text,
            run_id = %ctx.harness_state.run_id.0,
            turn_id = %ctx.harness_state.turn_id.0,
            "turn handoff metric"
        );
    }
    if matches!(result, TurnLoopResult::Cancelled) {
        terminate_exec_sessions_bounded(
            ctx.tool_registry.terminate_active_exec_sessions_for_exit_async(),
            "active exec sessions after turn cancellation",
        )
        .await;
    } else if matches!(result, TurnLoopResult::Exit) {
        terminate_exec_sessions_bounded(
            ctx.tool_registry.terminate_all_exec_sessions_for_exit_async(),
            "all exec sessions after turn exit",
        )
        .await;
    }
    if let Some(emitter) = ctx.harness_emitter {
        // Exit is a graceful user-initiated action, not a failure
        let event = match result {
            TurnLoopResult::Exit => turn_completed_event(turn_usage.clone()),
            TurnLoopResult::Completed { .. } => {
                // Capture live exec sessions only for the completed path so
                // `turn.completed` carries the same bounded id set as
                // `SnapshotTurnDiagnostics` for cross-turn resume correlation.
                // Backend-checked, newest first, capped at 4 by the helper.
                // Stops skip this optional lookup so reader contention cannot
                // delay accepted exit.
                let in_progress_ids: Vec<String> = crate::agent::runloop::unified::stop_requests::await_with_stop(
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    ctx.tool_registry
                        .in_progress_exec_sessions(vtcode_core::exec::events::MAX_IN_PROGRESS_EXEC_SESSIONS),
                )
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.id.as_str().to_string())
                .collect();
                if in_progress_ids.is_empty() {
                    turn_completed_event(turn_usage.clone())
                } else {
                    turn_completed_event_with_sessions(turn_usage.clone(), in_progress_ids)
                }
            }
            TurnLoopResult::Aborted => {
                turn_failed_event("turn aborted", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
            TurnLoopResult::Cancelled => {
                turn_failed_event("turn cancelled", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
            TurnLoopResult::Blocked { .. } => {
                turn_failed_event("turn blocked", has_turn_usage(turn_usage).then_some(turn_usage.clone()))
            }
        };
        if let Err(e) = emitter.emit(event) {
            tracing::debug!(error = %e, "harness turn outcome event emission failed");
        }
        if let TurnLoopResult::Blocked { reason } = result {
            let message = reason.clone().unwrap_or_else(|| "turn blocked".to_string());
            // Blocked-call fuse telemetry (last tool, enforced caps) captured
            // at trip time; blocks that did not trip the fuse keep the
            // previous `None`/zero fields.
            let fuse_telemetry = ctx.harness_state.take_blocked_tool_recovery_telemetry();
            let (last_tool, consecutive_cap, total_cap, blocked_streak, blocked_total) = fuse_telemetry
                .map(|telemetry| {
                    (
                        Some(telemetry.last_tool),
                        telemetry.consecutive_cap,
                        telemetry.total_cap,
                        telemetry.blocked_streak,
                        telemetry.blocked_total,
                    )
                })
                .unwrap_or((
                    None,
                    0,
                    0,
                    ctx.harness_state.consecutive_blocked_tool_calls,
                    ctx.harness_state.blocked_tool_calls,
                ));
            let blocked_event = vtcode_core::exec::events::TurnBlockedEvent {
                completed_at: None,
                message: message.clone(),
                last_tool,
                blocked_streak,
                blocked_total,
                consecutive_cap,
                total_cap,
                recovery_active: ctx.harness_state.is_recovery_active() || ctx.harness_state.recovery_pass_used(),
                usage: has_turn_usage(turn_usage).then(|| turn_usage.clone()),
            };
            if let Err(e) = emitter.emit(turn_blocked_event(blocked_event)) {
                tracing::debug!(error = %e, "harness turn.blocked event emission failed");
            }
            if let Err(e) = emitter.emit(harness_event(
                vtcode_core::exec::events::HarnessEventKind::TurnBlocked,
                Some(message),
                None,
                None,
                None,
            )) {
                tracing::debug!(error = %e, "harness TurnBlocked event emission failed");
            }
        }

        // Close streamed tool-call items whose calls never reached the
        // pipeline. Entries only survive here when the call was rejected
        // pre-flight, dropped from a batch, or interrupted by the turn ending;
        // dispatched calls remove themselves on execution or rejection.
        for (tool_call_id, streamed) in ctx.harness_state.take_all_streamed_tool_call_item_ids() {
            let failed = ToolCallStatus::Failed;
            let raw_id = (!tool_call_id.trim().is_empty()).then_some(tool_call_id.as_str());
            let _ = emitter.emit(tool_invocation_completed_event(
                streamed.item_id.clone(),
                &streamed.tool_name,
                None,
                raw_id,
                failed.clone(),
                ToolOutcome::Cancelled,
            ));
            let _ = emitter.emit(tool_output_completed_event(
                streamed.item_id,
                raw_id,
                failed,
                None,
                None,
                UNDISPATCHED_TOOL_CALL_CLOSURE_TEXT,
            ));
        }
    }
    let _ = crate::agent::runloop::unified::stop_requests::await_with_stop(
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        emit_turn_outcome_notification(
            effective_vt_cfg(ctx.vt_cfg, &ctx.live_vt_cfg),
            working_history,
            ctx.config.workspace.as_path(),
            ctx.harness_state,
            result,
        ),
    )
    .await;
}

/// Terminate exec sessions during turn-level Exit/Cancelled teardown, bounded
/// by [`TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT`]. A timeout only skips waiting: the
/// session-end backstop re-runs the sweep and the OS reaps any remainder at
/// process exit.
async fn terminate_exec_sessions_bounded(termination: impl Future<Output = Result<()>>, phase: &str) {
    match timeout(TURN_EXIT_EXEC_SHUTDOWN_TIMEOUT, termination).await {
        Ok(Ok(())) => {}
        Ok(Err(err)) => tracing::warn!(error = %err, "Failed to terminate {phase}"),
        Err(_elapsed) => tracing::warn!("timed out terminating {phase}; continuing teardown"),
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod hypothesis_revision_tests;
