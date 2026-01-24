use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::state::SessionStats;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Instant;
use tokio::sync::Notify;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::snapshots::SnapshotManager;
use vtcode_core::llm::provider as uni;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::unified::state::CtrlCState;

#[allow(dead_code)]
pub enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Blocked { reason: Option<String> },
}

/// Result of processing a single turn
#[allow(dead_code)]
pub(crate) enum TurnProcessingResult {
    /// Turn resulted in tool calls that need to be executed
    ToolCalls {
        tool_calls: Vec<uni::ToolCall>,
        assistant_text: String,
        reasoning: Option<String>,
    },
    /// Turn resulted in a text response
    TextResponse {
        text: String,
        reasoning: Option<String>,
    },
    /// Turn resulted in no actionable output
    Empty,
    /// Turn was completed successfully
    Completed,
    /// Turn was cancelled by user
    Cancelled,
    /// Turn was aborted due to error
    Aborted,
}

pub(crate) enum TurnHandlerOutcome {
    Continue,
    Break(TurnLoopResult),
}

pub struct TurnOutcomeContext<'a> {
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub default_placeholder: &'a Option<String>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub next_checkpoint_turn: &'a mut usize,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
}

/// Context for turn processing operations
#[allow(dead_code)]
pub(crate) struct TurnProcessingContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub working_history: &'a mut Vec<uni::Message>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse (HP-3 optimization)
    pub cached_tools: &'a Option<Arc<Vec<uni::ToolDefinition>>>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub session: &'a mut vtcode_core::ui::tui::InlineSession,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub safety_validator: &'a Arc<
        tokio::sync::RwLock<
            crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator,
        >,
    >,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub full_auto: bool,
    // Phase 4 Integration
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub harness_state: &'a mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
}

impl<'a> TurnProcessingContext<'a> {
    pub fn new(
        ctx: &'a mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_>,
        working_history: &'a mut Vec<uni::Message>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        vt_cfg: Option<&'a VTCodeConfig>,
        full_auto: bool,
    ) -> Self {
        Self {
            renderer: ctx.renderer,
            handle: ctx.handle,
            session_stats: ctx.session_stats,
            auto_exit_plan_mode_attempted: ctx.auto_exit_plan_mode_attempted,
            mcp_panel_state: ctx.mcp_panel_state,
            tool_result_cache: ctx.tool_result_cache,
            approval_recorder: ctx.approval_recorder,
            decision_ledger: ctx.decision_ledger,
            working_history,
            tool_registry: ctx.tool_registry,
            tools: ctx.tools,
            cached_tools: ctx.cached_tools,
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            vt_cfg,
            context_manager: ctx.context_manager,
            last_forced_redraw: ctx.last_forced_redraw,
            input_status_state: ctx.input_status_state,
            session: ctx.session,
            lifecycle_hooks: ctx.lifecycle_hooks,
            default_placeholder: ctx.default_placeholder,
            tool_permission_cache: ctx.tool_permission_cache,
            safety_validator: ctx.safety_validator,
            provider_client,
            full_auto,
            circuit_breaker: ctx.circuit_breaker,
            tool_health_tracker: ctx.tool_health_tracker,
            rate_limiter: ctx.rate_limiter,
            telemetry: ctx.telemetry,
            autonomous_executor: ctx.autonomous_executor,
            error_recovery: ctx.error_recovery,
            harness_state: ctx.harness_state,
            harness_emitter: ctx.harness_emitter,
        }
    }
}
