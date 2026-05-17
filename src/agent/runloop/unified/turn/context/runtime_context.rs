use super::*;

/// Result of processing a single turn
pub(crate) enum TurnProcessingResult {
    /// Turn resulted in tool calls that need to be executed
    ToolCalls {
        tool_calls: Vec<PreparedAssistantToolCall>,
        assistant_text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
    },
    /// Turn resulted in a text response
    TextResponse {
        text: String,
        reasoning: Vec<ReasoningSegment>,
        reasoning_details: Option<Vec<String>>,
        proposed_plan: Option<String>,
    },
    /// Turn resulted in no actionable output
    Empty,
}

pub(crate) enum TurnHandlerOutcome {
    Continue,
    Break(TurnLoopResult),
}

pub(crate) struct TurnOutcomeContext<'a> {
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub completed_turn_prompt: Option<&'a str>,
    pub completed_turn_prompt_message_index: Option<usize>,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub default_placeholder: &'a Option<String>,
    pub checkpoint_manager: Option<&'a SnapshotManager>,
    pub next_checkpoint_turn: &'a mut usize,
    pub session_end_reason: &'a mut SessionEndReason,
    pub turn_elapsed: Duration,
    pub show_turn_timer: bool,
    pub workspace: &'a std::path::Path,
    pub session_id: &'a str,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
}

pub(crate) struct ToolContext<'a> {
    pub tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub tool_permission_cache: &'a Arc<RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
    pub safety_validator:
        &'a Arc<crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator>,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
}

pub(crate) struct LLMContext<'a> {
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub config: &'a mut vtcode_core::config::types::AgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
}

pub(crate) struct UIContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut vtcode_tui::app::InlineSession,
    pub active_thread_label: &'a str,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
}

pub(crate) struct TurnProcessingState<'a> {
    pub session_stats: &'a mut SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub working_history: &'a mut Vec<uni::Message>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub skip_confirmations: bool,
    pub full_auto: bool,
    pub harness_state: &'a mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    pub runtime_steering: &'a mut RuntimeSteering,
}

pub(crate) struct TurnProcessingContextParts<'a> {
    pub tool: ToolContext<'a>,
    pub llm: LLMContext<'a>,
    pub ui: UIContext<'a>,
    pub state: TurnProcessingState<'a>,
}

/// Context for turn processing operations
pub(crate) struct TurnProcessingContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<vtcode_core::tools::ToolResultCache>>,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub working_history: &'a mut Vec<uni::Message>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub active_thread_label: &'a str,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub session: &'a mut vtcode_tui::app::InlineSession,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub permissions_state: &'a Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
    pub safety_validator:
        &'a Arc<crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub config: &'a mut vtcode_core::config::types::AgentConfig,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub skip_confirmations: bool,
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
    pub runtime_steering: &'a mut RuntimeSteering,
}

impl<'a> TurnProcessingContext<'a> {
    pub(crate) fn from_parts(parts: TurnProcessingContextParts<'a>) -> Self {
        let TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        } = parts;

