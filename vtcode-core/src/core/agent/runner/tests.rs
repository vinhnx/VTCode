use super::{AgentRunner, RunnerSettings};
use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::CapabilityLevel;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::runtime::AgentRuntime;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::state::TaskRunState;
use crate::core::agent::state::record_turn_duration;
use crate::core::agent::steering::SteeringMessage;
use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::threads::ThreadBootstrap;
use crate::exec::events::{
    HarnessEventKind, ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus,
};
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, ToolCall,
};
use crate::tools::Tool;
use crate::tools::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
use crate::tools::handlers::{PlanModeState, TaskTrackerTool};
use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::json;
use std::collections::VecDeque;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;

#[test]
fn record_turn_duration_records_once() {
    let mut durations = Vec::with_capacity(5);
    let mut total_ms = 0u128;
    let mut max_ms = 0u128;
    let mut count = 0usize;
    let mut recorded = false;
    let start = Instant::now();

    record_turn_duration(
        &mut durations,
        &mut total_ms,
        &mut max_ms,
        &mut count,
        &mut recorded,
        &start,
    );
    record_turn_duration(
        &mut durations,
        &mut total_ms,
        &mut max_ms,
        &mut count,
        &mut recorded,
        &start,
    );

    assert_eq!(durations.len(), 1);
    assert_eq!(count, 1);
}

#[test]
fn finalize_outcome_marks_success() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.has_completed = true;
    state.turns_executed = 2;

    state.finalize_outcome(4);

    assert_eq!(state.completion_outcome, TaskOutcome::Success);
}

#[test]
fn finalize_outcome_turn_limit() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.turns_executed = 6;

    state.finalize_outcome(6);

    assert!(matches!(
        state.completion_outcome,
        TaskOutcome::TurnLimitReached { .. }
    ));
}

#[test]
fn finalize_outcome_tool_loop_limit() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 2, 10000);
    state.turns_executed = 2;
    state.tool_loop_limit_hit = true;

    state.finalize_outcome(10);

    assert_eq!(
        state.completion_outcome,
        TaskOutcome::tool_loop_limit_reached(state.max_tool_loops, state.consecutive_tool_loops)
    );
}

#[test]
fn into_results_computes_metrics() {
    let mut state = TaskRunState::new(Vec::new(), Vec::new(), 5, 10000);
    state.turn_durations_ms = vec![100, 200, 300];
    state.turn_total_ms = 600;
    state.turn_max_ms = 300;
    state.turn_count = 3;
    state.turns_executed = 3;
    state.completion_outcome = TaskOutcome::Success;
    state.modified_files = vec!["file.rs".to_owned()];
    state.executed_commands = vec!["write_file".to_owned()];
    state.warnings = vec!["warning".to_owned()];

    let total_duration_ms = 1_000u128;
    let results = state.into_results("summary".to_owned(), Vec::new(), total_duration_ms);

    assert_eq!(results.outcome, TaskOutcome::Success);
    assert_eq!(results.turns_executed, 3);
    assert_eq!(results.total_duration_ms, total_duration_ms);
    assert_eq!(results.max_turn_duration_ms, Some(300));
    assert_eq!(results.average_turn_duration_ms, Some(200.0));
    assert_eq!(results.modified_files, vec!["file.rs".to_owned()]);
    assert_eq!(results.executed_commands, vec!["write_file".to_owned()]);
    assert_eq!(results.summary, "summary");
    assert_eq!(results.warnings, vec!["warning".to_owned()]);
}

#[tokio::test]
async fn full_auto_allowlist_hides_tools_from_exposure() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-allowlist".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    assert!(runner.is_tool_exposed(tools::UNIFIED_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);
}

#[tokio::test]
async fn runner_uses_public_tool_resolution_for_validation() {
    let temp = TempDir::new().expect("tempdir");
    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-validation".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    assert!(runner.is_valid_tool(tools::READ_FILE).await);
    assert!(runner.is_valid_tool("Exec code").await);
    assert!(!runner.is_valid_tool("exec_code").await);
}

