use super::fallbacks::{
    build_validation_error_content_with_fallback, preflight_validation_fallback,
    recovery_fallback_for_tool,
};
use super::looping::{
    low_signal_family_key, shell_run_signature, spool_chunk_read_path,
    task_tracker_create_signature,
};
use super::{
    ToolOutcomeContext, build_tool_budget_exhausted_reason, handle_prepared_tool_call,
    handle_single_tool_call,
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

#[tokio::test]
async fn build_tool_permissions_context_propagates_skip_confirmations() {
    let mut backing = TestContextBacking::new(2).await;
    let mut ctx = backing.turn_processing_context();

    let permissions = super::build_tool_permissions_context(&mut ctx);
    assert!(permissions.skip_confirmations);
    drop(permissions);

    ctx.skip_confirmations = false;

    let permissions = super::build_tool_permissions_context(&mut ctx);
    assert!(!permissions.skip_confirmations);
}

#[test]
fn low_signal_family_for_unified_search_normalizes_missing_default_path() {
    let first = low_signal_family_key(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","pattern":"-> Result","globs":["**/*.rs"]}),
    );
    let second = low_signal_family_key(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","path":".","pattern":"Result<","globs":["**/*.rs"]}),
    );

    assert_eq!(first, second);
}

#[test]
fn spool_chunk_read_path_detects_spooled_read_calls() {
    let args = json!({
        "path": ".vtcode/context/tool_outputs/unified_exec_123.txt",
        "offset": 41,
        "limit": 40
    });
    let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
    assert_eq!(
        path,
        Some(".vtcode/context/tool_outputs/unified_exec_123.txt")
    );
}

#[test]
fn spool_chunk_read_path_ignores_regular_reads() {
    let args = json!({
        "path": "src/main.rs",
        "offset": 1,
        "limit": 100
    });
    let path = spool_chunk_read_path(tool_names::READ_FILE, &args);
    assert_eq!(path, None);
}

#[test]
fn preflight_fallback_normalizes_unified_search_args() {
    let error =
        anyhow!("Invalid arguments for tool 'unified_search': \"action\" is a required property");
    let args = json!({
        "Pattern": "LLMStreamEvent::",
        "Path": "."
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for recoverable unified_search preflight");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "LLMStreamEvent::");
}

#[test]
fn preflight_fallback_maps_keyword_to_pattern_for_grep() {
    let error = anyhow!("Invalid arguments for tool 'unified_search': missing field `pattern`");
    let args = json!({
        "action": "grep",
        "keyword": "system prompt",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for grep missing pattern");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "system prompt");
}

#[test]
fn preflight_fallback_remaps_unified_search_read_action() {
    let error = anyhow!("Tool execution failed: Invalid action: read");
    let args = json!({
        "action": "read",
        "query": "retry",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for invalid read action");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "retry");
}

#[test]
fn recovery_fallback_skips_list_degradation_for_search_refinement() {
    let grep = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","path":"src","pattern":"Result<"}),
    );
    let structural = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"structural","path":"src","pattern":"fn $NAME() {}","lang":"rust"}),
    );

    assert!(grep.is_none());
    assert!(structural.is_none());
}

#[test]
fn recovery_fallback_preserves_list_for_file_discovery_calls() {
    let fallback = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"list","path":"src","mode":"tree"}),
    )
    .expect("list fallback expected");

    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "list");
    assert_eq!(fallback.1["path"], "src");
    assert_eq!(fallback.1["mode"], "tree");
}

