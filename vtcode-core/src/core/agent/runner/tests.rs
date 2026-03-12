use super::{AgentRunner, RunnerSettings};
use crate::config::VTCodeConfig;
use crate::config::constants::tools;
use crate::config::models::ModelId;
use crate::config::types::CapabilityLevel;
use crate::core::agent::state::TaskRunState;
use crate::core::agent::state::record_turn_duration;
use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::threads::ThreadBootstrap;
use crate::exec::events::{HarnessEventKind, ItemCompletedEvent, ThreadEvent, ThreadItemDetails};
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, ToolCall,
};
use crate::tools::Tool;
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
use tempfile::TempDir;

#[test]
fn record_turn_duration_records_once() {
    let mut durations = Vec::with_capacity(5);
    let mut total_ms = 0u128;
    let mut max_ms = 0u128;
    let mut count = 0usize;
    let mut recorded = false;
    let start = std::time::Instant::now();

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
    let mut runner = make_runner(&temp, VTCodeConfig::default(), "thread-exec-only-skip").await;
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