#[tokio::test]
async fn build_universal_tools_matches_registry_agent_runner_snapshot() {
    let temp = TempDir::new().expect("tempdir");
    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-snapshot".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    let registry_tools = runner
        .tool_registry
        .model_tools(SessionToolsConfig {
            surface: SessionSurface::AgentRunner,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: runner.config().agent.tool_documentation_mode,
            plan_mode: runner.tool_registry.is_plan_mode(),
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::for_model_name(&runner.model),
            deferred_tool_policy: crate::tools::handlers::deferred_tool_policy_for_runtime(
                crate::llm::factory::infer_provider(
                    Some(&runner.config().agent.provider),
                    &runner.model,
                ),
                runner
                    .provider_client
                    .supports_responses_compaction(&runner.model),
                Some(runner.config()),
            ),
        })
        .await;
    let mut expected = Vec::new();
    for tool in registry_tools {
        if runner.is_tool_exposed(tool.function_name()).await {
            expected.push(tool.function_name().to_string());
        }
    }

    let actual = runner
        .build_universal_tools()
        .await
        .expect("universal tools")
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();

    assert_eq!(actual, expected);
}

#[tokio::test]
async fn build_universal_tools_uses_override_when_present() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-override".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    runner.set_tool_definitions_override(vec![crate::llm::provider::ToolDefinition::function(
        "only_tool".to_string(),
        "Only tool".to_string(),
        json!({ "type": "object" }),
    )]);

    let tools = runner.build_universal_tools().await.expect("tool override");

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_name(), "only_tool");
}

#[tokio::test]
async fn normalize_tool_args_applies_transform_after_defaults() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-transform".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");
    runner.set_tool_arg_transform(Arc::new(|name, value| {
        let mut obj = value.as_object().cloned().expect("object args");
        obj.insert("tool_name".to_string(), json!(name));
        serde_json::Value::Object(obj)
    }));

    let mut state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
    state.last_dir_path = Some(temp.path().display().to_string());
    let normalized = runner.normalize_tool_args(
        tools::UNIFIED_SEARCH,
        &json!({"action": "list"}),
        &mut state,
    );

    assert_eq!(normalized["tool_name"], tools::UNIFIED_SEARCH);
    assert_eq!(normalized["path"], json!(temp.path().display().to_string()));
}

#[tokio::test]
async fn new_with_preloaded_config_uses_override_snapshot() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(
        temp.path().join("vtcode.toml"),
        "[agent]\nprovider = \"openai\"\n",
    )
    .expect("workspace config");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "anthropic".to_string();

    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-test".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        vt_cfg,
    )
    .await
    .expect("runner");

    assert_eq!(runner.core_agent_config().provider, "anthropic");
}

#[tokio::test]
async fn core_agent_config_normalizes_api_key_env_and_checkpoint_dir() {
    let temp = TempDir::new().expect("tempdir");
    let absolute_checkpoint_dir = temp.path().join("snapshots-absolute");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "minimax".to_string();
    vt_cfg.agent.api_key_env = crate::config::constants::defaults::DEFAULT_API_KEY_ENV.to_string();
    vt_cfg.agent.checkpointing.storage_dir = Some(absolute_checkpoint_dir.display().to_string());

    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-test-normalized-config".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        vt_cfg,
    )
    .await
    .expect("runner");

    let config = runner.core_agent_config();
    assert_eq!(config.api_key_env, "MINIMAX_API_KEY");
    assert_eq!(
        config.checkpointing_storage_dir,
        Some(absolute_checkpoint_dir)
    );
}

#[tokio::test]
async fn runner_uses_configured_provider_for_huggingface_repo_models() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.provider = "huggingface".to_string();
    vt_cfg.agent.default_model =
        crate::config::constants::models::huggingface::ZAI_GLM_5_NOVITA.to_string();

    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::HuggingFaceGlm5Novita,
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-huggingface-provider".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        vt_cfg,
    )
    .await
    .expect("runner");

    assert_eq!(runner.provider_client.name(), "huggingface");
}

#[tokio::test]
async fn review_tool_allowlist_excludes_mutating_and_plan_only_tools() {
    let temp = TempDir::new().expect("tempdir");
    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-review".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    let allowlist = runner
        .review_tool_allowlist(&[
            tools::UNIFIED_FILE.to_string(),
            tools::UNIFIED_EXEC.to_string(),
            "task_tracker".to_string(),
            "plan_task_tracker".to_string(),
            "enter_plan_mode".to_string(),
        ])
        .await;

    assert_eq!(allowlist, vec![tools::UNIFIED_FILE.to_string()]);
}