#[test]
fn preflight_fallback_remaps_unified_file_command_payload_to_unified_exec() {
    let error = anyhow!("Missing action in unified_file");
    let args = json!({
        "command": "git status --short",
        "cwd": "."
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_FILE, &args, &error)
        .expect("fallback expected for unified_file command payload");
    assert_eq!(fallback.0, tool_names::UNIFIED_EXEC);
    assert_eq!(fallback.1["action"], "run");
    assert_eq!(fallback.1["command"], "git status --short");
    assert_eq!(fallback.1["cwd"], ".");
}

#[test]
fn preflight_fallback_normalizes_request_user_input_single_question_shape() {
    let error = anyhow!(
        "Invalid arguments for tool 'request_user_input': \"questions\" is a required property"
    );
    let args = json!({
        "question": "Which direction should we take?",
        "header": "Scope",
        "options": [
            {"label": "Minimal", "description": "Smallest viable change"},
            {"label": "Full", "description": "Broader implementation"}
        ]
    });
    let fallback = preflight_validation_fallback(tool_names::REQUEST_USER_INPUT, &args, &error)
        .expect("fallback expected for request_user_input shorthand");
    assert_eq!(fallback.0, tool_names::REQUEST_USER_INPUT);
    assert_eq!(fallback.1["questions"][0]["id"], "question_1");
    assert_eq!(fallback.1["questions"][0]["header"], "Scope");
    assert_eq!(
        fallback.1["questions"][0]["question"],
        "Which direction should we take?"
    );
    assert_eq!(
        fallback.1["questions"][0]["options"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn preflight_fallback_normalizes_request_user_input_tabs_shape() {
    let error = anyhow!(
        "Invalid arguments for tool 'request_user_input': additional properties are not allowed"
    );
    let args = json!({
        "question": "Which area should we prioritize first?",
        "tabs": [
            {
                "id": "priority",
                "title": "Priority",
                "items": [
                    {"title": "Reliability", "subtitle": "Reduce failure modes"},
                    {"title": "UX", "subtitle": "Improve user flow"}
                ]
            }
        ]
    });
    let fallback = preflight_validation_fallback(tool_names::REQUEST_USER_INPUT, &args, &error)
        .expect("fallback expected for request_user_input tabbed payload");
    assert_eq!(fallback.0, tool_names::REQUEST_USER_INPUT);
    assert_eq!(fallback.1["questions"][0]["id"], "priority");
    assert_eq!(fallback.1["questions"][0]["header"], "Priority");
    assert_eq!(
        fallback.1["questions"][0]["question"],
        "Which area should we prioritize first?"
    );
    assert_eq!(
        fallback.1["questions"][0]["options"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn validation_error_payload_includes_fallback_metadata() {
    let payload = build_validation_error_content_with_fallback(
        "Tool preflight validation failed: x".to_string(),
        "preflight",
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(json!({"action":"grep","pattern":"foo","path":"."})),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("validation payload should be json");
    assert_eq!(parsed["error_class"], "invalid_arguments");
    assert_eq!(parsed["is_recoverable"], true);
    assert_eq!(parsed["fallback_tool"], tool_names::UNIFIED_SEARCH);
    assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
    assert_eq!(
        parsed.get("next_action"),
        Some(&json!("Retry with fallback_tool_args."))
    );
    assert!(parsed.get("loop_detected").is_none());
}

#[test]
fn validation_error_payload_marks_loop_detection_without_prose_hint() {
    let payload = build_validation_error_content_with_fallback(
        "Tool 'read_file' is blocked due to excessive repetition (Loop Detected).".to_string(),
        "loop_detection",
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(json!({"action":"list","path":"."})),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("validation payload should be json");
    assert_eq!(parsed.get("loop_detected"), Some(&json!(true)));
    assert_eq!(parsed["fallback_tool"], tool_names::UNIFIED_SEARCH);
    assert_eq!(parsed["fallback_tool_args"]["action"], "list");
    assert_eq!(
        parsed.get("next_action"),
        Some(&json!("Retry with fallback_tool_args."))
    );
}

#[test]
fn reused_read_only_result_uses_canonical_guidance() {
    let mut payload = json!({
        "output": "preview",
        "content": "preview",
        "stdout": "preview",
        "stderr": "preview",
        "stderr_preview": "preview"
    });

    super::apply_reused_read_only_loop_metadata(
        payload
            .as_object_mut()
            .expect("payload should be an object for reuse metadata"),
    );

    assert_eq!(payload.get("reused_recent_result"), Some(&json!(true)));
    assert_eq!(payload.get("result_ref_only"), Some(&json!(true)));
    assert_eq!(payload.get("loop_detected"), Some(&json!(true)));
    assert_eq!(
        payload.get("loop_detected_note"),
        Some(&json!(
            "Loop detected on repeated read-only call; reusing recent output. Use unified_search (action='grep') or summarize before another read."
        ))
    );
    assert_eq!(
        payload.get("next_action"),
        Some(&json!(
            "Use unified_search (action='grep') or retry unified_file with a narrower offset/limit before reading again."
        ))
    );
    assert!(payload.get("output").is_none());
    assert!(payload.get("content").is_none());
    assert!(payload.get("stdout").is_none());
    assert!(payload.get("stderr").is_none());
    assert!(payload.get("stderr_preview").is_none());
}

#[test]
fn task_tracker_create_signature_matches_identical_payloads() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_eq!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_payload_changes() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_title_change() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings later",
        "items": ["A", "B"],
        "notes": "n"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_differs_for_notes_change() {
    let first = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "n"
    });
    let second = json!({
        "action": "create",
        "title": "Fix clippy warnings",
        "items": ["A", "B"],
        "notes": "updated"
    });
    let sig1 = task_tracker_create_signature(tool_names::TASK_TRACKER, &first);
    let sig2 = task_tracker_create_signature(tool_names::TASK_TRACKER, &second);
    assert_ne!(sig1, sig2);
}

#[test]
fn task_tracker_create_signature_ignores_non_create_calls() {
    let args = json!({
        "action": "update",
        "index": 1,
        "status": "completed"
    });
    let sig = task_tracker_create_signature(tool_names::TASK_TRACKER, &args);
    assert!(sig.is_none());
}

#[test]
fn shell_run_signature_normalizes_run_pty_command_and_args() {
    let args = json!({
        "command": "  cargo   check  ",
        "args": ["-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::RUN_PTY_CMD, &args);
    assert_eq!(
        signature,
        Some("unified_exec::cargo check -p vtcode-core".to_string())
    );
}

#[test]
fn shell_run_signature_handles_unified_exec_run_action() {
    let args = json!({
        "action": "run",
        "command": ["cargo", "check", "-p", "vtcode-core"]
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert_eq!(
        signature,
        Some("unified_exec::cargo check -p vtcode-core".to_string())
    );
}

#[test]
fn shell_run_signature_normalizes_trivial_shell_quoting_differences() {
    let first = shell_run_signature(
        tool_names::UNIFIED_EXEC,
        &json!({
            "action": "run",
            "command": "grep -n '-> Result' vtcode-tui/src/**/*.rs"
        }),
    );
    let second = shell_run_signature(
        tool_names::UNIFIED_EXEC,
        &json!({
            "action": "run",
            "command": "grep -n \"-> Result\" vtcode-tui/src/**/*.rs"
        }),
    );

    assert_eq!(first, second);
}

#[test]
fn shell_run_signature_ignores_non_run_unified_exec_action() {
    let args = json!({
        "action": "poll",
        "session_id": "run-123"
    });
    let signature = shell_run_signature(tool_names::UNIFIED_EXEC, &args);
    assert!(signature.is_none());
}

#[test]
fn tool_budget_exhausted_reason_mentions_new_instruction_option() {
    let reason = build_tool_budget_exhausted_reason(32, 32);
    assert!(reason.contains("\"continue\" or provide a new instruction"));
}

#[tokio::test]
async fn blocked_tool_call_guard_emits_tool_and_system_messages() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = super::max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    let mut outcome = None;
    for idx in 0..=max_streak {
        outcome = super::enforce_blocked_tool_call_guard(
            &mut ctx,
            &format!("blocked_{idx}"),
            tool_names::READ_FILE,
            &args,
        );
    }

    assert!(matches!(
        outcome,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("blocked_streak"))
    );
    assert!(ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains("Consecutive blocked tool calls reached per-turn cap")
    }));
}

#[tokio::test]
async fn blocked_tool_call_guard_short_circuits_to_recovery_when_active() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path":"src/main.rs"});
    ctx.activate_recovery("loop detector");

    let outcome = super::enforce_blocked_tool_call_guard(
        &mut ctx,
        "blocked_recovery",
        tool_names::READ_FILE,
        &args,
    );

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}

#[tokio::test]
async fn unified_validation_ignores_preseeded_legacy_loop_detector_state() {
    let mut backing = TestContextBacking::new(2).await;
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

    let legacy_detector = backing.autonomous_executor.loop_detector();
    {
        let mut detector = legacy_detector
            .write()
            .expect("legacy loop detector should lock");
        detector.set_tool_limit(tool_names::READ_FILE, 2);
        let seeded_args = json!({"path": valid_file.to_string_lossy()});
        assert!(
            detector
                .record_call(tool_names::READ_FILE, &seeded_args)
                .is_none()
        );
        let _ = detector.record_call(tool_names::READ_FILE, &seeded_args);
        let warning = detector.record_call(tool_names::READ_FILE, &seeded_args);
        assert!(warning.is_some());
        assert!(detector.is_hard_limit_exceeded(tool_names::READ_FILE));
    }

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_single_tool_call(
        &mut outcome_ctx,
        "legacy_detector_seeded",
        tool_names::READ_FILE,
        valid_args,
    )
    .await
    .expect("unified validation should ignore legacy detector state");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Loop detector stopped repeated")
    }));
    assert!(
        legacy_detector
            .read()
            .expect("legacy loop detector should lock")
            .is_hard_limit_exceeded(tool_names::READ_FILE)
    );
}

#[tokio::test]
async fn repeated_shell_guard_activates_recovery_without_breaking_turn() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_repeated_runs = ctx
        .vt_cfg
        .map(|cfg| cfg.tools.max_repeated_tool_calls)
        .filter(|value| *value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);
    let args = json!({"action":"run","command":"cargo check"});

    let mut outcome = None;
    for idx in 0..=max_repeated_runs {
        outcome = super::enforce_repeated_shell_run_guard(
            &mut ctx,
            &format!("shell_{idx}"),
            tool_names::UNIFIED_EXEC,
            &args,
        );
    }

    assert!(matches!(outcome, Some(super::ValidationResult::Blocked)));
    assert!(ctx.is_recovery_active());
}

#[tokio::test]
async fn duplicate_task_tracker_create_is_blocked_not_breaking() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({
        "action": "create",
        "title": "Task Checklist",
        "items": ["step 1"]
    });

    let first = super::enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_first",
        tool_names::TASK_TRACKER,
        &args,
    );
    assert!(first.is_none());

    let second = super::enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_second",
        tool_names::TASK_TRACKER,
        &args,
    );
    assert!(matches!(second, Some(super::ValidationResult::Blocked)));
}

#[tokio::test]
async fn enter_plan_mode_clears_task_tracker_create_signatures() {
    let mut backing = TestContextBacking::new(4).await;
    let enter_args = json!({});
    cache_tool_permission(
        &mut backing,
        tool_names::ENTER_PLAN_MODE,
        &enter_args,
        PermissionGrant::Permanent,
    )
    .await;

    let mut ctx = backing.turn_processing_context();
    let create_args = json!({
        "action": "create",
        "title": "Task Checklist",
        "items": ["step 1"]
    });
    let first = super::enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_seed",
        tool_names::TASK_TRACKER,
        &create_args,
    );
    assert!(first.is_none());

    let result = super::validate_tool_call(
        &mut ctx,
        "enter_plan_mode_call",
        tool_names::ENTER_PLAN_MODE,
        &enter_args,
    )
    .await
    .expect("validate enter_plan_mode");
    assert!(matches!(result, super::ValidationResult::Proceed(_)));

    let second = super::enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_after_plan",
        tool_names::TASK_TRACKER,
        &create_args,
    );
    assert!(second.is_none());
}

