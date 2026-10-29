use super::*;

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