#[tokio::test]
async fn review_tool_allowlist_expands_wildcard_read_only() {
    let temp = TempDir::new().expect("tempdir");
    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-review-wildcard".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    let allowlist = runner
        .review_tool_allowlist(&[tools::WILDCARD_ALL.to_string()])
        .await;

    assert!(allowlist.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(allowlist.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!allowlist.iter().any(|tool| tool == tools::UNIFIED_EXEC));
}

#[tokio::test]
async fn validate_and_normalize_tool_name_matches_public_registry_resolution() {
    let temp = TempDir::new().expect("tempdir");
    let runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-normalization".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        VTCodeConfig::default(),
    )
    .await
    .expect("runner");

    let normalized = runner
        .validate_and_normalize_tool_name("Exec code", &json!({"command": "echo vtcode"}))
        .expect("humanized exec label should resolve");
    assert_eq!(normalized, tools::UNIFIED_EXEC);

    let err = runner
        .validate_and_normalize_tool_name("exec_code", &json!({"command": "echo vtcode"}))
        .expect_err("removed alias should stay rejected");
    assert!(err.to_string().contains("Unknown tool"));
}

#[derive(Clone)]
struct QueuedProvider {
    responses: Arc<Mutex<VecDeque<LLMResponse>>>,
}

impl QueuedProvider {
    fn new(responses: Vec<LLMResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
        }
    }
}

#[async_trait]
impl LLMProvider for QueuedProvider {
    fn name(&self) -> &str {
        "queued-test-provider"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.responses
            .lock()
            .pop_front()
            .ok_or(LLMError::InvalidRequest {
                message: "QueuedProvider has no queued responses".to_string(),
                metadata: None,
            })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.3-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[derive(Clone)]
struct RecordingQueuedProvider {
    responses: Arc<Mutex<VecDeque<LLMResponse>>>,
    requests: Arc<Mutex<Vec<LLMRequest>>>,
}

impl RecordingQueuedProvider {
    fn new(responses: Vec<LLMResponse>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn recorded_requests(&self) -> Vec<LLMRequest> {
        self.requests.lock().clone()
    }
}

#[async_trait]
impl LLMProvider for RecordingQueuedProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.requests.lock().push(request);
        self.responses
            .lock()
            .pop_front()
            .ok_or(LLMError::InvalidRequest {
                message: "RecordingQueuedProvider has no queued responses".to_string(),
                metadata: None,
            })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.3-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

fn task(title: &str, id: &str) -> Task {
    Task {
        id: id.to_string(),
        title: title.to_string(),
        description: title.to_string(),
        instructions: None,
    }
}

fn text_response(text: &str) -> LLMResponse {
    LLMResponse {
        content: Some(text.to_string()),
        model: "gpt-5.3-codex".to_string(),
        finish_reason: FinishReason::Stop,
        ..Default::default()
    }
}

fn json_response(value: serde_json::Value) -> LLMResponse {
    text_response(&value.to_string())
}

fn tool_call_response(tool_name: &str, args: serde_json::Value) -> LLMResponse {
    LLMResponse {
        content: Some(format!("Calling {tool_name}")),
        tool_calls: Some(vec![ToolCall::function(
            "call-1".to_string(),
            tool_name.to_string(),
            args.to_string(),
        )]),
        model: "gpt-5.3-codex".to_string(),
        finish_reason: FinishReason::ToolCalls,
        ..Default::default()
    }
}

fn planner_response_json(verify_command: &str) -> serde_json::Value {
    json!({
        "spec_markdown": "# Execution Spec\n\nImplement the requested change with a resumable tracker.",
        "task_title": "Planner seeded task",
        "items": [{
            "description": "Implement the requested change",
            "outcome": "The requested change is implemented and tracked.",
            "verify": [verify_command],
        }]
    })
}

fn evaluator_response_json(
    verdict: &str,
    summary: &str,
    high_severity_findings: usize,
) -> serde_json::Value {
    json!({
        "verdict": verdict,
        "summary": summary,
        "high_severity_findings": high_severity_findings,
        "findings": [{
            "severity": if high_severity_findings > 0 { "high" } else { "low" },
            "title": summary,
            "detail": summary,
        }],
        "unmet_contract_items": [],
        "residual_risks": [],
        "required_tracker_updates": [],
    })
}

fn tool_call_response_with_request_id(
    tool_name: &str,
    args: serde_json::Value,
    request_id: &str,
) -> LLMResponse {
    let mut response = tool_call_response(tool_name, args);
    response.request_id = Some(request_id.to_string());
    response
}

fn workspace_root(temp: &TempDir) -> PathBuf {
    temp.path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf())
}

async fn make_runner(temp: &TempDir, vt_cfg: VTCodeConfig, session_id: &str) -> AgentRunner {
    let mut runner = AgentRunner::new_with_thread_bootstrap_and_config(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        workspace_root(temp),
        session_id.to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        vt_cfg,
    )
    .await
    .expect("runner");
    runner.set_quiet(true);
    runner
}

async fn seed_tracker(workspace_root: &Path, items: serde_json::Value) {
    let tool = TaskTrackerTool::new(
        workspace_root.to_path_buf(),
        PlanModeState::new(workspace_root.to_path_buf()),
    );
    tool.execute(json!({
        "action": "create",
        "title": "Harness hardening",
        "items": items,
    }))
    .await
    .expect("seed tracker");
}

fn turn_started_count(results: &TaskResults) -> usize {
    results
        .thread_events
        .iter()
        .filter(|event| matches!(event, ThreadEvent::TurnStarted(_)))
        .count()
}

fn harness_events(results: &TaskResults) -> Vec<HarnessEventKind> {
    results
        .thread_events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::Harness(harness) => Some(harness.event.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn harness_paths(results: &TaskResults, kind: HarnessEventKind) -> Vec<String> {
    results
        .thread_events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::Harness(harness) if harness.event == kind => {
                    harness.path.clone()
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn completed_tool_invocation_item_id(events: &[ThreadEvent], tool_call_id: &str) -> Option<String> {
    events.iter().find_map(|event| match event {
        ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
            ThreadItemDetails::ToolInvocation(details)
                if details.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(item.id.clone())
            }
            _ => None,
        },
        _ => None,
    })
}

fn completed_tool_output_count(
    events: &[ThreadEvent],
    tool_call_id: &str,
    status: ToolCallStatus,
    call_item_id: &str,
) -> usize {
    events
        .iter()
        .filter(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::ToolOutput(details) => {
                    details.tool_call_id.as_deref() == Some(tool_call_id)
                        && details.status == status
                        && details.call_id == call_item_id
                }
                _ => false,
            },
            _ => false,
        })
        .count()
}

#[tokio::test]
async fn exec_full_auto_continues_until_tracker_is_completed() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Finish tracker step"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.automation.full_auto.max_turns = 3;
    let mut runner = make_runner(&temp, vt_cfg, "thread-continuation-success").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        text_response("The task is complete."),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("I have finished all the work."),
    ]));

    let result = runner
        .execute_task(&task("Harness continuation", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed > 1);
    assert!(harness_events(&result).contains(&HarnessEventKind::ContinuationStarted));

    let tracker =
        fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("- [x] Finish tracker step"));
}

