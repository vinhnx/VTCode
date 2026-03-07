use super::{AgentRunner, RunnerSettings};
use crate::config::VTCodeConfig;
use crate::config::models::ModelId;
use crate::core::agent::state::TaskRunState;
use crate::core::agent::state::record_turn_duration;
use crate::core::agent::task::TaskOutcome;
use crate::core::agent::types::AgentType;
use crate::core::threads::ThreadBootstrap;
use std::fs;
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

    runner.enable_full_auto(&["read_file".to_string()]).await;

    assert!(runner.is_tool_exposed("read_file").await);
    assert!(!runner.is_tool_exposed("run_pty_cmd").await);
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
            "read_file".to_string(),
            "run_pty_cmd".to_string(),
            "task_tracker".to_string(),
            "plan_task_tracker".to_string(),
            "enter_plan_mode".to_string(),
        ])
        .await;

    assert_eq!(allowlist, vec!["read_file".to_string()]);
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
        .review_tool_allowlist(&[crate::config::constants::tools::WILDCARD_ALL.to_string()])
        .await;

    assert!(!allowlist.is_empty());
    assert!(
        allowlist
            .iter()
            .all(|tool| !runner.tool_registry.is_mutating_tool(tool))
    );
    assert!(
        !allowlist
            .iter()
            .any(|tool| tool == crate::config::constants::tools::RUN_PTY_CMD)
    );
}
