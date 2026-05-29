use super::fallbacks::{
    build_validation_error_content_with_fallback, preflight_validation_fallback,
    recovery_fallback_for_tool,
};
use super::looping::{
    low_signal_family_key, shell_run_signature, spool_chunk_read_path,
    task_tracker_create_signature,
};
use super::recovery;
use super::{
    ToolOutcomeContext, ValidationResult, apply_reused_read_only_loop_metadata,
    build_tool_budget_exhausted_reason, build_tool_permissions_context,
    enforce_blocked_tool_call_guard, enforce_duplicate_task_tracker_create_guard,
    enforce_repeated_shell_run_guard, handle_prepared_tool_call, handle_single_tool_call,
    max_consecutive_blocked_tool_calls_per_turn, validate_tool_call,
};
use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_catalog::ToolCatalogState;
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
    TurnProcessingContextParts,
};
use crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
use anyhow::{Result, anyhow};
use hashbrown::HashMap;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Notify, RwLock};
use vtcode_config::core::PromptCachingConfig;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::types::{
    AgentConfig, ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
};
use vtcode_core::core::agent::runtime::RuntimeSteering;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter;
use vtcode_core::tools::circuit_breaker::CircuitBreaker;
use vtcode_core::tools::health::ToolHealthTracker;
use vtcode_core::tools::{ApprovalRecorder, ToolResultCache};
use vtcode_tui::app::{InlineHandle, InlineSession};

#[derive(Clone)]
struct NoopProvider;

#[async_trait::async_trait]
impl uni::LLMProvider for NoopProvider {
    fn name(&self) -> &str {
        "noop-provider"
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
            compaction: None,
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

struct TestContextBacking {
    _temp: tempfile::TempDir,
    sample_file: std::path::PathBuf,
    tool_registry: vtcode_core::tools::ToolRegistry,
    tools: Arc<RwLock<Vec<uni::ToolDefinition>>>,
    tool_result_cache: Arc<RwLock<ToolResultCache>>,
    tool_permission_cache: Arc<RwLock<ToolPermissionCache>>,
    permissions_state: Arc<RwLock<vtcode_core::config::PermissionsConfig>>,
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
    safety_validator: Arc<ToolCallSafetyValidator>,
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
    runtime_steering: RuntimeSteering,
    config: AgentConfig,
    provider_client: Box<dyn uni::LLMProvider>,
    traj: TrajectoryLogger,
    turn_metadata_cache: Option<Option<serde_json::Value>>,
}

impl TestContextBacking {
    async fn new(max_tool_calls: usize) -> Self {
        let temp = tempfile::TempDir::new().expect("temp workspace");
        let workspace = temp.path().to_path_buf();
        let sample_file = workspace.join("sample.txt");
        std::fs::write(&sample_file, "hello\n").expect("write sample file");

        let tool_registry = vtcode_core::tools::ToolRegistry::new(workspace.clone()).await;
        let tools = Arc::new(RwLock::new(Vec::new()));
        let tool_result_cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let tool_permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
        let permissions_state = Arc::new(RwLock::new(
            vtcode_core::config::PermissionsConfig::default(),
        ));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let approval_recorder = Arc::new(ApprovalRecorder::new(workspace.clone()));
        let session_stats = SessionStats::default();
        let mcp_panel_state = McpPanelState::default();
        let loaded_skills = Arc::new(RwLock::new(HashMap::new()));
        let context_manager = ContextManager::new(String::new(), (), loaded_skills, None);
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
        let safety_validator = Arc::new(ToolCallSafetyValidator::new());
        safety_validator.start_turn();
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
        let auto_exit_plan_mode_attempted = false;
        let working_history = Vec::new();
        let tool_catalog = Arc::new(ToolCatalogState::new());
        let default_placeholder = None;
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
        let provider_client: Box<dyn uni::LLMProvider> = Box::new(NoopProvider);
        let traj = TrajectoryLogger::disabled();
        let turn_metadata_cache = None;

        Self {
            _temp: temp,
            sample_file,
            tool_registry,
            tools,
            tool_result_cache,
            tool_permission_cache,
            permissions_state,
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
            auto_exit_plan_mode_attempted,
            working_history,
            tool_catalog,
            default_placeholder,
            runtime_steering: RuntimeSteering::default(),
            config,
            provider_client,
            traj,
            turn_metadata_cache,
        }
    }

    fn turn_processing_context(&mut self) -> TurnProcessingContext<'_> {
        let tool = crate::agent::runloop::unified::turn::context::ToolContext {
            tool_result_cache: &self.tool_result_cache,
            approval_recorder: &self.approval_recorder,
            tool_registry: &mut self.tool_registry,
            tools: &self.tools,
            tool_catalog: &self.tool_catalog,
            tool_permission_cache: &self.tool_permission_cache,
            permissions_state: &self.permissions_state,
            safety_validator: &self.safety_validator,
            circuit_breaker: &self.circuit_breaker,
            tool_health_tracker: &self.tool_health_tracker,
            rate_limiter: &self.rate_limiter,
            telemetry: &self.telemetry,
            autonomous_executor: &self.autonomous_executor,
            error_recovery: &self.error_recovery,
        };
        let llm = crate::agent::runloop::unified::turn::context::LLMContext {
            provider_client: &mut self.provider_client,
            config: &mut self.config,
            vt_cfg: None,
            context_manager: &mut self.context_manager,
            decision_ledger: &self.decision_ledger,
            traj: &self.traj,
        };
        let ui = crate::agent::runloop::unified::turn::context::UIContext {
            renderer: &mut self.renderer,
            handle: &self.handle,
            session: &mut self.session,
            active_thread_label: "main",
            ctrl_c_state: &self.ctrl_c_state,
            ctrl_c_notify: &self.ctrl_c_notify,
            lifecycle_hooks: None,
            default_placeholder: &self.default_placeholder,
            last_forced_redraw: &mut self.last_forced_redraw,
            input_status_state: &mut self.input_status_state,
        };
        let state = crate::agent::runloop::unified::turn::context::TurnProcessingState {
            session_stats: &mut self.session_stats,
            auto_exit_plan_mode_attempted: &mut self.auto_exit_plan_mode_attempted,
            mcp_panel_state: &mut self.mcp_panel_state,
            working_history: &mut self.working_history,
            turn_metadata_cache: &mut self.turn_metadata_cache,
            skip_confirmations: true,
            full_auto: false,
            harness_state: &mut self.harness_state,
            harness_emitter: None,
            runtime_steering: &mut self.runtime_steering,
        };

        TurnProcessingContext::from_parts(TurnProcessingContextParts {
            tool,
            llm,
            ui,
            state,
        })
    }
}

async fn cache_tool_permission(
    backing: &mut TestContextBacking,
    tool_name: &str,
    args: &serde_json::Value,
    grant: PermissionGrant,
) {
    let normalized_tool_name = backing
        .tool_registry
        .preflight_validate_call(tool_name, args)
        .expect("preflight should succeed")
        .normalized_tool_name;
    let mut cache = backing.tool_permission_cache.write().await;
    cache.cache_grant(normalized_tool_name, grant);
}

mod basic;
mod fallbacks;
mod runtime;