#[tokio::test]
async fn runner_reuses_openai_response_chain_and_session_cache_key() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Cache-aware tracker step"])).await;

    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-cache-lineage").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    let provider = RecordingQueuedProvider::new(vec![
        tool_call_response_with_request_id(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
            "resp_first_turn",
        ),
        text_response("All work is complete."),
        text_response("All work is complete."),
        text_response("All work is complete."),
    ]);
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result = runner
        .execute_task(&task("Cache-aware continuation", "exec-task"), &[])
        .await
        .expect("task result");

    assert!(result.turns_executed >= 2);

    let requests = recorded.recorded_requests();
    assert!(requests.len() >= 2);
    assert_eq!(requests[0].previous_response_id, None);
    assert_eq!(
        requests[0].prompt_cache_key.as_deref(),
        Some("vtcode:openai:thread-cache-lineage")
    );
    assert_eq!(
        requests[1].previous_response_id.as_deref(),
        Some("resp_first_turn")
    );
    assert_eq!(requests[1].prompt_cache_key, requests[0].prompt_cache_key);
}

#[tokio::test]
async fn exec_full_auto_runs_verification_before_accepting_completion() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(
        &workspace,
        json!([{
            "description": "Verify harness",
            "status": "completed",
            "verify": "pwd",
        }]),
    )
    .await;

    let mut runner = make_runner(
        &temp,
        VTCodeConfig::default(),
        "thread-verification-success",
    )
    .await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

    let result = runner
        .execute_task(&task("Verification success", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::VerificationStarted));
    assert!(events.contains(&HarnessEventKind::VerificationPassed));
}