#[tokio::test]
async fn recovery_skip_step_pushes_structured_tool_message() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    let outcome = super::recovery::apply_recovery_action(
        &mut ctx,
        "recovery_call",
        crate::agent::runloop::unified::turn::recovery_flow::RecoveryAction::SkipStep,
    )
    .await
    .expect("skip-step recovery should succeed");

    assert!(matches!(outcome, Some(super::ValidationResult::Handled)));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("\"skipped\":true") })
    );
}

#[tokio::test]
async fn repeated_identical_readonly_call_in_same_turn_reuses_recent_result() {
    let mut backing = TestContextBacking::new(4).await;
    let args = json!({
        "action": "read",
        "path": backing.sample_file.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(
        &mut outcome_ctx,
        "read_once",
        tool_names::UNIFIED_FILE,
        args.clone(),
    )
    .await
    .expect("first readonly call should succeed");

    assert!(first.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);

    let second = handle_single_tool_call(
        &mut outcome_ctx,
        "read_twice",
        tool_names::UNIFIED_FILE,
        args,
    )
    .await
    .expect("duplicate readonly call should be reused");

    assert!(second.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("\"reused_recent_result\":true")
    }));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("\"result_ref_only\":true")
    }));
}

#[tokio::test]
async fn repeated_same_file_read_variants_activate_recovery_before_fourth_read() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();

    let read_args = vec![
        json!({
            "action": "read",
            "path": sample_path.clone()
        }),
        json!({
            "action": "read",
            "path": sample_path.clone(),
            "line_start": 1,
            "line_end": 1
        }),
        json!({
            "action": "read",
            "path": sample_path.clone(),
            "offset": 0,
            "limit": 1
        }),
        json!({
            "action": "read",
            "path": sample_path.clone(),
            "line_start": 1,
            "line_end": 2
        }),
    ];

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    for (idx, args) in read_args.into_iter().enumerate().take(3) {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("read_variant_{idx}"),
            tool_names::UNIFIED_FILE,
            args,
        )
        .await
        .expect("read variant should complete");

        assert!(outcome.is_none());
    }

    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 3);
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .consecutive_same_file_read_family_calls,
        3
    );

    let blocked = handle_single_tool_call(
        &mut outcome_ctx,
        "read_variant_blocked",
        tool_names::UNIFIED_FILE,
        json!({
            "action": "read",
            "path": sample_path,
            "line_start": 1,
            "line_end": 2
        }),
    )
    .await
    .expect("fourth read attempt should be handled");

    assert!(matches!(blocked, Some(TurnHandlerOutcome::Continue)));
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 3);
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .consecutive_same_file_read_family_calls,
        4
    );
    assert!(outcome_ctx.ctx.is_recovery_active());
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("repeated_read_family") })
    );
}

