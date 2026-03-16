use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use hashbrown::HashMap;
use tokio::sync::{Notify, RwLock};
use vtcode_config::core::PromptCachingConfig;
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::types::{
    AgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider::{self as uni, ToolDefinition};
use vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter;
use vtcode_core::tools::circuit_breaker::CircuitBreaker;
use vtcode_core::tools::health::ToolHealthTracker;
use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
use vtcode_tui::{InlineHandle, InlineSession};

use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::context::{
    LLMContext, ToolContext, TurnProcessingContext, TurnProcessingContextParts,
    TurnProcessingState, UIContext,
};

#[derive(Clone)]
struct NoopProvider;

#[async_trait::async_trait]
impl uni::LLMProvider for NoopProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_streaming(&self) -> bool {
        false
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Ok(uni::LLMResponse {
            content: Some(String::new()),
            model: "noop-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
    }
}

pub(crate) struct TestTurnProcessingBacking {
    _temp: tempfile::TempDir,
    tool_registry: vtcode_core::tools::ToolRegistry,
    tools: Arc<RwLock<Vec<ToolDefinition>>>,
    tool_result_cache: Arc<RwLock<ToolResultCache>>,
    tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    decision_ledger: Arc<RwLock<DecisionTracker>>,
    approval_recorder: Arc<ApprovalRecorder>,
    session_stats: SessionStats,
    mcp_panel_state: McpPanelState,
    context_manager: ContextManager,
    last_forced_redraw: Instant,
    input_status_state: InputStatusState,
    session: InlineSession,
    handle: InlineHandle,
    renderer: vtcode_core::utils::ansi::AnsiRenderer,
    ctrl_c_state: Arc<CtrlCState>,
    ctrl_c_notify: Arc<Notify>,
    safety_validator: Arc<RwLock<ToolCallSafetyValidator>>,
    circuit_breaker: Arc<CircuitBreaker>,
    tool_health_tracker: Arc<ToolHealthTracker>,
    rate_limiter: Arc<AdaptiveRateLimiter>,
    telemetry: Arc<vtcode_core::core::telemetry::TelemetryManager>,
    autonomous_executor: Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    error_recovery: Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    harness_state: HarnessTurnState,
    auto_exit_plan_mode_attempted: bool,
    working_history: Vec<uni::Message>,
    tool_catalog: Arc<ToolCatalogState>,
    default_placeholder: Option<String>,
    steering_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    config: AgentConfig,
    provider_client: Box<dyn uni::LLMProvider>,
    traj: TrajectoryLogger,
    turn_metadata_cache: Option<Option<serde_json::Value>>,
}

impl TestTurnProcessingBacking {
    pub(crate) async fn new(max_tool_calls: usize) -> Self {
        let temp = tempfile::TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();

        let tool_registry = vtcode_core::tools::ToolRegistry::new(workspace.clone()).await;
        let tools = Arc::new(RwLock::new(Vec::new()));
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let approval_recorder = Arc::new(ApprovalRecorder::new(workspace.clone()));
        let session_stats = SessionStats::default();
        let mcp_panel_state = McpPanelState::default();
        let loaded_skills = Arc::new(RwLock::new(HashMap::new()));
        let context_manager =
            ContextManager::new("You are VT Code.".to_string(), (), loaded_skills, None);
        let last_forced_redraw = Instant::now();
        let input_status_state = InputStatusState::default();
        let mut session = create_headless_session();
        session.set_skip_confirmations(true);
        let handle = session.clone_inline_handle();
        let renderer = vtcode_core::utils::ansi::AnsiRenderer::with_inline_ui(
            handle.clone(),
            Default::default(),
        );
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let safety_validator = Arc::new(RwLock::new(ToolCallSafetyValidator::new()));
        safety_validator.write().await.start_turn().await;
        let circuit_breaker = Arc::new(CircuitBreaker::default());
        let tool_health_tracker = Arc::new(ToolHealthTracker::new(3));
        let rate_limiter = Arc::new(AdaptiveRateLimiter::default());
        let telemetry = Arc::new(vtcode_core::core::telemetry::TelemetryManager::new());
        let autonomous_executor =
            Arc::new(vtcode_core::tools::autonomous_executor::AutonomousExecutor::new());
        let error_recovery = Arc::new(RwLock::new(
            vtcode_core::core::agent::error_recovery::ErrorRecoveryState::default(),
        ));
        let harness_state = HarnessTurnState::new(
            TurnRunId("run-test".to_string()),
            TurnId("turn-test".to_string()),
            max_tool_calls,
            60,
            0,
        );
        let tool_catalog = Arc::new(ToolCatalogState::new());
        let config = AgentConfig {
            model: "noop-model".to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace,
            verbose: false,
            quiet: false,
            theme: "default".to_string(),
            reasoning_effort: ReasoningEffortLevel::Medium,
            ui_surface: UiSurfacePreference::Inline,
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: false,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 10,
            checkpointing_max_age_days: None,
            max_conversation_turns: 16,
            model_behavior: None,
            openai_chatgpt_auth: None,
        };

        Self {
            _temp: temp,
            tool_registry,
            tools,
            tool_result_cache,
            tool_permission_cache,
            decision_ledger,
            approval_recorder,
            session_stats,
            mcp_panel_state,
            context_manager,
            last_forced_redraw,
            input_status_state,
            session,
            handle,
            renderer,
            ctrl_c_state,
            ctrl_c_notify,
            safety_validator,
            circuit_breaker,
            tool_health_tracker,
            rate_limiter,
            telemetry,
            autonomous_executor,
            error_recovery,
            harness_state,
            auto_exit_plan_mode_attempted: false,
            working_history: Vec::new(),
            tool_catalog,
            default_placeholder: None,
            steering_receiver: None,
            config,
            provider_client: Box::new(NoopProvider),
            traj: TrajectoryLogger::disabled(),
            turn_metadata_cache: None,
        }
    }

    pub(crate) async fn add_tool_definition(&self, tool: ToolDefinition) {
        self.tools.write().await.push(tool);
    }

    pub(crate) fn set_steering_receiver(
        &mut self,
        receiver: tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>,
    ) {
        self.steering_receiver = Some(receiver);
    }

    pub(crate) fn legacy_loop_detector(
        &self,
    ) -> Arc<std::sync::RwLock<vtcode_core::core::loop_detector::LoopDetector>> {
        self.autonomous_executor.loop_detector()
    }

    pub(crate) fn recovery_is_tool_free(&self) -> bool {
        self.harness_state.recovery_is_tool_free()
    }

    pub(crate) fn last_history_message_contains(&self, needle: &str) -> bool {
        self.working_history
            .last()
            .is_some_and(|message| message.content.as_text().contains(needle))
    }

    pub(crate) fn turn_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_> {
        crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext::new(
            &mut self.renderer,
            &self.handle,
            &mut self.session,
            &mut self.session_stats,
            &mut self.auto_exit_plan_mode_attempted,
            &mut self.mcp_panel_state,
            &self.tool_result_cache,
            &self.approval_recorder,
            &self.decision_ledger,
            &mut self.tool_registry,
            &self.tools,
            &self.tool_catalog,
            &self.ctrl_c_state,
            &self.ctrl_c_notify,
            &mut self.context_manager,
            &mut self.last_forced_redraw,
            &mut self.input_status_state,
            None,
            &self.default_placeholder,
            &self.tool_permission_cache,
            &self.safety_validator,
            &self.circuit_breaker,
            &self.tool_health_tracker,
            &self.rate_limiter,
            &self.telemetry,
            &self.autonomous_executor,
            &self.error_recovery,
            &mut self.harness_state,
            None,
            &mut self.config,
            None,
            &mut self.turn_metadata_cache,
            &mut self.provider_client,
            &self.traj,
            false,
            &mut self.steering_receiver,
        )
    }

    pub(crate) fn turn_processing_context(&mut self) -> TurnProcessingContext<'_> {
        let tool = ToolContext {
            tool_result_cache: &self.tool_result_cache,
            approval_recorder: &self.approval_recorder,
            tool_registry: &mut self.tool_registry,
            tools: &self.tools,
            tool_catalog: &self.tool_catalog,
            tool_permission_cache: &self.tool_permission_cache,
            safety_validator: &self.safety_validator,
            circuit_breaker: &self.circuit_breaker,
            tool_health_tracker: &self.tool_health_tracker,
            rate_limiter: &self.rate_limiter,
            telemetry: &self.telemetry,
            autonomous_executor: &self.autonomous_executor,
            error_recovery: &self.error_recovery,
        };
        let llm = LLMContext {
            provider_client: &mut self.provider_client,
            config: &mut self.config,
            vt_cfg: None,
            context_manager: &mut self.context_manager,
            decision_ledger: &self.decision_ledger,
            traj: &self.traj,
        };
        let ui = UIContext {
            renderer: &mut self.renderer,
            handle: &self.handle,
            session: &mut self.session,
            ctrl_c_state: &self.ctrl_c_state,
            ctrl_c_notify: &self.ctrl_c_notify,
            lifecycle_hooks: None,
            default_placeholder: &self.default_placeholder,
            last_forced_redraw: &mut self.last_forced_redraw,
            input_status_state: &mut self.input_status_state,
        };
        let state = TurnProcessingState {
            session_stats: &mut self.session_stats,
            auto_exit_plan_mode_attempted: &mut self.auto_exit_plan_mode_attempted,
            mcp_panel_state: &mut self.mcp_panel_state,
            working_history: &mut self.working_history,
            turn_metadata_cache: &mut self.turn_metadata_cache,
            full_auto: false,
            harness_state: &mut self.harness_state,
            harness_emitter: None,
            steering_receiver: &mut self.steering_receiver,
        };

        TurnProcessingContext::from_parts(TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        })
    }
}