#[tokio::test]
async fn exec_full_auto_retries_after_verification_failure() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(
        &workspace,
        json!([{
            "description": "Verify harness",
            "status": "completed",
            "verify": "cat missing-verification-target",
        }]),
    )
    .await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.automation.full_auto.max_turns = 2;
    let mut runner = make_runner(&temp, vt_cfg, "thread-verification-failure").await;
    runner.enable_full_auto(&[]).await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        text_response("The task is complete."),
        text_response("Task is now complete."),
    ]));

    let result = runner
        .execute_task(&task("Verification failure", "exec-task"), &[])
        .await
        .expect("task result");

    assert!(matches!(
        result.outcome,
        TaskOutcome::TurnLimitReached { .. }
    ));
    assert!(result.turns_executed > 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::VerificationStarted));
    assert!(events.contains(&HarnessEventKind::VerificationFailed));
    assert!(events.contains(&HarnessEventKind::ContinuationStarted));
}

#[tokio::test]
async fn review_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-review-skip").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

    let result = runner
        .execute_task(&task("Review task", "review-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}

#[tokio::test]
async fn plan_mode_runs_skip_continuation_and_finish_single_pass() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-plan-mode-skip").await;
    runner.enable_full_auto(&[]).await;
    runner.enable_plan_mode();
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

    let result = runner
        .execute_task(&task("Plan mode task", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}

#[tokio::test]
async fn exec_only_policy_skips_when_full_auto_is_disabled() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.continuation_policy =
        vtcode_config::core::agent::ContinuationPolicy::ExecOnly;
    let mut runner = make_runner(&temp, vt_cfg, "thread-exec-only-skip").await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![text_response(
        "The task is complete.",
    )]));

    let result = runner
        .execute_task(&task("Exec task", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(result.turns_executed >= 1);
    assert_eq!(turn_started_count(&result), 1);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::ContinuationSkipped));
    assert!(!events.contains(&HarnessEventKind::ContinuationStarted));
}

#[tokio::test]
async fn tool_loop_limit_writes_blocked_handoff_artifacts() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);
    seed_tracker(&workspace, json!(["Investigate loop"])).await;

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.automation.full_auto.max_turns = 1;
    vt_cfg.tools.max_tool_loops = 1;
    let mut runner = make_runner(&temp, vt_cfg, "thread-tool-loop-blocked").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![tool_call_response(
        tools::TASK_TRACKER,
        json!({
            "action": "list"
        }),
    )]));

    let result = runner
        .execute_task(&task("Loop blocked", "exec-task"), &[])
        .await
        .expect("task result");

    assert!(matches!(
        result.outcome,
        TaskOutcome::ToolLoopLimitReached { .. }
    ));
    let paths = harness_paths(&result, HarnessEventKind::BlockedHandoffWritten);
    assert_eq!(paths.len(), 2);
    for path in paths {
        let content = fs::read_to_string(&path).expect("blocked handoff file");
        assert!(content.contains("tool_loop_limit_reached"));
        assert!(content.contains("Stopped after reaching tool loop limit"));
    }
}

#[tokio::test]
async fn plan_build_evaluate_exec_creates_spec_and_evaluation_artifacts() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = make_runner(&temp, vt_cfg, "thread-plan-build-evaluate").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json(
            "pass",
            "Evaluator accepted the implementation.",
            0,
        )),
    ]));

    let result = runner
        .execute_task(&task("Planner + evaluator", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    assert!(
        workspace.join(".vtcode/tasks/current_spec.md").exists(),
        "planner should write current_spec.md"
    );
    assert!(
        workspace
            .join(".vtcode/tasks/current_evaluation.md")
            .exists(),
        "evaluator should write current_evaluation.md"
    );
    let tracker =
        fs::read_to_string(workspace.join(".vtcode/tasks/current_task.md")).expect("tracker file");
    assert!(tracker.contains("outcome: The requested change is implemented and tracked."));
    assert!(tracker.contains("verify: pwd"));

    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::PlanningStarted));
    assert!(events.contains(&HarnessEventKind::PlanningCompleted));
    assert!(events.contains(&HarnessEventKind::EvaluationStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));
}