#[tokio::test]
async fn denied_tool_permission_emits_policy_response_without_budget_burn() {
    let mut backing = TestContextBacking::new(2).await;
    let valid_file = backing.sample_file.clone();
    let denial_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &denial_args,
        PermissionGrant::Denied,
    )
    .await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_single_tool_call(
        &mut outcome_ctx,
        "denied",
        tool_names::READ_FILE,
        denial_args,
    )
    .await
    .expect("denied permission should be handled");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("execution denied by policy")
    }));
}

#[tokio::test]
async fn prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

    let tool_call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        "prepared_read".to_string(),
        tool_names::READ_FILE.to_string(),
        serde_json::to_string(&valid_args).expect("serialize tool args"),
    ));

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_prepared_tool_call(&mut outcome_ctx, &tool_call)
        .await
        .expect("prepared tool call should execute");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(!outcome_ctx.ctx.harness_state.tool_budget_exhausted());
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("exceeded max tool calls per turn")
    }));
}

#[tokio::test]
async fn multiple_prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    let second_file = backing
        .sample_file
        .parent()
        .expect("temp workspace root")
        .join("other.txt");
    std::fs::write(&second_file, "world\n").expect("write second sample file");

    let tool_calls = vec![
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_search_1".to_string(),
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::to_string(&json!({
                "action": "grep",
                "path": ".",
                "pattern": "hello"
            }))
            .expect("serialize tool args"),
        )),
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_search_2".to_string(),
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::to_string(&json!({
                "action": "grep",
                "path": ".",
                "pattern": "world"
            }))
            .expect("serialize tool args"),
        )),
    ];

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &tool_calls)
        .await
        .expect("prepared tool calls should execute");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 2);
    assert!(!outcome_ctx.ctx.harness_state.tool_budget_exhausted());
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("exceeded max tool calls per turn")
    }));
}