        Self {
            renderer: ui.renderer,
            handle: ui.handle,
            session_stats: state.session_stats,
            auto_exit_plan_mode_attempted: state.auto_exit_plan_mode_attempted,
            mcp_panel_state: state.mcp_panel_state,
            tool_result_cache: tool.tool_result_cache,
            approval_recorder: tool.approval_recorder,
            decision_ledger: llm.decision_ledger,
            working_history: state.working_history,
            turn_metadata_cache: state.turn_metadata_cache,
            tool_registry: tool.tool_registry,
            tools: tool.tools,
            tool_catalog: tool.tool_catalog,
            ctrl_c_state: ui.ctrl_c_state,
            ctrl_c_notify: ui.ctrl_c_notify,
            active_thread_label: ui.active_thread_label,
            vt_cfg: llm.vt_cfg,
            context_manager: llm.context_manager,
            last_forced_redraw: ui.last_forced_redraw,
            input_status_state: ui.input_status_state,
            session: ui.session,
            lifecycle_hooks: ui.lifecycle_hooks,
            default_placeholder: ui.default_placeholder,
            tool_permission_cache: tool.tool_permission_cache,
            permissions_state: tool.permissions_state,
            safety_validator: tool.safety_validator,
            provider_client: llm.provider_client,
            config: llm.config,
            traj: llm.traj,
            skip_confirmations: state.skip_confirmations,
            full_auto: state.full_auto,
            circuit_breaker: tool.circuit_breaker,
            tool_health_tracker: tool.tool_health_tracker,
            rate_limiter: tool.rate_limiter,
            telemetry: tool.telemetry,
            autonomous_executor: tool.autonomous_executor,
            error_recovery: tool.error_recovery,
            harness_state: state.harness_state,
            harness_emitter: state.harness_emitter,
            runtime_steering: state.runtime_steering,
        }
    }

    pub(crate) fn parts_mut(&mut self) -> TurnProcessingContextParts<'_> {
        let tool = ToolContext {
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
        let llm = LLMContext {
            provider_client: self.provider_client,
            config: self.config,
            vt_cfg: self.vt_cfg,
            context_manager: self.context_manager,
            decision_ledger: self.decision_ledger,
            traj: self.traj,
        };
        let ui = UIContext {
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            active_thread_label: self.active_thread_label,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state: self.input_status_state,
        };
        let state = TurnProcessingState {
            session_stats: self.session_stats,
            auto_exit_plan_mode_attempted: self.auto_exit_plan_mode_attempted,
            mcp_panel_state: self.mcp_panel_state,
            working_history: self.working_history,
            turn_metadata_cache: self.turn_metadata_cache,
            skip_confirmations: self.skip_confirmations,
            full_auto: self.full_auto,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
            runtime_steering: self.runtime_steering,
        };

        TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        }
    }

    /// Creates a TurnLoopContext from this TurnProcessingContext.
    /// This is used when calling handle_tool_execution_result which requires TurnLoopContext.
    pub(crate) fn as_turn_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_> {
        let TurnProcessingContextParts {
            tool: tool_ctx,
            llm: llm_ctx,
            ui: ui_ctx,
            state,
        } = self.parts_mut();

        crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext::new(
            ui_ctx.renderer,
            ui_ctx.handle,
            ui_ctx.session,
            state.session_stats,
            state.auto_exit_plan_mode_attempted,
            state.mcp_panel_state,
            tool_ctx.tool_result_cache,
            tool_ctx.approval_recorder,
            llm_ctx.decision_ledger,
            tool_ctx.tool_registry,
            tool_ctx.tools,
            tool_ctx.tool_catalog,
            ui_ctx.ctrl_c_state,
            ui_ctx.ctrl_c_notify,
            llm_ctx.context_manager,
            ui_ctx.last_forced_redraw,
            ui_ctx.input_status_state,
            ui_ctx.lifecycle_hooks,
            ui_ctx.default_placeholder,
            tool_ctx.tool_permission_cache,
            tool_ctx.permissions_state,
            tool_ctx.safety_validator,
            tool_ctx.circuit_breaker,
            tool_ctx.tool_health_tracker,
            tool_ctx.rate_limiter,
            tool_ctx.telemetry,
            tool_ctx.autonomous_executor,
            tool_ctx.error_recovery,
            state.harness_state,
            state.harness_emitter,
            llm_ctx.config,
            llm_ctx.vt_cfg,
            state.turn_metadata_cache,
            llm_ctx.provider_client,
            llm_ctx.traj,
            state.skip_confirmations,
            state.full_auto,
            state.runtime_steering,
        )
    }

    /// Creates a RunLoopContext directly from this TurnProcessingContext,
    /// skipping the intermediate TurnLoopContext conversion.
    pub(crate) fn as_run_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
        let TurnProcessingContextParts {
            tool: tool_ctx,
            llm: llm_ctx,
            ui: ui_ctx,
            state,
        } = self.parts_mut();

        let auto_mode = Some(
            crate::agent::runloop::unified::run_loop_context::AutoModeRuntimeContext {
                config: llm_ctx.config,
                vt_cfg: llm_ctx.vt_cfg,
                provider_client: llm_ctx.provider_client.as_mut(),
                working_history: state.working_history.as_slice(),
            },
        );

        crate::agent::runloop::unified::run_loop_context::RunLoopContext::new_with_auto_mode_context(
            ui_ctx.renderer,
            ui_ctx.handle,
            tool_ctx.tool_registry,
            tool_ctx.tools,
            tool_ctx.tool_result_cache,
            tool_ctx.tool_permission_cache,
            tool_ctx.permissions_state,
            llm_ctx.decision_ledger,
            state.session_stats,
            state.mcp_panel_state,
            tool_ctx.approval_recorder,
            ui_ctx.session,
            Some(tool_ctx.safety_validator),
            llm_ctx.traj,
            state.harness_state,
            state.harness_emitter,
            auto_mode,
        )
    }
}