#[tokio::test]
async fn evaluator_failure_forces_revision_before_success() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = make_runner(&temp, vt_cfg, "thread-evaluator-revision").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json(
            "fail",
            "A high-severity issue remains.",
            1,
        )),
        text_response("Revision 1: task is complete."),
        json_response(evaluator_response_json(
            "pass",
            "All issues have been addressed.",
            0,
        )),
    ]));

    let result = runner
        .execute_task(&task("Evaluator revision", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);
    let events = harness_events(&result);
    assert!(events.contains(&HarnessEventKind::EvaluationFailed));
    assert!(events.contains(&HarnessEventKind::RevisionStarted));
    assert!(events.contains(&HarnessEventKind::EvaluationPassed));
}

#[tokio::test]
async fn evaluator_request_includes_verification_results() {
    let temp = TempDir::new().expect("tempdir");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    let mut runner = make_runner(&temp, vt_cfg, "thread-evaluator-verification").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    let provider = RecordingQueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json(
            "pass",
            "Verification evidence looks good.",
            0,
        )),
    ]);
    let recorded = provider.clone();
    runner.provider_client = Box::new(provider);

    let result = runner
        .execute_task(&task("Evaluator verification evidence", "exec-task"), &[])
        .await
        .expect("task result");

    assert_eq!(result.outcome, TaskOutcome::Success);

    let requests = recorded.recorded_requests();
    let evaluator_request = requests.last().expect("evaluator request");
    let evaluator_prompt = evaluator_request
        .messages
        .first()
        .map(|message| message.content.as_text().into_owned())
        .expect("evaluator prompt");
    assert!(evaluator_prompt.contains("Verification results:"));
    assert!(evaluator_prompt.contains("[PASS] pwd (exit 0)"));
}

#[tokio::test]
async fn evaluator_exhaustion_writes_blocked_handoff_with_artifact_paths() {
    let temp = TempDir::new().expect("tempdir");
    let workspace = workspace_root(&temp);

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.agent.harness.orchestration_mode =
        vtcode_config::core::agent::HarnessOrchestrationMode::PlanBuildEvaluate;
    vt_cfg.agent.harness.max_revision_rounds = 1;
    vt_cfg.automation.full_auto.max_turns = 4;
    let mut runner = make_runner(&temp, vt_cfg, "thread-evaluator-exhaustion").await;
    runner
        .enable_full_auto(&[tools::TASK_TRACKER.to_string()])
        .await;
    runner.provider_client = Box::new(QueuedProvider::new(vec![
        json_response(planner_response_json("pwd")),
        tool_call_response(
            tools::TASK_TRACKER,
            json!({
                "action": "update",
                "index": 1,
                "status": "completed",
            }),
        ),
        text_response("The task is complete."),
        json_response(evaluator_response_json(
            "fail",
            "First evaluator rejection.",
            1,
        )),
        text_response("Revision 1: task is complete."),
        json_response(evaluator_response_json(
            "fail",
            "Second evaluator rejection.",
            1,
        )),
    ]));

    let result = runner
        .execute_task(&task("Evaluator exhaustion", "exec-task"), &[])
        .await
        .expect("task result");

    assert!(matches!(result.outcome, TaskOutcome::Failed { .. }));
    let paths = harness_paths(&result, HarnessEventKind::BlockedHandoffWritten);
    assert_eq!(paths.len(), 2);
    for path in paths {
        let content = fs::read_to_string(&path).expect("blocked handoff file");
        assert!(content.contains("current_spec.md"));
        assert!(content.contains("current_evaluation.md"));
    }
    assert!(workspace.join(".vtcode/tasks/current_spec.md").exists());
    assert!(
        workspace
            .join(".vtcode/tasks/current_evaluation.md")
            .exists()
    );
}

#[tokio::test]
async fn denied_tool_call_emits_one_failed_output_for_runtime_invocation() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-tool-output").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let response = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let request = LLMRequest {
        model: "gpt-5.3-codex".to_string(),
        ..Default::default()
    };
    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("turn should succeed");

    let tool_calls = turn.response.tool_calls.expect("tool call response");
    let tool_call_id = tool_calls[0].id.clone();
    let mut recorder = ExecEventRecorder::new("thread-denied-tool-output", None, None);
    recorder.record_thread_events(runtime.take_emitted_events());

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id =
        completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(
        completed_tool_output_count(
            &events,
            &tool_call_id,
            ToolCallStatus::Failed,
            &call_item_id
        ),
        1
    );
}

