use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;

use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::InlineHandle;

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::model_picker::ModelPickerState;
use vtcode_core::core::agent::steering::SteeringMessage;

use crate::agent::runloop::unified::async_mcp_manager::AsyncMcpManager;
use crate::agent::runloop::unified::palettes::ActivePalette;
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::welcome::SessionBootstrap;
use crate::hooks::lifecycle::SessionEndReason;

#[allow(clippy::too_many_arguments)]
pub(crate) struct InteractionLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub session: &'a mut vtcode_tui::InlineSession,
    pub handle: &'a InlineHandle,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub config: &'a mut AgentConfig,
    pub vt_cfg: &'a mut Option<VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub session_bootstrap: &'a SessionBootstrap,
    pub async_mcp_manager: &'a Option<Arc<AsyncMcpManager>>,
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub tools: &'a Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<ToolCatalogState>,
    pub conversation_history: &'a mut Vec<uni::Message>,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub session_stats: &'a mut SessionStats,
    pub mcp_panel_state: &'a mut crate::agent::runloop::mcp_events::McpPanelState,
    pub linked_directories:
        &'a mut Vec<crate::agent::runloop::unified::workspace_links::LinkedDirectory>,
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub full_auto: bool,
    pub approval_recorder: &'a Arc<vtcode_core::tools::ApprovalRecorder>,
    pub tool_permission_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>,
    pub loaded_skills:
        &'a Arc<tokio::sync::RwLock<std::collections::HashMap<String, vtcode_core::skills::Skill>>>,
    pub default_placeholder: &'a mut Option<String>,
    pub follow_up_placeholder: &'a mut Option<String>,
    pub checkpoint_manager: Option<&'a vtcode_core::core::agent::snapshots::SnapshotManager>,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub harness_emitter:
        Option<&'a crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter>,
    pub safety_validator: &'a Arc<
        tokio::sync::RwLock<
            crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator,
        >,
    >,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub last_forced_redraw: &'a mut std::time::Instant,
    pub harness_config: vtcode_config::core::agent::AgentHarnessConfig,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a> InteractionLoopContext<'a> {
    pub fn as_turn_processing_context<'b>(
        &'b mut self,
        harness_state: &'b mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
        auto_exit_plan_mode_attempted: &'b mut bool,
        input_status_state: &'b mut crate::agent::runloop::unified::status_line::InputStatusState,
    ) -> crate::agent::runloop::unified::turn::context::TurnProcessingContext<'b> {
        crate::agent::runloop::unified::turn::context::TurnProcessingContext::new(
            self.renderer,
            self.handle,
            self.session_stats,
            auto_exit_plan_mode_attempted,
            self.mcp_panel_state,
            self.tool_result_cache,
            self.approval_recorder,
            self.decision_ledger,
            self.conversation_history,
            self.tool_registry,
            self.tools,
            self.tool_catalog,
            self.ctrl_c_state,
            self.ctrl_c_notify,
            self.vt_cfg.as_ref(),
            self.context_manager,
            self.last_forced_redraw,
            input_status_state,
            self.session,
            self.lifecycle_hooks,
            self.default_placeholder,
            self.tool_permission_cache,
            self.safety_validator,
            self.provider_client,
            self.config,
            self.traj,
            self.full_auto,
            self.circuit_breaker,
            self.tool_health_tracker,
            self.rate_limiter,
            self.telemetry,
            self.autonomous_executor,
            self.error_recovery,
            harness_state,
            self.harness_emitter,
            self.steering_receiver,
        )
    }
}

pub(crate) struct InteractionState<'a> {
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub queued_inputs: &'a mut VecDeque<String>,
    pub model_picker_state: &'a mut Option<ModelPickerState>,
    pub palette_state: &'a mut Option<ActivePalette>,
    pub last_known_mcp_tools: &'a mut Vec<String>,
    pub mcp_catalog_initialized: &'a mut bool,
    pub last_mcp_refresh: &'a mut Instant,
    pub ctrl_c_notice_displayed: &'a mut bool,
}

pub(crate) enum InteractionOutcome {
    Continue {
        input: String,
    },
    /// A direct tool command (e.g. `!cmd` / `run ...`) was executed and rendered;
    /// no LLM turn should be started for this loop iteration.
    DirectToolHandled,
    Exit {
        reason: SessionEndReason,
    },
    Resume {
        resume_session: Box<ResumeSession>,
    },
    /// Plan approved by user (Claude Code style HITL) - transition from Plan to Edit mode
    PlanApproved {
        /// If true, auto-accept file edits without prompting
        auto_accept: bool,
        /// If true, clear conversation context before continuing
        clear_context: bool,
    },
}

pub(crate) async fn run_interaction_loop(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
) -> Result<InteractionOutcome> {
    super::interaction_loop_runner::run_interaction_loop_impl(ctx, state).await
}