#[tokio::test]
async fn end_to_end_blocked_calls_do_not_burn_budget_before_valid_call() {
    let mut backing = TestContextBacking::new(1).await;
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

    let mut turn_modified_files: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut tp_ctx = backing.turn_processing_context();

    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let blocked_args = json!({"path":"/var/db/shadow"});
    let first = handle_single_tool_call(
        &mut outcome_ctx,
        "blocked_1",
        tool_names::READ_FILE,
        blocked_args.clone(),
    )
    .await
    .expect("first blocked call should not fail hard");
    assert!(first.is_none());

    let second = handle_single_tool_call(
        &mut outcome_ctx,
        "blocked_2",
        tool_names::READ_FILE,
        blocked_args,
    )
    .await
    .expect("second blocked call should not fail hard");
    assert!(second.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(!outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let third = handle_single_tool_call(
        &mut outcome_ctx,
        "valid_1",
        tool_names::READ_FILE,
        valid_args.clone(),
    )
    .await
    .expect("valid call should execute");
    assert!(third.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let exhausted = handle_single_tool_call(
        &mut outcome_ctx,
        "exhausted",
        tool_names::READ_FILE,
        valid_args,
    )
    .await
    .expect("exhausted-budget call should return structured outcome");
    assert!(matches!(
        exhausted,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains("\"continue\" or provide a new instruction")
    }));
}