#[tokio::test]
async fn denied_parallel_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-parallel").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied-parallel".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-denied-parallel", None, None);

    let start = Instant::now();
    runner
        .execute_parallel_tool_calls(tool_calls, &mut runtime, &mut recorder, "[parallel]", false)
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn duplicate_parallel_tool_names_are_split_into_safe_batches() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note-a.txt"), "hello\n").expect("workspace file");
    fs::write(temp.path().join("note-b.txt"), "world\n").expect("workspace file");

    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-duplicate-parallel").await;
    runner
        .enable_full_auto(&[tools::READ_FILE.to_string()])
        .await;

    let tool_calls = vec![
        ToolCall::function(
            "call-read-a".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note-a.txt",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-read-b".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note-b.txt",
            })
            .to_string(),
        ),
    ];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-duplicate-parallel".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-duplicate-parallel", None, None);

    runner
        .execute_tool_call_batches(
            tool_calls,
            &mut runtime,
            &mut recorder,
            "[batch]",
            false,
            false,
        )
        .await
        .expect("tool execution should finish");

    let tool_outputs = runtime
        .state
        .messages
        .iter()
        .filter_map(|message| {
            let id = message.tool_call_id.as_ref()?;
            let output =
                serde_json::from_str::<serde_json::Value>(&message.content.as_text()).ok()?;
            Some((id.as_str(), output))
        })
        .collect::<Vec<_>>();
    assert_eq!(tool_outputs.len(), 2);
    assert!(tool_outputs.iter().any(|(id, output)| {
        *id == "call-read-a"
            && output["success"].as_bool() == Some(true)
            && output["path"].as_str() == Some("note-a.txt")
    }));
    assert!(tool_outputs.iter().any(|(id, output)| {
        *id == "call-read-b"
            && output["success"].as_bool() == Some(true)
            && output["path"].as_str() == Some("note-b.txt")
    }));
}

#[tokio::test]
async fn denied_sequential_tool_halt_returns_promptly() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-denied-sequential").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    let tool_calls = tool_call_response(
        tools::UNIFIED_EXEC,
        json!({
            "action": "run",
            "command": "echo vtcode",
        }),
    )
    .tool_calls
    .expect("tool call response");

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-denied-sequential".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-denied-sequential", None, None);

    let start = Instant::now();
    runner
        .execute_sequential_tool_calls(
            tool_calls,
            &mut runtime,
            &mut recorder,
            "[sequential]",
            false,
        )
        .await
        .expect("tool execution should finish");

    assert!(start.elapsed() < Duration::from_millis(200));
}

#[tokio::test]
async fn execute_tool_internal_retries_open_circuit_breaker() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");
    let runner = make_runner(&temp, VTCodeConfig::default(), "thread-open-circuit").await;
    let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
        failure_threshold: 1,
        min_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(10),
        reset_timeout: Duration::from_millis(10),
        ..CircuitBreakerConfig::default()
    }));
    runner
        .tool_registry
        .set_shared_circuit_breaker(breaker.clone());
    breaker.record_failure_category_for_tool(
        tools::READ_FILE,
        vtcode_commons::ErrorCategory::ExecutionError,
    );

    let start = Instant::now();
    let result = runner
        .execute_tool_internal(tools::READ_FILE, &json!({"path": "note.txt"}))
        .await
        .expect("circuit-open retry should recover");

    assert!(start.elapsed() >= Duration::from_millis(10));
    assert_eq!(
        result.get("content").and_then(|value| value.as_str()),
        Some("hello")
    );
}

#[tokio::test]
async fn sequential_policy_failure_halts_following_tool_calls() {
    let temp = TempDir::new().expect("tempdir");
    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");

    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-halt").await;
    runner
        .enable_full_auto(&[
            tools::UNIFIED_EXEC.to_string(),
            tools::READ_FILE.to_string(),
        ])
        .await;
    assert!(runner.is_valid_tool(tools::UNIFIED_EXEC).await);
    assert!(runner.is_valid_tool(tools::READ_FILE).await);

    let tool_calls = vec![
        ToolCall::function(
            "call-blocked".to_string(),
            tools::UNIFIED_EXEC.to_string(),
            json!({
                "action": "run",
                "command": "blocked-cmd",
            })
            .to_string(),
        ),
        ToolCall::function(
            "call-read".to_string(),
            tools::READ_FILE.to_string(),
            json!({
                "path": "note.txt",
            })
            .to_string(),
        ),
    ];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-halt".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-halt", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    assert!(
        runtime.state.warnings.iter().any(
            |warning| warning == "Tool denied by policy; halting further tool calls this turn."
        ),
        "warnings: {:?}",
        runtime.state.warnings
    );
    assert!(
        !runtime
            .state
            .executed_commands
            .iter()
            .any(|tool| tool == tools::READ_FILE)
    );

    let events = recorder.into_events();
    assert!(completed_tool_invocation_item_id(&events, "call-blocked").is_some());
    assert!(completed_tool_invocation_item_id(&events, "call-read").is_none());
}

#[tokio::test]
async fn sequential_tool_failures_record_categorized_user_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-message").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_EXEC.to_string()])
        .await;
    assert!(runner.is_valid_tool(tools::UNIFIED_EXEC).await);

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        json!({
            "action": "run",
            "command": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-message".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-message", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let tool_error = runtime
        .state
        .messages
        .last()
        .map(|message| message.content.as_text().into_owned())
        .expect("tool error recorded");
    let tool_error: serde_json::Value =
        serde_json::from_str(&tool_error).expect("structured tool error");
    assert_eq!(
        tool_error["error"]["category"].as_str(),
        Some("PolicyViolation"),
        "{tool_error}"
    );
    assert!(
        tool_error["error"]["recovery_suggestions"]
            .as_array()
            .is_some_and(|suggestions| suggestions.iter().any(|value| {
                value.as_str() == Some("Review workspace policies and restrictions")
            })),
        "{tool_error}"
    );
    assert_eq!(
        tool_error["error"]["partial_state_possible"].as_bool(),
        Some(false),
        "{tool_error}"
    );
}

#[tokio::test]
async fn sequential_tool_failures_do_not_record_interruption_guards() {
    let temp = TempDir::new().expect("tempdir");
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.commands.deny_regex = vec!["^blocked-cmd$".to_string()];
    let mut runner = make_runner(&temp, vt_cfg, "thread-policy-guard").await;
    runner
        .enable_full_auto(&[tools::UNIFIED_EXEC.to_string()])
        .await;

    let tool_calls = vec![ToolCall::function(
        "call-blocked".to_string(),
        tools::UNIFIED_EXEC.to_string(),
        json!({
            "action": "run",
            "command": "blocked-cmd",
        })
        .to_string(),
    )];

    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-policy-guard".to_string(), 16, 4, 128_000),
        None,
        None,
    );
    let mut recorder = ExecEventRecorder::new("thread-policy-guard", None, None);

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    assert!(
        runtime.state.error_recovery.lock().recent_errors.is_empty(),
        "handled tool failures should not be recorded as interrupted executions"
    );
}

#[tokio::test]
async fn steer_stop_closes_open_tool_calls_with_failed_output_items() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-stop-tool-output").await;

    fs::write(temp.path().join("note.txt"), "hello\n").expect("workspace file");

    let response = tool_call_response(
        tools::READ_FILE,
        json!({
            "path": "note.txt",
        }),
    );
    let provider = QueuedProvider::new(vec![response]);
    let mut provider_box: Box<dyn LLMProvider> = Box::new(provider);

    let (steering_tx, steering_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = AgentRuntime::new(
        AgentSessionState::new("session-stop".to_string(), 16, 4, 128_000),
        None,
        Some(steering_rx),
    );
    let request = LLMRequest {
        model: "gpt-5.3-codex".to_string(),
        ..Default::default()
    };
    let turn = runtime
        .run_turn_once(&mut provider_box, request, None)
        .await
        .expect("turn should succeed");

    let tool_calls = turn.response.tool_calls.expect("tool call response");
    let tool_call_id = tool_calls[0].id.clone();
    let mut recorder = ExecEventRecorder::new("thread-stop-tool-output", None, None);
    recorder.record_thread_events(runtime.take_emitted_events());
    steering_tx
        .send(SteeringMessage::SteerStop)
        .expect("steer stop should queue");

    runner
        .execute_sequential_tool_calls(tool_calls, &mut runtime, &mut recorder, "[single]", false)
        .await
        .expect("tool execution should finish");

    let events = recorder.into_events();
    let call_item_id =
        completed_tool_invocation_item_id(&events, &tool_call_id).expect("completed invocation");
    assert_eq!(
        completed_tool_output_count(
            &events,
            &tool_call_id,
            ToolCallStatus::Failed,
            &call_item_id
        ),
        1
    );
}
