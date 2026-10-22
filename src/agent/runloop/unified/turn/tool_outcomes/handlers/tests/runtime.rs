#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use std::time::{Duration, Instant};

use super::*;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::tool_outcomes::execution_result::handle_tool_execution_result;
use vtcode_core::config::ToolDisplayMode;
use vtcode_core::tools::registry::ToolExecutionError;

#[tokio::test]
async fn parallel_reads_persist_actual_outcomes_and_consume_streamed_items() {
    assert_parallel_read_lifecycle(false).await;
}

#[tokio::test]
async fn cancelled_parallel_reads_persist_terminal_events_while_draining() {
    assert_parallel_read_lifecycle(true).await;
}

struct ParallelReadCase {
    call_id: &'static str,
    args: serde_json::Value,
    status: &'static str,
    exit_code: Option<i64>,
}

async fn assert_parallel_read_lifecycle(cancelled: bool) {
    use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;
    use crate::agent::runloop::unified::turn::context::TurnLoopResult;
    use vtcode_core::core::agent::events::tool_started_event;

    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().expect("sample workspace").to_path_buf();
    let emitter = HarnessEventEmitter::new_async(&workspace, "parallel-outcomes", None)
        .await
        .expect("canonical emitter");
    let read_cases = [
        ParallelReadCase {
            call_id: "parallel_read",
            args: json!({"cmd":"sed -n '1p' sample.txt", "workdir":workspace}),
            status: "completed",
            exit_code: Some(0),
        },
        ParallelReadCase {
            call_id: "parallel_no_match",
            args: json!({"cmd":"grep 'absent-pattern' sample.txt", "workdir":workspace}),
            status: "completed",
            exit_code: Some(1),
        },
    ];
    for case in &read_cases {
        cache_tool_permission(&mut backing, tool_names::EXEC_COMMAND, &case.args, PermissionGrant::Permanent).await;
    }
    backing.harness_state.remember_streamed_tool_call_items([(
        "parallel_read".to_string(),
        StreamedToolCallItem {
            item_id: "streamed-read".to_string(),
            tool_name: tool_names::EXEC_COMMAND.to_string(),
        },
    )]);
    emitter
        .emit(tool_started_event(
            "streamed-read".to_string(),
            tool_names::EXEC_COMMAND,
            Some(&read_cases[0].args),
            Some("parallel_read"),
        ))
        .expect("streamed start");
    let calls = read_cases
        .iter()
        .map(|case| {
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                case.call_id.to_string(),
                tool_names::EXEC_COMMAND.to_string(),
                serde_json::to_string(&case.args).expect("arguments"),
            ))
        })
        .collect::<Vec<_>>();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    {
        let mut ctx = backing.turn_processing_context();
        ctx.full_auto = true;
        ctx.harness_emitter = Some(&emitter);
        if cancelled {
            ctx.ctrl_c_state.request_local_cancel();
            ctx.ctrl_c_notify.notify_one();
        }
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut outcome_ctx, &calls).await.expect("parallel reads");
        if cancelled {
            assert!(matches!(outcome, Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled))));
        } else {
            assert!(outcome.is_none());
        }
        assert!(
            ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty(),
            "turn teardown must not close executed calls again"
        );
        if !cancelled {
            for case in &read_cases {
                let response = ctx
                    .working_history
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some(case.call_id))
                    .expect("provider tool response");
                assert!(!response.content.as_text().contains("before it could execute"));
            }
        }
    }
    emitter.finish().await.expect("persist canonical events");
    let persisted = std::fs::read_to_string(workspace.join(".vtcode/sessions/parallel-outcomes/events.jsonl"))
        .expect("canonical events");
    let events = persisted
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("event JSON"))
        .collect::<Vec<_>>();
    for case in &read_cases {
        let call_id = case.call_id;
        let status = if cancelled { "failed" } else { case.status };
        let exit_code = if cancelled { None } else { case.exit_code };
        let matching = events
            .iter()
            .filter(|record| record["event"]["item"]["tool_call_id"] == call_id)
            .map(|record| &record["event"])
            .collect::<Vec<_>>();
        let starts = matching
            .iter()
            .filter(|event| event["type"] == "item.started" && event["item"]["type"] == "tool_invocation")
            .collect::<Vec<_>>();
        assert_eq!(starts.len(), 1, "one invocation start for {call_id}");
        let completions = matching
            .iter()
            .filter(|event| event["type"] == "item.completed")
            .collect::<Vec<_>>();
        assert_eq!(completions.len(), 2, "invocation and output completion for {call_id}");
        let invocation = completions
            .iter()
            .find(|event| event["item"]["type"] == "tool_invocation")
            .expect("invocation completion");
        assert_eq!(invocation["item"]["id"], starts[0]["item"]["id"]);
        assert_eq!(invocation["item"]["status"], status);
        let output = completions
            .iter()
            .find(|event| event["item"]["type"] == "tool_output")
            .expect("output completion");
        assert_eq!(output["item"]["status"], status);
        assert_eq!(output["item"]["exit_code"].as_i64(), exit_code);
        if call_id == "parallel_read" {
            assert_eq!(invocation["item"]["id"], "streamed-read");
        }
        let output_text = output["item"]["output"].as_str().expect("output text");
        if cancelled {
            let payload: serde_json::Value = serde_json::from_str(output_text).expect("cancelled response JSON");
            assert_eq!(payload["interrupted"], true);
            assert_eq!(payload["not_executed"], true);
            assert!(payload["error"].as_str().unwrap().contains("cancelled before execution"));
        } else if call_id == "parallel_read" {
            assert!(output_text.contains("hello"));
        }
    }
    assert_eq!(
        persisted.matches("tool_latency_recorded").count(),
        if cancelled { 0 } else { 2 },
        "pre-cancelled calls never enter the execution pipeline; executed calls record latency once"
    );
}

#[tokio::test]
async fn blocked_tool_call_guard_emits_tool_and_system_messages() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    let mut outcome = None;
    for idx in 0..=max_streak {
        outcome = enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
    }

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(ctx.harness_state.blocked_tool_recovery_pending());
    flush_blocked_tool_recovery(&mut ctx);
    assert!(!ctx.harness_state.blocked_tool_recovery_pending());
    assert!(ctx.harness_state.recovery_is_tool_free());
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
                .contains("A bounded recovery response will run without more tool calls")
    }));

    // UI synchronization: guard trips must clear any inline loading placeholder
    // so the bottom-line doesn't remain stuck. Input status left/right should
    // be None after the guard fires.
    assert!(ctx.input_status_state.left.is_none() && ctx.input_status_state.right.is_none());
}

#[tokio::test]
async fn blocked_tool_call_guard_allows_configured_consecutive_cap() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..max_streak {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked call {idx} should stay under cap");
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}

#[tokio::test]
async fn blocked_tool_call_guard_caps_non_consecutive_total_churn() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..limits.total_cap {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked total {idx} should stay under cap");
        ctx.reset_blocked_tool_call_streak();
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_total_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    flush_blocked_tool_recovery(&mut ctx);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("blocked_total"))
    );
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains(&format!("{} total blocked calls", limits.total_cap))
    }));
}

#[tokio::test]
async fn blocked_tool_call_guard_allows_four_times_total_churn_in_planning_mode() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path":"src/main.rs"});

    assert_eq!(limits.total_cap, limits.consecutive_cap * 4);
    for idx in 0..limits.total_cap {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "blocked total {idx} should stay under planning cap");
        ctx.reset_blocked_tool_call_streak();
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_total_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    flush_blocked_tool_recovery(&mut ctx);
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains(&format!("{} total blocked calls", limits.total_cap))
    }));
}

#[tokio::test]
async fn denied_execution_result_uses_planning_total_fuse() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let limits = blocked_tool_call_limits(&ctx);
    let args = json!({"path": "src/main.rs"});
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    for index in 0..limits.total_cap {
        let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
            error: ToolExecutionError::policy_violation("read_file", "denied for test"),
        });
        let outcome = handle_tool_execution_result(
            &mut outcome_ctx,
            format!("denied_{index}"),
            tool_names::READ_FILE,
            &args,
            &pipeline_outcome,
            Instant::now(),
        )
        .await
        .expect("denied execution result should be handled");
        assert!(outcome.is_none(), "planning denial {index} should stay under total fuse");
        outcome_ctx.ctx.reset_blocked_tool_call_streak();
    }

    let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
        error: ToolExecutionError::policy_violation("read_file", "denied for test"),
    });
    let outcome = handle_tool_execution_result(
        &mut outcome_ctx,
        "denied_over_cap".to_string(),
        tool_names::READ_FILE,
        &args,
        &pipeline_outcome,
        Instant::now(),
    )
    .await
    .expect("total-fuse denial should be handled");

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(outcome_ctx.ctx.harness_state.blocked_tool_recovery_pending());
}

async fn successful_tool_history_for_display_mode(mode: ToolDisplayMode) -> Vec<(Option<String>, String)> {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.renderer.set_tool_display_mode(mode);

    let output = json!({
        "command": "printf context",
        "output": "stdout context",
        "stdout": "stdout context",
        "stderr": "stderr context",
        "exit_code": 0,
        "critical_note": "critical context",
        "next_action": "next context",
        "generated_files": {
            "files": ["src/generated.rs"]
        },
        "metadata_flag": false,
        "metadata_count": 0,
        "fallback_tool": "read_file",
        "fallback_tool_args": {"path": "src/generated.rs"}
    });
    let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output,
        stdout: Some("stdout context".to_string()),
        modified_files: Vec::new(),
        command_success: true,
    });
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    {
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };

        handle_tool_execution_result(
            &mut outcome_ctx,
            "context_call".to_string(),
            tool_names::EXECUTE_CODE,
            &json!({"command": "printf context"}),
            &pipeline_outcome,
            Instant::now(),
        )
        .await
        .expect("successful command result should be handled");
    }

    ctx.working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Tool)
        .map(|message| (message.tool_call_id.clone(), message.content.as_text().into_owned()))
        .collect()
}

#[tokio::test]
async fn compact_display_keeps_provider_history_identical_to_expanded_display() {
    let compact_history = successful_tool_history_for_display_mode(ToolDisplayMode::Compact).await;
    let expanded_history = successful_tool_history_for_display_mode(ToolDisplayMode::Expanded).await;

    assert_eq!(compact_history, expanded_history);
    assert_eq!(compact_history.len(), 1);
    let content = &compact_history[0].1;
    for expected in [
        "stdout context",
        "stderr context",
        "critical context",
        "next context",
        "src/generated.rs",
        "metadata_flag",
        "metadata_count",
        "read_file",
        "fallback_tool_args",
    ] {
        assert!(content.contains(expected), "provider history lost {expected:?}: {content}");
    }
    assert!(!content.contains("Ctrl+T"));
    assert!(!content.contains("click to expand"));
}

#[tokio::test]
async fn non_zero_read_result_is_diagnosed_without_successful_readonly_signature() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path": "missing.rs"});
    let output = json!({
        "path": "missing.rs",
        "stderr": "No such file or directory",
        "exit_code": 1
    });
    let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output,
        stdout: None,
        modified_files: Vec::new(),
        command_success: false,
    });
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_tool_execution_result(
        &mut outcome_ctx,
        "missing-read".to_string(),
        tool_names::READ_FILE,
        &args,
        &pipeline_outcome,
        Instant::now(),
    )
    .await
    .expect("non-zero result should be handled");

    let tool_response = outcome_ctx
        .ctx
        .working_history
        .iter()
        .find(|message| message.role == uni::MessageRole::Tool)
        .expect("tool response should be recorded");
    let response: serde_json::Value =
        serde_json::from_str(&tool_response.content.as_text()).expect("tool response should remain JSON");
    assert_eq!(
        response["diagnosis"]["observed"],
        "'read_file' returned exit code 1. Stderr: No such file or directory"
    );
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .snapshot_turn_diagnostics(Default::default(), 0)
            .failed_tool_calls,
        1
    );
    let signature =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::signature_key_for(tool_names::READ_FILE, &args);
    assert!(!outcome_ctx.ctx.harness_state.has_successful_readonly_signature(&signature));
}

#[tokio::test]
async fn blocked_tool_call_guard_short_circuits_to_recovery_when_active() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path":"src/main.rs"});
    ctx.activate_recovery("loop detector");

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_recovery", tool_names::READ_FILE, &args);

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}

#[tokio::test]
async fn malformed_tool_calls_trip_preflight_circuit_at_configured_cap() {
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let malformed_call = || {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "malformed_call".to_string(),
            tool_names::CODE_SEARCH.to_string(),
            "{not-json".to_string(),
        ))
    };
    let max_failures = {
        let ctx = backing.turn_processing_context();
        max_consecutive_blocked_tool_calls_per_turn(&ctx)
    };

    for failure in 1..max_failures {
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
            .await
            .expect("malformed call should produce recoverable feedback");
        assert!(outcome.is_none(), "failure {failure} should remain recoverable");
    }

    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let outcome = handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
        .await
        .expect("preflight circuit should return a structured outcome");

    // The circuit breaker no longer hard-blocks; it arms a bounded tool-free
    // recovery pass so the model can synthesize a plain-text response instead
    // of the turn terminating as `Blocked` (which silently dropped approved-
    // plan builds — checkpoint turn_874).
    assert!(
        matches!(outcome, Some(TurnHandlerOutcome::Continue)),
        "circuit trip must return Continue (recovery), not Break(Blocked)"
    );
    assert!(ctx.working_history.iter().any(|message| {
        let content = message.content.as_text();
        content.contains("preflight_circuit_breaker")
            && content.contains("schema_correction")
            && content.contains("next_action")
    }));
    // The flush (called inside handle_tool_calls via dispatch) must arm
    // tool-free recovery and push the synthesis directive.
    assert!(ctx.harness_state.recovery_is_tool_free(), "preflight circuit trip must arm tool-free recovery mode");
    assert!(ctx.harness_state.consume_recovery_pass(), "recovery pass must be pending after the circuit flush");
    assert!(
        !ctx.working_history.iter().any(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("circuit breaker")
                && message.content.as_text().contains("tools are disabled for this pass")
        }),
        "preflight synthesis must be request-only"
    );
}

#[tokio::test]
async fn unrelated_admitted_tool_call_preserves_preflight_circuit_streak() {
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let malformed_call = || {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "malformed_call".to_string(),
            tool_names::CODE_SEARCH.to_string(),
            "{not-json".to_string(),
        ))
    };

    for _ in 0..2 {
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        assert!(
            handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
                .await
                .expect("malformed call should be recoverable")
                .is_none()
        );
    }

    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "valid_call", tool_names::READ_FILE, valid_args)
            .await
            .expect("valid call should execute")
            .is_none()
    );

    let max_failures = {
        let ctx = backing.turn_processing_context();
        max_consecutive_blocked_tool_calls_per_turn(&ctx)
    };
    assert_eq!(backing.harness_state.consecutive_preflight_failures, 2);
    for failure in 3..max_failures {
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        assert!(
            handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
                .await
                .expect("malformed call after reset should be recoverable")
                .is_none(),
            "failure {failure} should remain recoverable"
        );
    }
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let outcome = handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
        .await
        .expect("preflight circuit should return a recovery outcome");
    assert!(
        matches!(outcome, Some(TurnHandlerOutcome::Continue)),
        "circuit trip after reset must return Continue, not Break(Blocked)"
    );
    assert!(ctx.harness_state.recovery_is_tool_free(), "circuit trip after reset must arm tool-free recovery");
}

#[tokio::test]
async fn batched_preflight_failures_trip_the_same_circuit() {
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let malformed_call = |id: &str| {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            id.to_string(),
            tool_names::CODE_SEARCH.to_string(),
            r#"{"query":4}"#.to_string(),
        ))
    };
    let max_failures = {
        let ctx = backing.turn_processing_context();
        max_consecutive_blocked_tool_calls_per_turn(&ctx)
    };

    for _ in 0..max_failures {
        let mut ctx = backing.turn_processing_context();
        ctx.full_auto = true;
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let calls = [malformed_call("batch_a"), malformed_call("batch_b")];
        let outcome = handle_tool_calls(&mut outcome_ctx, &calls)
            .await
            .expect("batched preflight validation should complete");
        if outcome.is_some() {
            assert!(
                matches!(outcome, Some(TurnHandlerOutcome::Continue)),
                "batched preflight circuit trip must return Continue, not Break(Blocked)"
            );
            return;
        }
    }

    panic!("batched preflight failures did not trip the circuit");
}

#[tokio::test]
async fn preflight_circuit_drains_remaining_batch_tool_responses() {
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let max_failures = {
        let ctx = backing.turn_processing_context();
        max_consecutive_blocked_tool_calls_per_turn(&ctx)
    };
    let mut tool_calls = Vec::with_capacity(max_failures + 2);
    for index in 0..max_failures {
        tool_calls.push(PreparedAssistantToolCall::new(uni::ToolCall::function(
            format!("malformed_batch_{index}"),
            tool_names::CODE_SEARCH.to_string(),
            "{not-json".to_string(),
        )));
    }
    for index in 0..2 {
        tool_calls.push(PreparedAssistantToolCall::new(uni::ToolCall::function(
            format!("valid_after_{index}"),
            tool_names::CODE_SEARCH.to_string(),
            serde_json::to_string(&json!({"query": format!("batch continuation {index}")}))
                .expect("serialize valid tool args"),
        )));
    }
    let expected_ids = tool_calls.iter().map(|call| call.call_id().to_string()).collect::<Vec<_>>();

    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let outcome = handle_tool_calls(&mut outcome_ctx, &tool_calls)
        .await
        .expect("batch preflight circuit should return a recovery outcome");

    // The circuit trip arms recovery (Continue) instead of hard-blocking.
    assert!(
        matches!(outcome, Some(TurnHandlerOutcome::Continue)),
        "batch preflight circuit trip must return Continue, not Break(Blocked)"
    );
    // Every tool-call ID — including the drained remaining batch calls — must
    // receive a tool response so providers that enforce strict
    // assistant/tool ordering do not reject the next request.
    for tool_call_id in expected_ids {
        assert!(
            ctx.working_history
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(tool_call_id.as_str())),
            "missing tool response for {tool_call_id}"
        );
    }
    // The flush must arm tool-free recovery after all responses land.
    assert!(
        ctx.harness_state.recovery_is_tool_free(),
        "batch preflight circuit trip must arm tool-free recovery"
    );
}

#[tokio::test]
async fn prose_blob_tool_names_do_not_trip_circuit_or_skip_valid_siblings() {
    // Session session-vtcode-20260925T234343Z: the agent's prose about
    // tool-call tags was parsed as tool invocations with multi-KB reasoning
    // text as the tool name. Three of those LLM-mistake rejects tripped the
    // preflight circuit and `drain_preflight_circuit_responses` skipped a
    // valid `exec_command` sibling. Name mistakes must not advance the streak.
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let prose_blob = |id: &str, name: &str| {
        PreparedAssistantToolCall::new(uni::ToolCall::function(id.to_string(), name.to_string(), "{}".to_string()))
    };
    let valid_call = || {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "valid_sibling".to_string(),
            tool_names::CODE_SEARCH.to_string(),
            serde_json::to_string(&json!({"query": "sibling still runs"})).expect("serialize"),
        ))
    };

    let tool_calls = vec![
        prose_blob("blob_0", "exec_command\n... the fence opener is unclosed, so with my fix"),
        prose_blob("blob_1", "`. So the arg key tag in my test is"),
        prose_blob("blob_2", "exec_command\n"),
        valid_call(),
    ];
    let expected_ids = tool_calls.iter().map(|call| call.call_id().to_string()).collect::<Vec<_>>();

    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let outcome = handle_tool_calls(&mut outcome_ctx, &tool_calls)
        .await
        .expect("name mistakes should be recoverable feedback");

    assert!(outcome.is_none(), "name mistakes must not trip the preflight circuit or arm recovery");
    assert_eq!(
        ctx.harness_state.consecutive_preflight_failures, 1,
        "the empty exec_command arguments are invalid; unrelated sibling success must retain that failure"
    );
    for id in ["blob_0", "blob_1"] {
        let response = ctx
            .working_history
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id))
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&response.content.as_text()).unwrap();
        assert_eq!(payload["failure_count"], 0, "name mistakes must not advance the failure streak");
    }
    // Every call still gets a tool response (providers require one per call).
    for tool_call_id in expected_ids {
        assert!(
            ctx.working_history
                .iter()
                .any(|message| message.tool_call_id.as_deref() == Some(tool_call_id.as_str())),
            "missing tool response for {tool_call_id}"
        );
    }
    // The valid sibling must have been admitted and executed, not drained.
    assert!(
        ctx.working_history.iter().any(|message| {
            let content = message.content.as_text();
            message.tool_call_id.as_deref() == Some("valid_sibling") && !content.contains("was not executed")
        }),
        "valid sibling call must execute rather than be drained by the preflight circuit"
    );
    // Name-mistake responses tell the model to correct and retry.
    assert!(
        ctx.working_history.iter().any(|message| {
            message.tool_call_id.as_deref() == Some("blob_0")
                && message.content.as_text().contains("preflight_validation")
                && message.content.as_text().contains("schema_correction")
        }),
        "name mistakes must report preflight_validation (not circuit_breaker)"
    );
}

#[tokio::test]
async fn cleared_history_stub_echo_is_bounded_by_preflight_circuit() {
    // Historical placeholders are invalid calls, not an exemption from recovery limits.
    assert!(super::super::is_cleared_tool_input_echo(Some(&json!({"cleared": "tool_input"}))));
    assert!(!super::super::is_cleared_tool_input_echo(Some(&json!({"command": "ls"}))));
    assert!(!super::super::is_cleared_tool_input_echo(None));
    assert!(!super::super::is_cleared_tool_input_echo(Some(&json!({"cleared": "other"}))));

    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let cleared_call = |id: &str| {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            id.to_string(),
            tool_names::EXEC_COMMAND.to_string(),
            r#"{"cleared":"tool_input"}"#.to_string(),
        ))
    };

    let mut config = vtcode_core::config::VTCodeConfig::default();
    config.tools.max_consecutive_blocked_tool_calls_per_turn = 3;
    for index in 0..3 {
        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(&config);
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut outcome_ctx, &[cleared_call(&format!("cleared_{index}"))])
            .await
            .expect("cleared echo should be recoverable");
        assert_eq!(outcome.is_some(), index == 2, "the third invalid call must arm recovery");
    }
    let ctx = backing.turn_processing_context();
    assert_eq!(
        ctx.harness_state.consecutive_preflight_failures, 3,
        "cleared echoes count toward the preflight streak"
    );
    assert!(ctx.harness_state.recovery_is_tool_free(), "the validation cap arms tool-free recovery");
    assert!(
        ctx.working_history.iter().any(|message| {
            message.tool_call_id.as_deref() == Some("cleared_0")
                && message.content.as_text().contains("Do not echo the history stub")
                && message.content.as_text().contains("requires")
                && message.content.as_text().contains("cmd")
        }),
        "exec_command cleared echo must name the public `cmd` contract"
    );

    let mut tracker_backing = TestContextBacking::new(4).await;
    let mut tracker_attempts = LoopTracker::new();
    let mut tracker_files = BTreeSet::new();
    let mut tracker_ctx = tracker_backing.turn_processing_context();
    let mut tracker_outcome_ctx = ToolOutcomeContext {
        ctx: &mut tracker_ctx,
        repeated_tool_attempts: &mut tracker_attempts,
        turn_modified_files: &mut tracker_files,
    };
    assert!(
        handle_tool_calls(
            &mut tracker_outcome_ctx,
            &[PreparedAssistantToolCall::new(uni::ToolCall::function(
                "cleared_tracker".to_string(),
                tool_names::TASK_TRACKER.to_string(),
                r#"{"cleared":"tool_input"}"#.to_string(),
            ))]
        )
        .await
        .expect("task_tracker cleared echo should be recoverable")
        .is_none()
    );
    assert!(
        tracker_ctx.working_history.iter().any(|message| {
            message.tool_call_id.as_deref() == Some("cleared_tracker")
                && message.content.as_text().contains("Do not echo the history stub")
                && !message.content.as_text().contains("exec_command requires")
        }),
        "task_tracker cleared echo must not mention exec_command"
    );
}

#[tokio::test]
async fn exec_command_missing_command_correction_names_cmd_contract() {
    // Public schema requires `cmd` (vtcode-utility-tool-specs), internal
    // UNIFIED_EXEC validation reports `command`. The correction must name the
    // public `cmd` contract with `command` as alias, not just repeat the
    // internal name.
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    assert!(
        handle_tool_calls(
            &mut outcome_ctx,
            &[PreparedAssistantToolCall::new(uni::ToolCall::function(
                "missing_cmd".to_string(),
                tool_names::EXEC_COMMAND.to_string(),
                r#"{"yield_time_ms":10000}"#.to_string(),
            ))]
        )
        .await
        .expect("missing cmd should be recoverable")
        .is_none()
    );
    assert!(
        ctx.working_history.iter().any(|message| {
            message.tool_call_id.as_deref() == Some("missing_cmd")
                && message.content.as_text().contains("cmd")
                && message.content.as_text().contains("alias")
        }),
        "exec_command missing-arg correction must name `cmd` with `command` alias"
    );
}

#[tokio::test]
async fn preflight_circuit_does_not_block_approved_plan_execution() {
    // Regression for checkpoint turn_874: when the preflight circuit breaker
    // tripped during an approved-plan build turn, the old code returned
    // `Break(Blocked)`. A blocked result never derives
    // `plan_approved_execution_pending`, so the session loop silently dropped
    // the approved build and the agent could not continue. The fix arms a
    // bounded tool-free recovery pass instead, returning `Continue`.
    let mut backing = TestContextBacking::new(4).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let malformed_call = || {
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "malformed_call".to_string(),
            tool_names::CODE_SEARCH.to_string(),
            "{not-json".to_string(),
        ))
    };
    let max_failures = {
        let ctx = backing.turn_processing_context();
        max_consecutive_blocked_tool_calls_per_turn(&ctx)
    };

    // Simulate the fresh build turn that the session loop starts after the
    // user approves the plan.
    backing.harness_state.set_approved_plan_execution(true);
    assert!(backing.harness_state.is_approved_plan_execution());

    for failure in 1..max_failures {
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
            .await
            .expect("malformed call should produce recoverable feedback");
        assert!(outcome.is_none(), "failure {failure} should remain recoverable");
    }

    let mut ctx = backing.turn_processing_context();
    // Re-assert the approved-plan flag survived into the turn context.
    assert!(ctx.harness_state.is_approved_plan_execution());
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let outcome = handle_tool_calls(&mut outcome_ctx, &[malformed_call()])
        .await
        .expect("circuit trip should produce a recovery outcome");

    // Must NOT be Break(Blocked) — that would silently drop the approved build.
    assert!(
        matches!(outcome, Some(TurnHandlerOutcome::Continue)),
        "approved-plan build must not hard-block on preflight circuit trip"
    );
    assert!(
        ctx.harness_state.recovery_is_tool_free(),
        "approved-plan build must arm tool-free recovery after circuit trip"
    );
    assert!(
        ctx.harness_state.consume_recovery_pass(),
        "recovery pass must be pending so the next loop iteration runs it"
    );
}

#[tokio::test]
async fn flush_preflight_circuit_recovery_is_idempotent_and_arms_tool_free_mode() {
    // The flush must be safe to call multiple times (dispatch calls it at
    // several exit points) and must arm request-only tool-free recovery once.
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();

    // Arming without flushing must not push the directive.
    ctx.harness_state.arm_preflight_circuit_recovery();
    let directive_count_before = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("circuit breaker")
                && message.content.as_text().contains("tools are disabled for this pass")
        })
        .count();
    assert_eq!(directive_count_before, 0, "directive must not be pushed before flush");

    // First flush arms recovery without persisting the directive.
    flush_preflight_circuit_recovery(&mut ctx);
    let directive_count_after = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("circuit breaker")
                && message.content.as_text().contains("tools are disabled for this pass")
        })
        .count();
    assert_eq!(directive_count_after, 0, "directive must stay request-only");
    assert!(ctx.harness_state.recovery_is_tool_free(), "flush must arm tool-free recovery mode");
    assert!(ctx.harness_state.consume_recovery_pass(), "recovery pass must be pending after flush");

    // Second flush is a no-op (the pending flag was consumed).
    flush_preflight_circuit_recovery(&mut ctx);
    let directive_count_final = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("circuit breaker")
                && message.content.as_text().contains("tools are disabled for this pass")
        })
        .count();
    assert_eq!(directive_count_final, 0, "canonical history must stay free of synthesis directives");
}

#[test]
fn dynamic_find_preflight_feedback_names_safe_static_alternatives() {
    let correction = preflight_schema_correction(
        tool_names::EXEC_COMMAND,
        "dynamic shell expansion in find commands is not allowed",
    );

    assert!(correction.contains("literal path and quoted pattern"));
    assert!(correction.contains("rg --files"));
    assert!(correction.contains("Do not use `$()`"));
    assert!(correction.contains("Do not retry"));
}

#[test]
fn policy_preflight_feedback_does_not_recommend_json_repair() {
    let security = preflight_schema_correction(
        tool_names::EXEC_COMMAND,
        "Command security check failed: Command injection pattern detected",
    );
    assert!(security.contains("security policy"));
    assert!(!security.contains("matching the declared schema"));
    assert!(security.contains("Do not retry"));
    let planning = preflight_schema_correction(tool_names::EXEC_COMMAND, "tool denied by planning workflow");
    assert!(planning.contains("command -v"));
    assert!(planning.contains("defer unknown scripts"));
    let malformed = preflight_schema_correction(tool_names::EXEC_COMMAND, "Invalid arguments: missing cmd");
    assert!(malformed.contains("matching the declared schema"));
}

#[tokio::test]
async fn planning_preflight_recovery_requires_plan_artifact() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.arm_preflight_circuit_recovery();

    flush_preflight_circuit_recovery(&mut ctx);

    assert!(ctx.harness_state.recovery_is_tool_free());
    assert_eq!(ctx.harness_state.recovery_reason.as_deref(), Some("preflight validation circuit breaker"));
    assert!(!ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("tools are disabled")
    }));
}

#[tokio::test]
async fn unified_validation_ignores_preseeded_legacy_loop_detector_state() {
    let mut backing = TestContextBacking::new(2).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;

    backing.autonomous_executor.set_loop_limit(tool_names::READ_FILE, 2);
    let seeded_args = json!({"path": valid_file.to_string_lossy()});
    assert!(
        backing
            .autonomous_executor
            .record_tool_call(tool_names::READ_FILE, &seeded_args)
            .is_none()
    );
    let _ = backing
        .autonomous_executor
        .record_tool_call(tool_names::READ_FILE, &seeded_args);
    let warning = backing
        .autonomous_executor
        .record_tool_call(tool_names::READ_FILE, &seeded_args);
    assert!(warning.is_some());
    assert!(backing.autonomous_executor.is_hard_limit_exceeded(tool_names::READ_FILE));

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome =
        handle_single_tool_call(&mut outcome_ctx, "legacy_detector_seeded", tool_names::READ_FILE, valid_args)
            .await
            .expect("unified validation should ignore legacy detector state");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("Loop detector stopped repeated") })
    );
    assert!(backing.autonomous_executor.is_hard_limit_exceeded(tool_names::READ_FILE));
}

#[tokio::test]
async fn active_primary_agent_policy_blocks_hallucinated_denied_tool_call() {
    let mut backing = TestContextBacking::new(2).await;
    let mut spec = test_primary_agent_spec("reader");
    spec.tools = Some(vec![tool_names::READ_FILE.to_string()]);
    spec.disallowed_tools = vec![tool_names::READ_FILE.to_string()];
    backing.select_primary_agent_from_specs(&[spec], "reader");

    let valid_file = backing.sample_file.clone();
    let args = json!({"path": valid_file.to_string_lossy()});
    let mut ctx = backing.turn_processing_context();

    let result = validate_tool_call(&mut ctx, "denied_read", tool_names::READ_FILE, &args)
        .await
        .expect("validation should complete");

    assert!(matches!(result, ValidationResult::Blocked));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("active primary agent policy") })
    );
    assert_eq!(ctx.harness_state.tool_calls, 0);
}

#[tokio::test]
async fn invalid_preflight_arguments_do_not_trip_blocked_tool_guard() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();

    let result = validate_tool_call(
        &mut ctx,
        "invalid_code_search",
        tool_names::CODE_SEARCH,
        &json!({"query": "session setup", "max_results": 120}),
    )
    .await
    .expect("invalid arguments should be returned as structured tool feedback");

    assert!(matches!(result, ValidationResult::Handled));
    assert_eq!(ctx.harness_state.consecutive_blocked_tool_calls, 0);
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.role == uni::MessageRole::Tool && message.content.as_text().contains("max=100") })
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
        outcome = enforce_repeated_shell_run_guard(&mut ctx, &format!("shell_{idx}"), tool_names::UNIFIED_EXEC, &args);
    }

    assert!(matches!(outcome, Some(ValidationResult::Blocked)));
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

    let first =
        enforce_duplicate_task_tracker_create_guard(&mut ctx, "task_tracker_first", tool_names::TASK_TRACKER, &args);
    assert!(first.is_none());
    let second =
        enforce_duplicate_task_tracker_create_guard(&mut ctx, "task_tracker_second", tool_names::TASK_TRACKER, &args);
    assert!(matches!(second, Some(ValidationResult::Blocked)));
}

#[tokio::test]
async fn validate_tool_call_blocks_when_wall_clock_budget_exhausted() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.turn_started_at = Instant::now()
        .checked_sub(Duration::from_secs(ctx.harness_state.max_tool_wall_clock.as_secs() + 1))
        .unwrap();

    let result =
        validate_tool_call(&mut ctx, "wall_clock_exhausted", tool_names::READ_FILE, &json!({"path": sample_path}))
            .await
            .expect("validate wall-clock-exhausted tool call");

    assert!(matches!(result, ValidationResult::Blocked));
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Policy violation: exceeded tool wall clock budget")
    }));

    // A second rejected call in the same batch must NOT repeat the full policy
    // message — it gets a compact "call skipped" stub instead.
    let second =
        validate_tool_call(&mut ctx, "wall_clock_exhausted_2", tool_names::READ_FILE, &json!({"path": sample_path}))
            .await
            .expect("validate second wall-clock-exhausted tool call");
    assert!(matches!(second, ValidationResult::Blocked));
    let stub = ctx
        .working_history
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("wall_clock_exhausted_2"))
        .expect("wall-clock rejection response");
    let payload: serde_json::Value = serde_json::from_str(&stub.content.as_text()).unwrap();
    assert!(
        payload["next_action"]
            .as_str()
            .unwrap()
            .contains("Synthesize your final answer now")
    );
    assert!(!stub.content.as_text().contains("alternative tool"));
    let policy_violation_count = ctx
        .working_history
        .iter()
        .filter(|message| {
            message
                .content
                .as_text()
                .contains("Policy violation: exceeded tool wall clock budget")
        })
        .count();
    assert_eq!(policy_violation_count, 1, "full policy message must be emitted exactly once per turn");
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("call skipped") })
    );

    // Flushing after the batch pushes exactly one "synthesize now" directive so
    // the model produces a final answer from gathered context instead of stalling.
    flush_budget_synthesis_directives(&mut ctx);
    let directive_count = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("Synthesize your final answer now")
        })
        .count();
    assert_eq!(directive_count, 1, "synthesis directive must be pushed once");

    // The flush must also arm the tool-free recovery pass so the next request
    // strips tool definitions at the API level — the directive alone is
    // advisory and models kept emitting (rejected) tool calls after it
    // (checkpoints turn_637, turn_647).
    assert!(
        ctx.harness_state.recovery_is_tool_free(),
        "wall-clock directive flush must arm tool-free recovery mode"
    );
    assert!(
        ctx.harness_state.consume_recovery_pass(),
        "recovery pass must be pending so the next loop iteration consumes it"
    );

    // Flushing again is a no-op (the pending flag is consumed).
    flush_budget_synthesis_directives(&mut ctx);
    let directive_count_after = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().contains("Synthesize your final answer now")
        })
        .count();
    assert_eq!(directive_count_after, 1, "directive must not be duplicated");
}

#[tokio::test]
async fn start_planning_clears_task_tracker_create_signatures() {
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let enter_args = json!({});
    cache_tool_permission(&mut backing, tool_names::START_PLANNING, &enter_args, PermissionGrant::Permanent).await;

    let mut ctx = backing.turn_processing_context();
    let create_args = json!({
        "action": "create",
        "title": "Task Checklist",
        "items": ["step 1"]
    });
    let first = enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_seed",
        tool_names::TASK_TRACKER,
        &create_args,
    );
    assert!(first.is_none());

    let result = validate_tool_call(&mut ctx, "start_planning_call", tool_names::START_PLANNING, &enter_args)
        .await
        .expect("validate start_planning");
    assert!(matches!(result, ValidationResult::Proceed(_)));

    let second = enforce_duplicate_task_tracker_create_guard(
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

    let outcome = recovery::apply_recovery_action(
        &mut ctx,
        "recovery_call",
        crate::agent::runloop::unified::turn::recovery_flow::RecoveryAction::SkipStep,
    )
    .await
    .expect("skip-step recovery should succeed");

    assert!(matches!(outcome, Some(ValidationResult::Handled)));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("\"skipped\":true") })
    );
}

#[tokio::test]
async fn repeated_identical_readonly_call_in_same_turn_reuses_recent_result() {
    use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;
    use vtcode_core::core::agent::events::tool_started_event;

    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let emitter = HarnessEventEmitter::new_async(&workspace, "cached-read-lifecycle", None)
        .await
        .unwrap();
    let args = json!({
        "path": backing.sample_file.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    tp_ctx.harness_emitter = Some(&emitter);
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(&mut outcome_ctx, "read_once", tool_names::READ_FILE, args.clone())
        .await
        .expect("first readonly call should succeed");

    assert!(first.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);
    assert!(outcome_ctx.ctx.harness_state.has_successful_readonly_signature(
        &crate::agent::runloop::unified::turn::tool_outcomes::helpers::signature_key_for(tool_names::READ_FILE, &args,),
    ));
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .find_recent_successful_output(
                tool_names::READ_FILE,
                &args,
                outcome_ctx.ctx.harness_state.max_tool_wall_clock
            )
            .is_some()
    );

    outcome_ctx.ctx.harness_state.remember_streamed_tool_call_items([(
        "read_twice".to_owned(),
        StreamedToolCallItem {
            item_id: "streamed-cached-read".to_owned(),
            tool_name: tool_names::READ_FILE.to_owned(),
        },
    )]);
    emitter
        .emit(tool_started_event(
            "streamed-cached-read".to_owned(),
            tool_names::READ_FILE,
            Some(&args),
            Some("read_twice"),
        ))
        .unwrap();
    let second = handle_single_tool_call(&mut outcome_ctx, "read_twice", tool_names::READ_FILE, args.clone())
        .await
        .expect("duplicate readonly call should be reused");

    assert!(second.is_none());
    assert!(
        outcome_ctx.ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty(),
        "cached reads must not be cancelled by teardown"
    );
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .snapshot_turn_diagnostics(Default::default(), 0)
            .reused_results,
        1
    );
    emitter.finish().await.unwrap();
    let persisted =
        std::fs::read_to_string(workspace.join(".vtcode/sessions/cached-read-lifecycle/events.jsonl")).unwrap();
    let events = persisted
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let completions = events
        .iter()
        .map(|record| &record["event"])
        .filter(|event| event["type"] == "item.completed" && event["item"]["tool_call_id"] == "read_twice")
        .collect::<Vec<_>>();
    assert_eq!(completions.len(), 2, "one cached invocation and output completion");
    let invocation = completions
        .iter()
        .find(|event| event["item"]["type"] == "tool_invocation")
        .unwrap();
    assert_eq!(invocation["item"]["id"], "streamed-cached-read");
    assert_eq!(invocation["item"]["status"], "completed");
    assert_eq!(invocation["item"]["outcome"], "success");
    assert_eq!(invocation["item"]["arguments"], args);
    let output = completions.iter().find(|event| event["item"]["type"] == "tool_output").unwrap();
    assert_eq!(output["item"]["status"], "completed");
    assert!(output["item"]["output"].as_str().unwrap().contains("hello"));
    assert!(!output["item"]["output"].as_str().unwrap().contains("before it could execute"));
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("\"reused_recent_result\":true") })
    );
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("\"result_ref_only\":true") })
    );
}

#[tokio::test]
async fn repeated_same_file_paginated_reads_do_not_trip_read_family_cap() {
    // Regression: reading different slices of the same file (different
    // offset/limit) is legitimate pagination, not a retry loop. The family
    // cap must NOT trip on these — otherwise the agent is forced into a
    // tool-free recovery pass that produces a garbage final answer.
    // Reproduces the failure seen in checkpoint turn_613.
    let read_family_cap = 4;
    let mut backing = TestContextBacking::new(read_family_cap).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let build_paginated_read_args = |line: usize| {
        json!({
            "path": sample_path.clone(),
            "line_start": line,
            "line_end": line
        })
    };

    // Paginated reads: each variant targets a different line range, so each
    // gets a distinct family key (`file_operation::read::<path>::off=<line>`).
    for idx in 1..=read_family_cap {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("read_variant_{idx}"),
            tool_names::READ_FILE,
            build_paginated_read_args(idx),
        )
        .await
        .expect("paginated read variant should complete");

        assert!(outcome.is_none(), "paginated read {idx} must not be blocked by the family cap");
    }

    // No pagination burst should have tripped recovery: the streak resets on
    // every distinct slice, so it never reaches the cap.
    assert_eq!(
        outcome_ctx.ctx.harness_state.consecutive_same_file_read_family_calls, 1,
        "paginated reads must reset the family streak, not accumulate it"
    );
    assert!(!outcome_ctx.ctx.is_recovery_active(), "recovery must not activate for legitimate pagination");
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("repeated_read_family") }),
        "no repeated_read_family error should be emitted for pagination"
    );
}

#[tokio::test]
async fn repeated_identical_slice_read_trips_read_family_cap() {
    // True retry loop: same path + same slice, repeated verbatim. The cap must
    // trip here — this is the guard's reason for existing.
    let read_family_cap = 4;
    let mut backing = TestContextBacking::new(read_family_cap).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    // Identical slice every time -> same family key -> streak accumulates.
    let identical_args = json!({
        "path": sample_path.clone(),
        "offset": 0,
        "limit": 4
    });

    for idx in 1..read_family_cap {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("read_repeat_{idx}"),
            tool_names::READ_FILE,
            identical_args.clone(),
        )
        .await
        .expect("identical read should complete");
        assert!(outcome.is_none(), "read {idx} below cap should not block");
    }
    assert_eq!(outcome_ctx.ctx.harness_state.consecutive_same_file_read_family_calls, read_family_cap - 1,);

    let execution_history_len_before_block = outcome_ctx.ctx.tool_registry.execution_history_len();
    let tool_calls_before_block = outcome_ctx.ctx.harness_state.tool_calls;

    let blocked =
        handle_single_tool_call(&mut outcome_ctx, "read_repeat_blocked", tool_names::READ_FILE, identical_args.clone())
            .await
            .expect("read-family cap attempt should be handled");

    assert!(matches!(blocked, Some(TurnHandlerOutcome::Continue)));
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), execution_history_len_before_block);
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, tool_calls_before_block);
    assert_eq!(outcome_ctx.ctx.harness_state.consecutive_same_file_read_family_calls, read_family_cap);
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
async fn intermittent_path_cap_rejections_allow_progress_but_consecutive_retries_trip_fuse() {
    for productive in [true, false] {
        let mut backing = TestContextBacking::new(64).await;
        backing.select_build_primary_agent();
        let sample = backing.sample_file.clone();
        let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-hello\n+continued after read cap\n*** End Patch\n"});
        cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch, PermissionGrant::Permanent).await;
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let cap = max_consecutive_blocked_tool_calls_per_turn(&ctx);
        for _ in 0..6 {
            ctx.harness_state
                .record_file_read_path_call(sample.to_string_lossy().into_owned());
        }
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
        };
        let attempts = if productive { cap * 2 + 1 } else { cap + 1 };
        for index in 1..=attempts {
            let before = outcome_ctx.ctx.tool_registry.execution_history_len();
            handle_single_tool_call(
                &mut outcome_ctx,
                &format!("capped_{index}"),
                tool_names::READ_FILE,
                json!({"path":sample, "offset":index, "limit":1}),
            )
            .await
            .unwrap();
            assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), before, "capped reads cannot execute");
            if productive {
                let other = sample.with_file_name(format!("other_{index}.txt"));
                std::fs::write(&other, format!("distinct evidence {index}\n")).unwrap();
                handle_single_tool_call(
                    &mut outcome_ctx,
                    &format!("progress_{index}"),
                    tool_names::READ_FILE,
                    json!({"path":other, "offset":1, "limit":1}),
                )
                .await
                .unwrap();
                assert!(outcome_ctx.ctx.tool_registry.execution_history_len() > before);
                assert!(!outcome_ctx.ctx.harness_state.blocked_tool_recovery_pending());
            }
        }
        assert_eq!(outcome_ctx.ctx.harness_state.blocked_tool_calls, 0);
        assert_eq!(
            outcome_ctx.ctx.harness_state.blocked_tool_recovery_pending() || outcome_ctx.ctx.is_recovery_active(),
            !productive,
        );
        if productive {
            handle_single_tool_call(&mut outcome_ctx, "continue_patch", tool_names::APPLY_PATCH, patch)
                .await
                .unwrap();
            assert_eq!(std::fs::read_to_string(sample).unwrap(), "continued after read cap\n");
        } else {
            assert_eq!(outcome_ctx.ctx.harness_state.consecutive_blocked_tool_calls, cap + 1);
            assert_eq!(std::fs::read_to_string(sample).unwrap(), "hello\n");
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn concentrated_search_coaching_keeps_distinct_queries_available() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let content = ('A'..='G').map(|letter| format!("unique_fact_{letter}\n")).collect::<String>();
    std::fs::write(workspace.join("README.md"), content).unwrap();
    let args = json!({"query":"unique_fact_A","path":"README.md","result_types":["text"]});
    cache_tool_permission(&mut backing, tool_names::CODE_SEARCH, &args, PermissionGrant::Permanent).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    for (index, letter) in ('A'..='G').enumerate() {
        let id = format!("focused_{letter}");
        let mut args = args.clone();
        args["query"] = json!(format!("unique_fact_{letter}"));
        let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
            id.clone(),
            tool_names::CODE_SEARCH.to_string(),
            serde_json::to_string(&args).unwrap(),
        ));
        assert!(handle_tool_calls(&mut outcome_ctx, &[call]).await.unwrap().is_none());
        let history = &outcome_ctx.ctx.working_history;
        let output = history
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(&id))
            .unwrap()
            .content
            .as_text();
        let payload: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(payload["returned"], 1, "expected a distinct line for {letter}: {output}");
        let notices = history
            .iter()
            .filter(|message| message.content.as_text().contains("Several searches have concentrated"))
            .count();
        assert_eq!(notices, usize::from(index >= 5));
        assert_eq!(outcome_ctx.repeated_tool_attempts.low_signal_tool_calls, 0);
        assert!(!outcome_ctx.ctx.is_recovery_active());
    }
    assert_eq!(outcome_ctx.ctx.harness_state.blocked_tool_calls, 0);
    assert!(!outcome_ctx.repeated_tool_attempts.verification_is_pending());
}

#[cfg(unix)]
#[tokio::test]
async fn redundant_sed_coaching_keeps_edits_and_verification_available() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    std::fs::write(&backing.sample_file, "alpha\nbeta\ngamma\n").unwrap();
    std::fs::create_dir(workspace.join("scripts")).unwrap();
    std::fs::write(workspace.join("scripts/check_markdown.py"), "print('verified')\n").unwrap();
    let first = json!({"cmd":"sed -n '1,3p' sample.txt", "workdir":workspace});
    let repeated = json!({"cmd":"sed -n '2p' sample.txt", "workdir":workspace});
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-beta\n+delta\n*** End Patch\n"});
    let verifier = json!({"cmd":"python3 scripts/check_markdown.py", "workdir":workspace});
    for (name, args) in [
        (tool_names::EXEC_COMMAND, &first),
        (tool_names::EXEC_COMMAND, &repeated),
        (tool_names::APPLY_PATCH, &patch),
        (tool_names::EXEC_COMMAND, &verifier),
    ] {
        cache_tool_permission(&mut backing, name, args, PermissionGrant::Permanent).await;
    }
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    for (id, args) in [("first_read", first), ("retained_read", repeated.clone())] {
        let mut source_call = uni::Message::assistant(String::new());
        source_call.tool_calls = Some(vec![uni::ToolCall::function(
            id.into(),
            tool_names::EXEC_COMMAND.into(),
            args.to_string(),
        )]);
        outcome_ctx.ctx.working_history.push(source_call);
        let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
            id.to_string(),
            tool_names::EXEC_COMMAND.to_string(),
            serde_json::to_string(&args).unwrap(),
        ));
        assert!(handle_tool_calls(&mut outcome_ctx, &[call]).await.unwrap().is_none());
    }
    outcome_ctx
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcome_ctx.ctx.working_history);
    let history = &outcome_ctx.ctx.working_history;
    let evidence_index = history
        .iter()
        .position(|m| m.tool_call_id.as_deref() == Some("retained_read"))
        .unwrap();
    let notice_index = history
        .iter()
        .position(|m| m.content.as_text().contains("This inspection returned only text evidence"))
        .unwrap();
    assert!(evidence_index < notice_index);
    assert_eq!(outcome_ctx.repeated_tool_attempts.low_signal_tool_calls, 1);
    assert!(!outcome_ctx.ctx.is_recovery_active());
    assert!(!outcome_ctx.repeated_tool_attempts.verification_is_pending());

    assert!(
        handle_single_tool_call(&mut outcome_ctx, "useful_edit", tool_names::APPLY_PATCH, patch)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "changed_read", tool_names::EXEC_COMMAND, repeated)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read_to_string(workspace.join("sample.txt")).unwrap(), "alpha\ndelta\ngamma\n");
    assert_eq!(outcome_ctx.repeated_tool_attempts.low_signal_tool_calls, 1, "the changed read is productive");
    outcome_ctx.repeated_tool_attempts.mark_verification_pending();
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "verify_continuation", tool_names::EXEC_COMMAND, verifier)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!outcome_ctx.repeated_tool_attempts.verification_is_pending());
    assert_eq!(outcome_ctx.ctx.harness_state.blocked_tool_calls, 0);
    assert!(!outcome_ctx.ctx.is_recovery_active());
}

#[cfg(unix)]
#[tokio::test]
async fn masked_checker_feedback_is_bounded_and_standalone_verification_continues_loop() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    std::fs::create_dir(workspace.join("scripts")).unwrap();
    let checker = workspace.join("scripts/check_markdown.py");
    std::fs::write(&checker, "import sys, time, pathlib\nwhile not pathlib.Path('release-checker').exists(): time.sleep(0.01)\nprint('README: checker failed')\nsys.exit(7)\n").unwrap();
    let masked = json!({"cmd":"python3 scripts/check_markdown.py; echo \"lint exit: $?\"", "workdir":workspace, "yield_time_ms":250});
    let standalone = json!({"cmd":"python3 scripts/check_markdown.py", "workdir":workspace});
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-hello\n+continued after verification\n*** End Patch\n"});
    for (name, args) in [
        (tool_names::EXEC_COMMAND, &masked),
        (tool_names::EXEC_COMMAND, &standalone),
        (tool_names::WRITE_STDIN, &json!({"session_id":"placeholder", "chars":""})),
        (tool_names::APPLY_PATCH, &patch),
    ] {
        cache_tool_permission(&mut backing, name, args, PermissionGrant::Permanent).await;
    }
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry
        .set_inspection_mode(vtcode_core::config::core::agent::InspectionMode::Off);
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    for index in 0..2 {
        let mut call_id = format!("masked_checker_{index}");
        let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
            call_id.clone(),
            tool_names::EXEC_COMMAND.to_string(),
            serde_json::to_string(&masked).unwrap(),
        ));
        assert!(handle_tool_calls(&mut outcome_ctx, &[call]).await.unwrap().is_none());
        let output = outcome_ctx
            .ctx
            .working_history
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(&call_id))
            .unwrap()
            .content
            .as_text();
        if index == 0 {
            let running: serde_json::Value = serde_json::from_str(&output).unwrap();
            std::fs::write(workspace.join("release-checker"), "release").unwrap();
            assert!(running.get("exit_code").is_none());
            assert!(running["next_wait_args"]["session_id"].is_string(), "expected a running session: {running}");
            assert!(!outcome_ctx.repeated_tool_attempts.piped_verification_notice_pending);
            assert!(
                !outcome_ctx
                    .ctx
                    .working_history
                    .iter()
                    .any(|message| message.content.as_text().contains("Verification was not recorded because"))
            );
            call_id = "masked_checker_poll".to_string();
            let poll = PreparedAssistantToolCall::new(uni::ToolCall::function(
                call_id.clone(),
                tool_names::WRITE_STDIN.to_string(),
                serde_json::to_string(&json!({"session_id":running["next_wait_args"]["session_id"], "action":"wait", "wait_timeout_seconds":10}))
                    .unwrap(),
            ));
            assert!(handle_tool_calls(&mut outcome_ctx, &[poll]).await.unwrap().is_none());
        }
        let output = outcome_ctx
            .ctx
            .working_history
            .iter()
            .find(|message| message.tool_call_id.as_deref() == Some(&call_id))
            .unwrap()
            .content
            .as_text();
        assert!(output.contains("lint exit: 7"), "the checker must really have failed: {output}");
        let payload: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(payload["exit_code"], 0, "the final echo masks the checker's failure");
        let notice_count = outcome_ctx
            .ctx
            .working_history
            .iter()
            .filter(|message| {
                message.role == uni::MessageRole::System
                    && message.content.as_text().contains("Verification was not recorded because")
            })
            .count();
        assert_eq!(notice_count, 1, "repeated completed calls must not repeat coaching");
        if index == 0 {
            let history = &outcome_ctx.ctx.working_history;
            let output_index = history
                .iter()
                .position(|message| message.tool_call_id.as_deref() == Some(&call_id))
                .unwrap();
            let notice_index = history
                .iter()
                .position(|message| {
                    message.role == uni::MessageRole::System
                        && message.content.as_text().contains("Verification was not recorded because")
                })
                .unwrap();
            assert!(output_index < notice_index, "coaching must follow the tool's evidence");
        }
        assert!(!outcome_ctx.repeated_tool_attempts.verification_is_pending());
    }
    std::fs::write(&checker, "print('README: checker passed')\n").unwrap();
    outcome_ctx.repeated_tool_attempts.mark_verification_pending();
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "standalone_checker", tool_names::EXEC_COMMAND, standalone)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!outcome_ctx.repeated_tool_attempts.verification_is_pending());
    assert!(
        handle_single_tool_call(&mut outcome_ctx, "verified_edit", tool_names::APPLY_PATCH, patch)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(std::fs::read_to_string(workspace.join("sample.txt")).unwrap(), "continued after verification\n");
    assert_eq!(outcome_ctx.ctx.harness_state.blocked_tool_calls, 0);
    assert!(!outcome_ctx.ctx.is_recovery_active());
}

#[cfg(unix)]
#[tokio::test]
async fn filtered_verifier_failure_grants_repair_and_success_continues_loop() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    std::fs::create_dir(workspace.join("scripts")).unwrap();
    let checker = workspace.join("scripts/check_markdown.py");
    let command = "python3 scripts/check_markdown.py 2>&1 | grep";
    let args = json!({"cmd":command, "raw_command":command, "args":["README"], "workdir":workspace});
    cache_tool_permission(&mut backing, tool_names::EXEC_COMMAND, &args, PermissionGrant::Permanent).await;
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-hello\n+verified continuation\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch, PermissionGrant::Permanent).await;
    // Completion debt belongs to observed code mutations, not the local snapshot alone.
    let setup_patch = json!({"input":"*** Begin Patch\n*** Add File: verification_fixture.rs\n+pub fn requires_verification() {}\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &setup_patch, PermissionGrant::Permanent).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    handle_single_tool_call(&mut outcome_ctx, "establish_verification_debt", tool_names::APPLY_PATCH, setup_patch)
        .await
        .unwrap();
    assert!(workspace.join("verification_fixture.rs").is_file(), "fixture must establish a real code mutation");
    assert!(outcome_ctx.ctx.tool_registry.completion_verification_pending());
    assert!(outcome_ctx.repeated_tool_attempts.verification_is_pending());
    for (exit, pending) in [(7, true), (0, false)] {
        std::fs::write(
            &checker,
            format!("import sys\nprint('README: checker output')\nprint('unrelated output')\nsys.exit({exit})\n"),
        )
        .unwrap();
        let start = outcome_ctx.ctx.working_history.len();
        handle_single_tool_call(
            &mut outcome_ctx,
            &format!("filtered_check_{exit}"),
            tool_names::EXEC_COMMAND,
            args.clone(),
        )
        .await
        .unwrap();
        let output = outcome_ctx.ctx.working_history[start..]
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .map(|message| message.content.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            outcome_ctx.repeated_tool_attempts.verification_is_pending(),
            pending,
            "exit={exit}; tool responses: {output}"
        );
        assert!(output.contains("README: checker output"), "{output}");
        assert!(!output.contains("unrelated output"), "filter must be preserved: {output}");
        assert_eq!(outcome_ctx.ctx.harness_state.blocked_tool_calls, 0);
    }
    handle_single_tool_call(&mut outcome_ctx, "after_filtered_verification", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(workspace.join("sample.txt")).unwrap(), "verified continuation\n");
    assert!(!outcome_ctx.ctx.is_recovery_active());
}

#[tokio::test]
async fn repeated_paginated_sed_reads_eventually_trip_per_file_path_cap() {
    // turn_911-style regression: simple `sed -n` pagination should behave like
    // file reads. Different ranges must not trip the identical-slice family
    // cap, but repeated exploration of the same file should still block that
    // path without disabling unrelated reads or edits for the turn.
    let mut backing = TestContextBacking::new(20).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let other_file = sample_file.with_file_name("other.txt");
    std::fs::write(&other_file, "other evidence\n").expect("write unrelated read fixture");
    let patch_args = json!({
        "input": "*** Begin Patch\n*** Update File: sample.txt\n@@\n-line 1\n+edited line 1\n*** End Patch\n"
    });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let mut blocked_at = None;
    for idx in 1..=10 {
        let args = json!({
            "cmd": format!("sed -n '{idx},{idx}p' {sample_path}")
        });
        let execution_history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
        let outcome =
            handle_single_tool_call(&mut outcome_ctx, &format!("sed_read_{idx}"), tool_names::EXEC_COMMAND, args)
                .await
                .expect("sed read should complete");

        if outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("repeated_read_path"))
        {
            assert!(outcome.is_none(), "path-specific rejection should not schedule broad recovery");
            assert_eq!(
                outcome_ctx.ctx.tool_registry.execution_history_len(),
                execution_history_len,
                "the capped read must not execute"
            );
            blocked_at = Some(idx);
            break;
        }
    }

    assert_eq!(blocked_at, Some(7), "sed pagination should stop on the shared per-file-path cap");
    assert_eq!(
        outcome_ctx.ctx.harness_state.consecutive_same_file_read_family_calls, 1,
        "different sed ranges must reset the identical-slice family streak"
    );
    assert!(!outcome_ctx.ctx.is_recovery_active(), "path cap must not disable tools for the turn");
    assert!(
        outcome_ctx.ctx.working_history.iter().any(|message| {
            let content = message.content.as_text();
            content.contains("repeated_read_path")
                && content.contains("further reads of this path are blocked")
                && content.to_ascii_lowercase().contains("reads of other paths, edits")
        }),
        "path-cap error should explain its scope and available next actions"
    );

    let path_error_count_before_retry = outcome_ctx
        .ctx
        .working_history
        .iter()
        .filter(|message| message.content.as_text().contains("repeated_read_path"))
        .count();
    let execution_history_len_before_retry = outcome_ctx.ctx.tool_registry.execution_history_len();
    let repeated_path_read = handle_single_tool_call(
        &mut outcome_ctx,
        "sed_read_same_path_blocked",
        tool_names::EXEC_COMMAND,
        json!({"cmd": format!("sed -n '17,17p' {sample_path}")}),
    )
    .await
    .expect("a read of the capped path should be rejected");
    assert!(repeated_path_read.is_none(), "path-scoped blocks do not schedule recovery by themselves");
    assert_eq!(
        outcome_ctx.ctx.tool_registry.execution_history_len(),
        execution_history_len_before_retry,
        "a later read of the capped path must remain unexecuted"
    );
    assert_eq!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .filter(|message| message.content.as_text().contains("repeated_read_path"))
            .count(),
        path_error_count_before_retry + 1,
        "the later read should receive a path-cap error"
    );
    assert!(!outcome_ctx.ctx.is_recovery_active(), "a repeated blocked read must remain path-scoped");

    let execution_history_len_before_other_read = outcome_ctx.ctx.tool_registry.execution_history_len();
    let unrelated_read = handle_single_tool_call(
        &mut outcome_ctx,
        "sed_read_other_path",
        tool_names::EXEC_COMMAND,
        json!({"cmd": format!("sed -n '1,1p' {}", other_file.display())}),
    )
    .await
    .expect("an unrelated read should execute");
    assert!(unrelated_read.is_none());
    assert!(
        outcome_ctx.ctx.tool_registry.execution_history_len() > execution_history_len_before_other_read,
        "an unrelated path read must execute after the capped path is blocked"
    );
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("other evidence")),
        "the unrelated read should return its content"
    );

    let patch =
        handle_single_tool_call(&mut outcome_ctx, "patch_after_path_read_cap", tool_names::APPLY_PATCH, patch_args)
            .await
            .expect("an edit should execute after the path cap");
    assert!(patch.is_none());
    assert_eq!(
        std::fs::read_to_string(sample_file)
            .expect("read edited fixture")
            .lines()
            .next(),
        Some("edited line 1")
    );
    assert!(!outcome_ctx.ctx.is_recovery_active(), "productive tools should remain enabled");
}

#[tokio::test]
async fn denied_tool_permission_emits_policy_response_without_budget_burn() {
    let mut backing = TestContextBacking::new(2).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let denial_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &denial_args, PermissionGrant::Denied).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_single_tool_call(&mut outcome_ctx, "denied", tool_names::READ_FILE, denial_args)
        .await
        .expect("denied permission should be handled");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("execution denied by policy") })
    );
}

#[tokio::test]
async fn prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;

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
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("exceeded max tool calls per turn") })
    );
}

#[tokio::test]
async fn multiple_prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    backing.select_build_primary_agent();
    let second_file = backing.sample_file.parent().expect("temp workspace root").join("other.txt");
    std::fs::write(&second_file, "world\n").expect("write second sample file");

    let tool_calls = vec![
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_read_1".to_string(),
            tool_names::READ_FILE.to_string(),
            serde_json::to_string(&json!({
                "path": backing.sample_file.to_string_lossy()
            }))
            .expect("serialize tool args"),
        )),
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_read_2".to_string(),
            tool_names::READ_FILE.to_string(),
            serde_json::to_string(&json!({
                "path": second_file.to_string_lossy()
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
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("exceeded max tool calls per turn") })
    );
}

#[tokio::test]
async fn end_to_end_blocked_calls_do_not_burn_budget_before_valid_call() {
    let mut backing = TestContextBacking::new(1).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;

    let mut turn_modified_files: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut tp_ctx = backing.turn_processing_context();

    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let blocked_args = json!({"path":"/var/db/shadow"});
    let first = handle_single_tool_call(&mut outcome_ctx, "blocked_1", tool_names::READ_FILE, blocked_args.clone())
        .await
        .expect("first blocked call should not fail hard");
    assert!(first.is_none());

    let second = handle_single_tool_call(&mut outcome_ctx, "blocked_2", tool_names::READ_FILE, blocked_args)
        .await
        .expect("second blocked call should not fail hard");
    assert!(second.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(!outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let third = handle_single_tool_call(&mut outcome_ctx, "valid_1", tool_names::READ_FILE, valid_args.clone())
        .await
        .expect("valid call should execute");
    assert!(third.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let exhausted = handle_single_tool_call(&mut outcome_ctx, "exhausted", tool_names::READ_FILE, valid_args)
        .await
        .expect("exhausted-budget call should return structured outcome");
    // Tool-call budget exhaustion must NOT break the turn as `Blocked` — that
    // skips the synthesis pass. It rejects the call with a policy-violation
    // tool response and lets the post-batch flush push a single synthesis
    // directive (see `flush_budget_synthesis_directives`). A single rejected
    // call is below the blocked-streak cap, so the guard returns no outcome.
    assert!(exhausted.is_none());
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool && message.content.as_text().contains("exceeded max tool calls per turn")
    }));
    let second_exhausted = validate_tool_call(
        outcome_ctx.ctx,
        "exhausted_stub",
        tool_names::READ_FILE,
        &json!({"path": valid_file.to_string_lossy()}),
    )
    .await
    .expect("second budget rejection");
    assert!(matches!(second_exhausted, ValidationResult::Blocked));
    let stub = outcome_ctx
        .ctx
        .working_history
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("exhausted_stub"))
        .expect("tool-call budget rejection response");
    let payload: serde_json::Value = serde_json::from_str(&stub.content.as_text()).unwrap();
    assert!(
        payload["next_action"]
            .as_str()
            .unwrap()
            .contains("Synthesize your final answer now")
    );
    assert!(!stub.content.as_text().contains("narrower scope"));
}

#[tokio::test]
async fn control_plane_wait_survives_exhausted_tool_budget() {
    let mut backing = TestContextBacking::new(1).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.to_string_lossy().to_string();
    let mut turn_modified_files: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut tp_ctx = backing.turn_processing_context();

    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    // Exhaust the one-call budget with a real work call.
    let work_args = json!({"path": sample_file});
    let work = handle_single_tool_call(&mut outcome_ctx, "work_1", tool_names::READ_FILE, work_args)
        .await
        .expect("work call should execute");
    assert!(work.is_none());
    assert!(outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    // A control-plane wait must still be admitted after exhaustion and must
    // not push a budget-exhaustion rejection into the tool history.
    let wait_args = json!({"action": "wait", "session_id": "run-test-1"});
    let wait = handle_single_tool_call(&mut outcome_ctx, "wait_1", tool_names::UNIFIED_EXEC, wait_args)
        .await
        .expect("wait call should return a structured outcome");
    assert!(wait.is_none() || wait.is_some(), "wait must not hard-fail the turn");
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::Tool
                && message.content.as_text().contains("exceeded max tool calls per turn")
                && message.content.as_text().contains("wait")),
        "wait must not be rejected by the budget-exhaustion gate"
    );
    // The exempt call must not increment the ordinary tool-call counter.
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);

    // The exempt call must also not consume the one-shot budget-exhaustion
    // notice: the next real (non-exempt) call still receives the full policy
    // message rather than the compact "call skipped" stub.
    let follow_up_args = json!({"path": sample_file});
    let follow_up = handle_single_tool_call(&mut outcome_ctx, "work_2", tool_names::READ_FILE, follow_up_args)
        .await
        .expect("exhausted follow-up should return a structured outcome");
    assert!(follow_up.is_none());
    let tool_messages = outcome_ctx
        .ctx
        .working_history
        .iter()
        .filter(|message| message.role == uni::MessageRole::Tool)
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>();
    assert!(
        tool_messages
            .iter()
            .any(|text| text.contains("Policy violation: exceeded max tool calls per turn")),
        "the first real rejection must keep the full policy message: {tool_messages:?}"
    );
    assert!(
        !tool_messages
            .iter()
            .any(|text| text.contains("Tool-call budget exhausted for this turn; call skipped.")),
        "an exempt call must not consume the first-notice: {tool_messages:?}"
    );
}

#[tokio::test]
async fn pending_verification_allows_more_than_six_edits_and_repairs() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let code_path = workspace.join("repair.rs");
    std::fs::write(&code_path, "0\n").unwrap();
    backing.tool_registry.allow_all_tools().await.unwrap();
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    for index in 0..9 {
        if index == 3 {
            let mut outcome_ctx = ToolOutcomeContext {
                ctx: &mut ctx,
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
            };
            handle_single_tool_call(
                &mut outcome_ctx,
                "failed-check",
                tool_names::EXEC_COMMAND,
                json!({"cmd":"rustc --invalid-flag-xyz"}),
            )
            .await
            .unwrap();
            let result = ctx
                .working_history
                .iter()
                .rev()
                .find(|message| message.tool_call_id.as_deref() == Some("failed-check"))
                .unwrap();
            let output: serde_json::Value = serde_json::from_str(&result.content.as_text()).unwrap();
            assert_eq!(output["exit_code"], 1, "repair follows a real terminal failure");
        }
        let args = json!({"input": format!("*** Begin Patch\n*** Update File: repair.rs\n@@\n-{index}\n+{}\n*** End Patch\n", index + 1)});
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
        };
        handle_single_tool_call(&mut outcome_ctx, &format!("edit-{index}"), tool_names::APPLY_PATCH, args)
            .await
            .unwrap();
        assert_eq!(std::fs::read_to_string(&code_path).unwrap(), format!("{}\n", index + 1));
    }
    assert!(ctx.tool_registry.completion_verification_pending());
    assert_eq!(ctx.harness_state.blocked_tool_calls, 0);
}

#[tokio::test]
async fn cached_heading_inspection_adopted_as_required_check_executes_fresh() {
    for (required_contents, required_exit) in [("No heading here\n", 1), ("# Actual heading\n", 0)] {
        let mut backing = TestContextBacking::new(20).await;
        backing.select_build_primary_agent();
        backing.tool_registry.allow_all_tools().await.unwrap();
        let workspace = backing.sample_file.parent().unwrap().to_path_buf();
        let readme = workspace.join("README.md");
        std::fs::write(&readme, "# Cached heading\n").unwrap();
        let command = "grep -n '^#' README.md";
        let args = json!({"cmd":command});
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        for (id, required, contents, exit_code) in [
            ("prime", false, "# Cached heading\n", 0),
            ("required", true, required_contents, required_exit),
        ] {
            std::fs::write(&readme, contents).unwrap();
            if required {
                let adopted = ctx
                    .tool_registry
                    .execute_tool(
                        tool_names::TASK_TRACKER,
                        json!({"action":"create", "items":[{"description":"Check headings", "verify":[command]}]}),
                    )
                    .await
                    .unwrap();
                assert!(adopted.get("error").is_none(), "{adopted}");
                ctx.tool_registry.restore_completion_verification_debt();
            }
            let mut outcome_ctx = ToolOutcomeContext {
                ctx: &mut ctx,
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
            };
            handle_single_tool_call(&mut outcome_ctx, id, tool_names::EXEC_COMMAND, args.clone())
                .await
                .unwrap();
            let response = ctx
                .working_history
                .iter()
                .rev()
                .find(|m| m.tool_call_id.as_deref() == Some(id))
                .unwrap();
            let output: serde_json::Value = serde_json::from_str(&response.content.as_text()).unwrap();
            assert_eq!(output["exit_code"], exit_code, "{output}");
            assert!(output.get("reused_recent_result").is_none(), "required checks must execute: {output}");
            if required {
                assert_eq!(ctx.tool_registry.completion_verifier_passed(command), exit_code == 0);
                assert_eq!(ctx.tool_registry.completion_verification_pending(), exit_code != 0);
            }
        }
    }
}

#[tokio::test]
async fn repeated_read_only_guard_dedups_plan_file_in_planning_mode() {
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();

    // Create a runtime-owned plan file inside the temporary workspace so the
    // plan-artifact fast path (`.vtcode/plans/`) is exercised.
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let plans_dir = workspace.join(".vtcode").join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_path = plans_dir.join("modular-dreaming-pixel.md");
    let plan_content = "# Plan\n\n1. Fix planning-mode clarity\n2. Throttle memory envelopes\n";
    std::fs::write(&plan_path, plan_content).expect("write plan file");

    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.enable_planning();

    let args = json!({
        "path": plan_path.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(&mut outcome_ctx, "read_plan_1", tool_names::READ_FILE, args.clone())
        .await
        .expect("first plan-file read should succeed");
    assert!(first.is_none(), "first read should execute normally");

    let second = handle_single_tool_call(&mut outcome_ctx, "read_plan_2", tool_names::READ_FILE, args)
        .await
        .expect("second plan-file read should be deduplicated");
    assert!(second.is_none(), "duplicate read should be handled by guard");

    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool && message.content.as_text().contains("\"reused_recent_result\":true")
    }));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool
            && message
                .content
                .as_text()
                .contains("was already read. Stop re-reading and finalize the plan.")
    }));
}

async fn execute_tracker_regression_call(
    outcome_ctx: &mut ToolOutcomeContext<'_, '_>,
    call_id: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    let outcome = handle_single_tool_call(outcome_ctx, call_id, tool_names::TASK_TRACKER, args)
        .await
        .expect("tracker call should execute");
    assert!(outcome.is_none());
    assert!(outcome_ctx.ctx.tool_registry.execution_history_len() > history_len, "tracker must execute live");
    let response = outcome_ctx
        .ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(call_id))
        .expect("tracker response");
    let payload: serde_json::Value = serde_json::from_str(&response.content.as_text()).expect("tracker JSON");
    assert!(payload.get("error").is_none(), "{payload}");
    assert!(
        payload.get("reused_recent_result").is_none(),
        "stateful tracker results must never replay: {payload}"
    );
    payload
}

#[tokio::test]
async fn tracker_repeated_lists_observe_current_progress_without_replay() {
    struct TrackerCallCase {
        call_id: &'static str,
        args: serde_json::Value,
        expected_completed: usize,
    }
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let calls = [
        TrackerCallCase {
            call_id: "create",
            args: json!({"action":"create", "title":"README work", "items":["Revise copy", "Check links"]}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "list_before",
            args: json!({"action":"list"}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "list_before_again",
            args: json!({"action":"list"}),
            expected_completed: 0,
        },
        TrackerCallCase {
            call_id: "finish_links",
            args: json!({"action":"update", "index":2, "status":"completed"}),
            expected_completed: 1,
        },
        TrackerCallCase {
            call_id: "list_after",
            args: json!({"action":"list"}),
            expected_completed: 1,
        },
        TrackerCallCase {
            call_id: "list_after_again",
            args: json!({"action":"list"}),
            expected_completed: 1,
        },
    ];
    for case in &calls {
        cache_tool_permission(&mut backing, tool_names::TASK_TRACKER, &case.args, PermissionGrant::Permanent).await;
    }
    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    for case in calls {
        let payload = execute_tracker_regression_call(&mut outcome_ctx, case.call_id, case.args).await;
        assert_eq!(payload["checklist"]["completed"], case.expected_completed, "{}: {payload}", case.call_id);
        assert_eq!(payload["checklist"]["total"], 2);
    }
    handle_single_tool_call(&mut outcome_ctx, "list_over_limit", tool_names::TASK_TRACKER, json!({"action":"list"}))
        .await
        .expect("excessive tracker polling should return policy feedback");
    let response = outcome_ctx
        .ctx
        .working_history
        .iter()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some("list_over_limit"))
        .expect("blocked tracker response");
    let payload: serde_json::Value = serde_json::from_str(&response.content.as_text()).expect("tracker JSON");
    assert_eq!(payload["error_class"], "policy_blocked", "repeat limits must remain active: {payload}");
    assert!(payload.get("reused_recent_result").is_none());
}

#[tokio::test]
async fn tracker_repeated_update_targets_replaced_checklist_instead_of_old_history() {
    let mut backing = TestContextBacking::new(12).await;
    backing.select_build_primary_agent();
    let old_create = json!({"action":"create", "title":"Planning", "items":["Inspect README", "Draft plan"]});
    let finish_second = json!({"action":"update", "index":2, "status":"completed"});
    let new_create =
        json!({"action":"create", "title":"Implementation", "items":["[x] Revise copy", "Check new links"]});
    for args in [&old_create, &finish_second, &new_create] {
        cache_tool_permission(&mut backing, tool_names::TASK_TRACKER, args, PermissionGrant::Permanent).await;
    }
    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    execute_tracker_regression_call(&mut outcome_ctx, "old_create", old_create).await;
    let old_update = execute_tracker_regression_call(&mut outcome_ctx, "old_update", finish_second.clone()).await;
    assert_eq!(old_update["checklist"]["completed"], 1);
    execute_tracker_regression_call(&mut outcome_ctx, "new_create", new_create).await;
    outcome_ctx.ctx.tool_registry.begin_tracker_request(false);
    let new_update = execute_tracker_regression_call(&mut outcome_ctx, "new_update", finish_second).await;
    assert_eq!(new_update["checklist"]["completed"], 2);
    assert_eq!(new_update["changed_item"]["description"], "Check new links");
    assert!(
        outcome_ctx.ctx.tool_registry.tracker_adopted_for_request(),
        "a repeated update must adopt the current request"
    );
}

#[tokio::test]
async fn repeated_registry_results_keep_new_inspections_visible() {
    for planning in [false, true] {
        let mut backing = TestContextBacking::new(120).await;
        backing.select_build_primary_agent();
        if planning {
            backing.tool_registry.enable_planning();
        }
        let workspace = backing.sample_file.parent().unwrap().to_path_buf();
        let paths = (0..16)
            .map(|index| {
                let path = workspace.join(format!("evidence-{index}.txt"));
                std::fs::write(&path, format!("evidence-{index} {}\n", "x".repeat(8000))).unwrap();
                path
            })
            .collect::<Vec<_>>();
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
        };
        for (index, path) in paths.iter().enumerate() {
            let result = handle_single_tool_call(
                &mut outcome_ctx,
                &format!("read-{index}"),
                tool_names::READ_FILE,
                json!({"path": path, "limit": 2_000, "condense": false}),
            )
            .await
            .unwrap();
            assert!(result.is_none(), "a productive read must not trigger a recovery outcome");
            let stored = outcome_ctx
                .ctx
                .working_history
                .iter()
                .rev()
                .find(|message| message.role == uni::MessageRole::Tool)
                .unwrap()
                .content
                .as_text();
            assert!(stored.contains(&format!("evidence-{index}")), "fresh evidence must remain visible: {stored}");
            assert!(!stored.contains("preview_budget_exhausted"));
        }
        assert!(!outcome_ctx.ctx.is_recovery_active());
    }
}

#[tokio::test]
async fn spool_page_remains_visible_after_large_results() {
    use crate::agent::runloop::unified::turn::tool_outcomes::handlers::guards::spool_guard::enforce_spool_chunk_read_guard;
    let mut backing = TestContextBacking::new(8).await;
    let mut ctx = backing.turn_processing_context();
    ctx.push_tool_response("large", Some(tool_names::EXEC_COMMAND), "x".repeat(100_000));
    let passed = enforce_spool_chunk_read_guard(
        &mut ctx,
        "page",
        tool_names::READ_FILE,
        &json!({"path": ".vtcode/context/tool_outputs/write_stdin_run-abc123.txt"}),
    )
    .await;
    assert!(passed.is_none());
    let page = "visible page content ".repeat(256);
    ctx.push_tool_response("page", Some(tool_names::READ_FILE), page.clone());
    assert!(ctx.working_history.last().unwrap().content.as_text().contains(&page));
    assert!(!ctx.is_recovery_active());
}

#[tokio::test]
async fn planning_mode_allows_request_user_input_blocked_through_to_failure() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let args = json!({"questions": [{"id": "q1", "header": "Q1", "question": "Test?"}]});

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "interview_blocked", tool_names::REQUEST_USER_INPUT, &args);

    assert!(
        outcome.is_none(),
        "Blocked request_user_input should flow through to handle_failure, not fast-exit from guard"
    );
    assert!(
        !ctx.plan_session.is_interview_denied(),
        "Interview denial is handled by handle_failure in execution_result.rs, not the guard"
    );
    assert_eq!(ctx.harness_state.consecutive_blocked_tool_calls, 1, "blocked call should be recorded");
}

#[tokio::test]
async fn permanent_interview_denial_switches_to_tool_free_plan_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    backing
        .tool_registry
        .set_tool_policy(tool_names::REQUEST_USER_INPUT, ToolPolicy::Deny)
        .await
        .expect("deny interview tool");

    let mut ctx = backing.turn_processing_context();
    let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        "interview_denied".to_string(),
        tool_names::REQUEST_USER_INPUT.to_string(),
        json!({"questions": [{"id": "q1", "header": "Q1", "question": "Test?"}]}).to_string(),
    ));
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &[call])
        .await
        .expect("denied interview should be handled as a tool response");

    assert!(outcome.is_none());
    assert!(outcome_ctx.ctx.plan_session.is_interview_denied());
    assert!(outcome_ctx.ctx.harness_state.is_recovery_active());
    assert!(outcome_ctx.ctx.harness_state.recovery_is_tool_free());
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("synthesize exactly one completed `<proposed_plan>`")
    }));
}

#[tokio::test]
async fn planning_mode_non_planning_tool_blocked_calls_schedule_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing.tool_registry.enable_planning();
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path": "src/main.rs"});
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);

    for idx in 0..max_streak {
        let outcome =
            enforce_blocked_tool_call_guard(&mut ctx, &format!("blocked_{idx}"), tool_names::READ_FILE, &args);
        assert!(outcome.is_none(), "non-planning blocked call {idx} should stay under cap");
    }

    let outcome = enforce_blocked_tool_call_guard(&mut ctx, "blocked_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(ctx.harness_state.blocked_tool_recovery_pending());
}

#[tokio::test]
async fn blocked_batch_drains_responses_and_schedules_tool_free_recovery() {
    let mut backing = TestContextBacking::new(4).await;
    backing
        .tool_registry
        .set_tool_policy(tool_names::READ_FILE, ToolPolicy::Deny)
        .await
        .expect("deny read_file");
    let mut ctx = backing.turn_processing_context();
    ctx.full_auto = true;
    let args_for_call = |index: usize| json!({"path": format!("src/main_{index}.rs")});
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let tool_calls = (0..=max_streak)
        .map(|index| {
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                format!("blocked_batch_{index}"),
                tool_names::READ_FILE.to_string(),
                args_for_call(index).to_string(),
            ))
        })
        .collect::<Vec<_>>();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_tool_calls(&mut outcome_ctx, &tool_calls)
        .await
        .expect("blocked batch should schedule recovery");

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
    assert!(outcome_ctx.ctx.harness_state.is_recovery_active());
    assert!(outcome_ctx.ctx.harness_state.recovery_is_tool_free());
    assert_eq!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .count(),
        tool_calls.len(),
        "every assistant tool call needs a response before recovery"
    );
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message.content.as_text().contains("tools are disabled for one bounded pass")
    }));
}

#[tokio::test]
async fn single_tool_call_dispatch_records_requested_tool_calls() {
    // Checkpoints turn_912/913/917 showed `requested_tool_calls: 0` alongside
    // `admitted_tool_calls: N`: only the full-auto batch path recorded the
    // requested count. Recording moved to the shared dispatch so every path
    // counts identically.
    let mut backing = TestContextBacking::new(4).await;
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;

    let mut ctx = backing.turn_processing_context();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
        "single_call".to_string(),
        tool_names::READ_FILE.to_string(),
        valid_args.to_string(),
    ));
    handle_tool_calls(&mut outcome_ctx, &[call])
        .await
        .expect("single valid call should execute");

    let diagnostics = ctx.harness_state.snapshot_turn_diagnostics(Default::default(), 0);
    assert_eq!(diagnostics.requested_tool_calls, 1, "single-call dispatch must record requested tool calls");
    assert_eq!(diagnostics.admitted_tool_calls, 1);
}

#[tokio::test]
async fn read_after_write_guard_blocks_bare_full_read_of_written_file() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.record_written_file(&sample_path);

    let args = json!({ "path": sample_path });
    let outcome = enforce_read_after_write_guard(&mut ctx, "bare_read_after_write", tool_names::READ_FILE, &args);
    assert!(
        matches!(outcome, Some(ValidationResult::Blocked)),
        "a bare full read of a just-written file must stay blocked"
    );
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "the block must carry the diff-preview guidance"
    );
}

#[tokio::test]
async fn read_after_write_guard_admits_bounded_slice_read_of_written_file() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.record_written_file(&sample_path);

    // The block message directs the model to "specify offset/limit for a
    // specific range" — that advice must be truthful, so a bounded slice
    // read passes. Repeated slice reads stay bounded by the family and
    // per-file-path caps enforced after this guard.
    let args = json!({ "path": sample_path, "offset": 1, "limit": 10 });
    let outcome = enforce_read_after_write_guard(&mut ctx, "slice_read_after_write", tool_names::READ_FILE, &args);
    assert!(outcome.is_none(), "a read with an explicit offset/limit slice must be admitted");

    let raw_flagged_args = json!({ "path": sample_path, "raw": true });
    let raw_outcome =
        enforce_read_after_write_guard(&mut ctx, "raw_only_read_after_write", tool_names::READ_FILE, &raw_flagged_args);
    assert!(
        matches!(raw_outcome, Some(ValidationResult::Blocked)),
        "a raw flag without a slice is an uncondensed full re-read and must stay blocked"
    );
}

#[tokio::test]
async fn read_after_write_guard_ignores_unwritten_paths_and_non_read_tools() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();

    // No write recorded yet: even a bare read passes.
    let unwritten_args = json!({ "path": sample_path });
    let outcome = enforce_read_after_write_guard(&mut ctx, "read_unwritten", tool_names::READ_FILE, &unwritten_args);
    assert!(outcome.is_none(), "reads of paths not written this turn must pass");

    // Written path but a non-read action on the unified file tool: not the
    // guard's concern.
    ctx.harness_state.record_written_file(&sample_path);
    let mutating_args = json!({ "action": "write", "path": sample_path, "content": "x" });
    let non_read_outcome =
        enforce_read_after_write_guard(&mut ctx, "write_after_write", tool_names::UNIFIED_FILE, &mutating_args);
    assert!(non_read_outcome.is_none(), "non-read actions must not trip the read-after-write guard");
}

#[tokio::test]
async fn successful_apply_patch_arms_read_after_write_guard_for_patch_targets() {
    // The model-facing write surface is apply_patch with `input`-only args
    // (the schema has no `path` field), so arming must come from the parsed
    // patch targets, and the workspace-relative target must match an
    // absolute-path read of the same file.
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=4).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch_args = json!({
        "input": "*** Begin Patch\n*** Update File: sample.txt\n@@\n-line 1\n+edited line 1\n*** End Patch\n"
    });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;
    let read_args = json!({ "path": sample_path });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
    let slice_args = json!({ "path": sample_path, "offset": 2, "limit": 2 });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &slice_args, PermissionGrant::Permanent).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_single_tool_call(&mut outcome_ctx, "patch_arm", tool_names::APPLY_PATCH, patch_args)
        .await
        .expect("patch should execute");
    assert_eq!(
        std::fs::read_to_string(&sample_file)
            .expect("read patched fixture")
            .lines()
            .next(),
        Some("edited line 1"),
        "patch must succeed for the arming path to be exercised"
    );

    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "bare_read_after_patch", tool_names::READ_FILE, read_args)
        .await
        .expect("a blocked read still produces a tool response");
    assert_eq!(
        outcome_ctx.ctx.tool_registry.execution_history_len(),
        history_len,
        "a bare full read of the patched file must not execute"
    );
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "the block must carry the diff-preview guidance"
    );

    handle_single_tool_call(&mut outcome_ctx, "slice_read_after_patch", tool_names::READ_FILE, slice_args)
        .await
        .expect("a bounded slice read of the patched file should execute");
    assert!(
        outcome_ctx.ctx.tool_registry.execution_history_len() > history_len,
        "the slice-read exemption must apply to patch-armed targets too"
    );
}

#[tokio::test]
async fn failed_apply_patch_does_not_arm_read_after_write_guard() {
    // A failed patch wrote nothing, so there is no diff preview to reuse and
    // plain reads must stay available for inspecting the failure.
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch_args = json!({ "input": "not a patch at all" });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;
    let read_args = json!({ "path": sample_path });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_single_tool_call(&mut outcome_ctx, "failed_patch", tool_names::APPLY_PATCH, patch_args)
        .await
        .expect("a malformed patch returns an error response");
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "a failed patch must not produce a read-after-write block"
    );

    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "read_after_failed_patch", tool_names::READ_FILE, read_args)
        .await
        .expect("read after failed patch should execute");
    assert!(
        outcome_ctx.ctx.tool_registry.execution_history_len() > history_len,
        "reads must stay available after a failed mutation"
    );
}

#[tokio::test]
async fn patch_context_mismatch_gets_one_fresh_read_after_path_cap_then_corrected_edit() {
    assert_mismatch_recovery_at_read_cap(false).await;
}

#[tokio::test]
async fn literal_mismatch_gets_one_fresh_read_after_path_cap_then_corrected_edit() {
    assert_mismatch_recovery_at_read_cap(true).await;
}

async fn assert_mismatch_recovery_at_read_cap(literal: bool) {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, "old evidence\nsecond line\nthird line\n").expect("fixture");
    let sample_path = sample_file.to_string_lossy().to_string();
    let read_args = json!({"path":sample_path, "offset":1, "limit":2});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
    let bad_patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-vtcode.toml provider credentials\n+edited\n*** End Patch\n"});
    let failure_tool = if literal {
        tool_names::EDIT_FILE
    } else {
        tool_names::APPLY_PATCH
    };
    let bad_patch = if literal {
        json!({"path":"sample.txt", "old_str":"vtcode.toml provider credentials", "new_str":"edited"})
    } else {
        bad_patch
    };
    cache_tool_permission(&mut backing, failure_tool, &bad_patch, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    handle_single_tool_call(&mut outcome_ctx, "initial_read", tool_names::READ_FILE, read_args.clone())
        .await
        .expect("seed cache");
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("old evidence"))
    );
    // Fill the path counter without tripping the independent identical-slice guard.
    for _ in 1..6 {
        outcome_ctx.ctx.harness_state.record_file_read_path_call(sample_path.clone());
    }
    // Simulate external file changes; a replay would return the previous text.
    std::fs::write(&sample_file, "fresh evidence\nsecond line\nthird line\n").expect("external update");
    // Accept the disk version in the conflict monitor only. Retain the stale
    // tool-result cache so this fixture isolates uncached mismatch recovery.
    outcome_ctx
        .ctx
        .tool_registry
        .edited_file_monitor_ref()
        .accept_disk_version(&sample_file)
        .await
        .unwrap();
    handle_single_tool_call(&mut outcome_ctx, "mismatched_patch", failure_tool, bad_patch.clone())
        .await
        .expect("typed patch mismatch");
    assert_eq!(std::fs::read_to_string(&sample_file).unwrap(), "fresh evidence\nsecond line\nthird line\n");
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    let history_start = outcome_ctx.ctx.working_history.len();
    let mut source_call = uni::Message::assistant(String::new());
    source_call.tool_calls = Some(vec![uni::ToolCall::function(
        "fresh_recovery_read".into(),
        tool_names::READ_FILE.into(),
        read_args.to_string(),
    )]);
    outcome_ctx.ctx.working_history.push(source_call);
    handle_single_tool_call(&mut outcome_ctx, "fresh_recovery_read", tool_names::READ_FILE, read_args.clone())
        .await
        .expect("one read beyond cap");
    let outputs = outcome_ctx.ctx.working_history[history_start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(outputs.contains("fresh evidence"), "{outputs}");
    assert!(!outputs.contains("old evidence"), "cached evidence must not be reused: {outputs}");
    assert!(!outputs.contains("reused_recent_result"), "{outputs}");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    // Another failure on the same path must not replenish the allowance.
    handle_single_tool_call(&mut outcome_ctx, "repeated_mismatch", failure_tool, bad_patch)
        .await
        .expect("second mismatch");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    let before_block = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "second_recovery_read", tool_names::READ_FILE, read_args)
        .await
        .expect("read cap rejection");
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), before_block);
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("repeated_read_path"))
    );
    outcome_ctx
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcome_ctx.ctx.working_history);
    let corrected = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-fresh evidence\n+edited evidence\n*** End Patch\n"});
    let corrected = if literal {
        json!({"path":"sample.txt", "old_str":"fresh evidence", "new_str":"edited evidence"})
    } else {
        corrected
    };
    handle_single_tool_call(&mut outcome_ctx, "corrected_patch", failure_tool, corrected)
        .await
        .expect("corrected edit");
    assert_eq!(std::fs::read_to_string(&sample_file).unwrap(), "edited evidence\nsecond line\nthird line\n");
    assert!(!outcome_ctx.turn_modified_files.is_empty(), "successful mutation must be reported");
    assert!(!outcome_ctx.ctx.is_recovery_active());
}

#[test]
fn patch_preflight_correction_names_canonical_json_input() {
    for (name, error) in [
        (tool_names::APPLY_PATCH, "Missing required argument: input"),
        (
            tool_names::APPLY_PATCH,
            "Tool preflight validation failed for 'apply_patch': Missing required argument: patch",
        ),
        (tool_names::EXEC_COMMAND, "apply_patch is a tool, not a shell executable"),
    ] {
        let correction = preflight_schema_correction(name, error);
        assert!(correction.contains(r#"{"input":"*** Begin Patch\n"#));
        assert!(correction.contains("retry once"));
        assert!(correction.contains("do not run apply_patch in a shell"));
    }
}

#[tokio::test]
async fn patch_context_mismatch_allows_one_uncached_shell_range_after_six_reads() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, "old shell evidence\nsecond\nthird\nfourth\nfifth\nsixth\n").unwrap();
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-vtcode.toml credentials\n+edited\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    for index in 1..=6 {
        handle_single_tool_call(
            &mut outcome_ctx,
            &format!("shell_page_{index}"),
            tool_names::EXEC_COMMAND,
            json!({"cmd":format!("sed -n '{index},{index}p' {sample_path}")}),
        )
        .await
        .unwrap();
    }
    std::fs::write(&sample_file, "fresh shell evidence\nsecond\nthird\nfourth\nfifth\nsixth\n").unwrap();
    handle_single_tool_call(&mut outcome_ctx, "shell_patch_mismatch", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    let args = json!({"cmd":format!("sed -n '1,1p' {sample_path}")});
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::EXEC_COMMAND, &args)
    );
    // Cached identical reads can already be at the family boundary when a
    // mismatch asks for this same slice again. The one fresh read covers both caps.
    for _ in 0..3 {
        outcome_ctx
            .ctx
            .harness_state
            .record_file_read_family_call(format!("unified_exec::read::{sample_path}::off=1::lim=1"));
    }
    let start = outcome_ctx.ctx.working_history.len();
    let mut source_call = uni::Message::assistant(String::new());
    source_call.tool_calls = Some(vec![uni::ToolCall::function(
        "fresh_shell_range".into(),
        tool_names::EXEC_COMMAND.into(),
        args.to_string(),
    )]);
    outcome_ctx.ctx.working_history.push(source_call);
    handle_single_tool_call(&mut outcome_ctx, "fresh_shell_range", tool_names::EXEC_COMMAND, args.clone())
        .await
        .unwrap();
    let outputs = outcome_ctx.ctx.working_history[start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(outputs.contains("fresh shell evidence"), "{outputs}");
    assert!(!outputs.contains("old shell evidence"), "{outputs}");
    assert!(!outputs.contains("repeated_read_path"), "{outputs}");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::EXEC_COMMAND, &args)
    );
    outcome_ctx
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcome_ctx.ctx.working_history);
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-fresh shell evidence\n+edited shell evidence\n*** End Patch\n"});
    handle_single_tool_call(&mut outcome_ctx, "fixed_shell_patch", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&sample_file)
            .unwrap()
            .starts_with("edited shell evidence\n")
    );
}

#[tokio::test]
async fn patch_recovery_path_cap_exception_is_reserved_before_batch_execution() {
    for prior_reads in [5, 6] {
        let mut backing = TestContextBacking::new(30).await;
        backing.select_build_primary_agent();
        let sample_path = backing.sample_file.to_string_lossy().to_string();
        let bad_patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-missing context\n+edited\n*** End Patch\n"});
        cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &bad_patch, PermissionGrant::Permanent).await;
        let read_args = json!({"path":sample_path, "offset":1, "limit":2});
        cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        handle_single_tool_call(&mut outcome_ctx, "mismatch", tool_names::APPLY_PATCH, bad_patch)
            .await
            .expect("typed mismatch");
        for _ in 0..prior_reads {
            outcome_ctx.ctx.harness_state.record_file_read_path_call(sample_path.clone());
        }
        let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
        let first = validate_tool_call(outcome_ctx.ctx, "first_batch_read", tool_names::READ_FILE, &read_args)
            .await
            .expect("first batch admission");
        assert!(matches!(first, ValidationResult::Proceed(_)), "prior reads: {prior_reads}");
        let second = validate_tool_call(
            outcome_ctx.ctx,
            "second_batch_read",
            tool_names::READ_FILE,
            &json!({"path":sample_path, "offset":3, "limit":2}),
        )
        .await
        .expect("second batch admission");
        assert!(matches!(second, ValidationResult::ReadCapBlocked), "prior reads: {prior_reads}");
        assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), history_len);
        assert!(
            outcome_ctx
                .ctx
                .tool_registry
                .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
        );
    }
}

#[tokio::test]
async fn patch_recovery_rejects_ignored_limit_alias_at_the_path_cap() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let path = backing.sample_file.to_string_lossy().to_string();
    std::fs::write(&backing.sample_file, (1..=400).map(|line| format!("fixture-{line}\n")).collect::<String>())
        .unwrap();
    let bad_patch =
        json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-missing\n+edited\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &bad_patch, PermissionGrant::Permanent).await;
    let bounded = json!({"path":path, "limit":1, "condense":false});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &bounded, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    handle_single_tool_call(&mut outcome_ctx, "mismatch", tool_names::APPLY_PATCH, bad_patch)
        .await
        .unwrap();
    for _ in 0..6 {
        outcome_ctx.ctx.harness_state.record_file_read_path_call(path.clone());
    }
    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(
        &mut outcome_ctx,
        "ignored_limit",
        tool_names::READ_FILE,
        json!({"path":path, "limit_lines":1, "condense":false}),
    )
    .await
    .unwrap();
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), history_len);
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &bounded)
    );
    let history_start = outcome_ctx.ctx.working_history.len();
    handle_single_tool_call(&mut outcome_ctx, "bounded_read", tool_names::READ_FILE, bounded)
        .await
        .unwrap();
    let output = outcome_ctx.ctx.working_history[history_start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(output.contains("fixture-1"), "{output}");
    assert!(!output.contains("fixture-201"), "{output}");
    assert!(!output.contains("fixture-400"), "{output}");
}

#[tokio::test]
async fn preflight_archived_tracker_fixture_stops_after_one_corrective_turn() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/tracker_preflight_session.json")).unwrap();
    let sequence = fixture["sequence"].as_array().unwrap();
    let arguments = fixture["rejected_arguments"].as_array().unwrap();
    assert_eq!(sequence.len(), 39);
    assert_eq!(arguments.len(), 6);
    assert_eq!(fixture["baseline_internal_continuations"], 17);
    let mut config = vtcode_core::config::VTCodeConfig::default();
    config.tools.max_consecutive_blocked_tool_calls_per_turn = 6;
    let mut backing = TestContextBacking::new(32).await;
    backing.select_build_primary_agent();
    assert!(backing.session_stats.record_tracker_continuation_turn_with_limit(3));
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut calls = 0;
    let mut continuations = 0;
    let mut distinct = BTreeSet::new();
    for index in sequence {
        let args = arguments[index.as_u64().unwrap() as usize].clone();
        distinct.insert(args.to_string());
        let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
            format!("fixture-{calls}"),
            tool_names::TASK_TRACKER.to_string(),
            args.to_string(),
        ));
        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(&config);
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        let outcome = handle_tool_calls(&mut outcome_ctx, &[call]).await.unwrap();
        calls += 1;
        if ctx.session_stats.preflight_recovery.exhausted() {
            break;
        }
        if matches!(outcome, Some(TurnHandlerOutcome::Continue)) {
            assert!(ctx.session_stats.preflight_recovery.corrective_pending());
            assert_eq!(continuations, 0);
            continuations += 1;
            ctx.session_stats.preflight_recovery.begin_corrective_turn();
            let prompt = format!(
                "{} Correct rejected tracker arguments.",
                crate::agent::runloop::unified::state::PREFLIGHT_CORRECTIVE_FOLLOW_UP_PREFIX
            );
            assert_eq!(
                ctx.session_stats.register_follow_up_prompt(&prompt),
                crate::agent::runloop::unified::state::FollowUpPromptAction::None
            );
            *ctx.harness_state =
                HarnessTurnState::new(TurnRunId("corrective".into()), TurnId("corrective".into()), 32, 60, 0);
        }
    }
    assert_eq!(
        calls, 7,
        "six initial failures, one changed corrective call, then its semantic repeat stops recovery: \
         cosmetic status/max_output_tokens variation on the same invalid `list` cannot open another turn"
    );
    // One `create` variant collapses into the lead `create`'s identity, so the
    // independently counted exact duplicates changed with semantic identity.
    assert_eq!(calls - distinct.len(), 2, "independently counted duplicate calls");
    assert_eq!(continuations, 1);
    assert!(backing.session_stats.preflight_recovery.exhausted());
    assert_eq!(
        backing.session_stats.tracker_continuation_turns(),
        1,
        "validation must not replenish tracker budgets"
    );
    assert_eq!(
        backing
            .working_history
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .count(),
        calls
    );
    assert!(
        !backing
            .working_history
            .iter()
            .any(|message| message.role == uni::MessageRole::System
                && message.content.as_text().contains("tools are disabled"))
    );
}

#[tokio::test]
async fn preflight_corrective_turn_revalidates_live_schema_and_stops_unchanged_calls() {
    for correct in [true, false] {
        let mut backing = TestContextBacking::new(32).await;
        backing.select_build_primary_agent();
        let args = json!({"action":"list","index":2});
        let mut ctx = backing.turn_processing_context();
        assert!(
            ctx.tool_registry
                .admit_public_tool_call(tool_names::TASK_TRACKER, &args)
                .is_err()
        );
        ctx.session_stats.preflight_recovery.reject(
            tool_names::TASK_TRACKER,
            crate::agent::runloop::unified::state::ValidationCategory::Arguments,
            Some(&args),
            6,
            false,
        );
        ctx.session_stats.preflight_recovery.synthesis();
        ctx.session_stats.preflight_recovery.begin_corrective_turn();
        assert!(ctx.session_stats.record_tracker_continuation_turn_with_limit(3));
        let retry = if correct {
            json!({"action":"list"})
        } else {
            json!({"index":2,"action":"list","max_output_tokens":20})
        };
        let result = validate_tool_call(&mut ctx, "corrective", tool_names::TASK_TRACKER, &retry)
            .await
            .unwrap();
        if correct {
            assert!(matches!(result, ValidationResult::Proceed(_)));
            assert!(!ctx.session_stats.preflight_recovery.is_clear(), "admission alone must not clear recovery");
            let mut repeated_tool_attempts = LoopTracker::new();
            let mut turn_modified_files = BTreeSet::new();
            let mut outcome_ctx = ToolOutcomeContext {
                ctx: &mut ctx,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
            };
            handle_single_tool_call(&mut outcome_ctx, "corrected-execution", tool_names::TASK_TRACKER, retry)
                .await
                .unwrap();
            assert!(ctx.session_stats.preflight_recovery.is_clear());
            assert_eq!(ctx.session_stats.tracker_continuation_turns(), 1);
        } else {
            assert!(matches!(result, ValidationResult::Outcome(TurnHandlerOutcome::Break(_))));
            assert!(ctx.session_stats.preflight_recovery.exhausted());
            let response = ctx.working_history.last().unwrap().content.as_text();
            assert!(response.contains("Repeated preflight rejection"));
            assert!(!response.contains("Expected schema"), "repeat feedback must remain compact");
        }
    }
}

#[tokio::test]
async fn preflight_batch_closes_every_invocation_and_retains_parsed_arguments() {
    use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
    for cancelled in [false, true] {
        let mut backing = TestContextBacking::new(32).await;
        backing.select_build_primary_agent();
        let workspace = backing.sample_file.parent().unwrap().to_path_buf();
        let emitter = HarnessEventEmitter::new_async(&workspace, "preflight-batch", None)
            .await
            .unwrap();
        let valid = json!({"path":backing.sample_file});
        cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid, PermissionGrant::Permanent).await;
        let invalid = json!({"action":"list","index":3});
        let calls = [
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                "admitted-first".into(),
                tool_names::READ_FILE.into(),
                valid.to_string(),
            )),
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                "invalid".into(),
                tool_names::TASK_TRACKER.into(),
                invalid.to_string(),
            )),
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                "malformed".into(),
                tool_names::TASK_TRACKER.into(),
                "{not-json".into(),
            )),
        ];
        let mut config = vtcode_core::config::VTCodeConfig::default();
        config.tools.max_consecutive_blocked_tool_calls_per_turn = 1;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(&config);
            ctx.harness_emitter = Some(&emitter);
            if cancelled {
                ctx.ctrl_c_state.request_local_cancel();
            }
            let mut outcome_ctx = ToolOutcomeContext {
                ctx: &mut ctx,
                repeated_tool_attempts: &mut repeated_tool_attempts,
                turn_modified_files: &mut turn_modified_files,
            };
            let outcome = handle_tool_calls(&mut outcome_ctx, &calls).await.unwrap();
            assert!(outcome.is_some());
            if cancelled {
                assert!(matches!(
                    outcome,
                    Some(TurnHandlerOutcome::Break(
                        crate::agent::runloop::unified::turn::context::TurnLoopResult::Cancelled
                    ))
                ));
                assert!(!ctx.session_stats.preflight_recovery.corrective_pending());
                assert!(!ctx.harness_state.recovery_is_tool_free());
            }
            for call in &calls {
                assert_eq!(
                    ctx.working_history
                        .iter()
                        .filter(|message| message.tool_call_id.as_deref() == Some(call.call_id()))
                        .count(),
                    1
                );
            }
        }
        emitter.finish().await.unwrap();
        let persisted =
            std::fs::read_to_string(workspace.join(".vtcode/sessions/preflight-batch/events.jsonl")).unwrap();
        let events: Vec<serde_json::Value> =
            persisted.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        for (id, expected_args) in [
            ("admitted-first", Some(&valid)),
            ("invalid", Some(&invalid)),
            ("malformed", None),
        ] {
            let completed: Vec<_> = events
                .iter()
                .filter(|record| {
                    record["event"]["type"] == "item.completed" && record["event"]["item"]["tool_call_id"] == id
                })
                .collect();
            assert_eq!(completed.len(), 2, "invocation and output close exactly once: {id}");
            let invocation = completed
                .iter()
                .find(|record| record["event"]["item"]["type"] == "tool_invocation")
                .unwrap();
            assert_eq!(invocation["event"]["item"].get("arguments"), expected_args);
        }
    }
}

#[tokio::test]
async fn preflight_corrective_turn_uses_current_schema_before_identity_check() {
    let mut backing = TestContextBacking::new(32).await;
    backing.select_build_primary_agent();
    let mut ctx = backing.turn_processing_context();
    let args = json!({"action":"update","index":0,"status":"completed"});
    ctx.tool_registry.enable_planning();
    assert!(
        ctx.tool_registry
            .admit_public_tool_call(tool_names::TASK_TRACKER, &args)
            .is_err()
    );
    ctx.session_stats.preflight_recovery.reject(
        tool_names::TASK_TRACKER,
        crate::agent::runloop::unified::state::ValidationCategory::Arguments,
        Some(&args),
        6,
        true,
    );
    ctx.session_stats.preflight_recovery.synthesis();
    ctx.session_stats.preflight_recovery.begin_corrective_turn();
    ctx.tool_registry.disable_planning();
    let result = validate_tool_call(&mut ctx, "live-schema", tool_names::TASK_TRACKER, &args)
        .await
        .unwrap();
    assert!(
        matches!(result, ValidationResult::Proceed(_)),
        "index zero checklist completion is valid outside planning"
    );
    assert!(!ctx.session_stats.preflight_recovery.exhausted());
}

#[tokio::test]
async fn preflight_failed_corrective_execution_does_not_clear_episode() {
    let mut backing = TestContextBacking::new(32).await;
    backing.select_build_primary_agent();
    std::fs::create_dir_all(backing.config.workspace.join(".vtcode/tasks/current_task.md")).unwrap();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.session_stats.preflight_recovery.reject(
        tool_names::TASK_TRACKER,
        crate::agent::runloop::unified::state::ValidationCategory::Arguments,
        Some(&json!({"action":"list","index":1})),
        6,
        false,
    );
    ctx.session_stats.preflight_recovery.synthesis();
    ctx.session_stats.preflight_recovery.begin_corrective_turn();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    handle_single_tool_call(&mut outcome_ctx, "failed-correction", tool_names::TASK_TRACKER, json!({"action":"list"}))
        .await
        .unwrap();
    assert!(!ctx.session_stats.preflight_recovery.is_clear());
    ctx.session_stats.preflight_recovery.finish_corrective_turn();
    assert!(ctx.session_stats.preflight_recovery.exhausted());
}

#[tokio::test]
async fn preflight_tracker_corrections_match_live_operations_and_canonical_aliases() {
    struct CorrectionCase {
        rejected_name: &'static str,
        rejected_args: serde_json::Value,
        correction: serde_json::Value,
        clears: bool,
    }
    for case in [
        CorrectionCase {
            rejected_name: tool_names::TASK_TRACKER,
            rejected_args: json!({"action":"lst"}),
            correction: json!({"action":"list"}),
            clears: true,
        },
        CorrectionCase {
            rejected_name: tool_names::TASK_TRACKER,
            rejected_args: json!({"action":"update","index":1}),
            correction: json!({"action":"update","index":2,"status":"in_progress"}),
            clears: false,
        },
        CorrectionCase {
            rejected_name: "track_tasks",
            rejected_args: json!({"action":"update","index":1}),
            correction: json!({"action":"update","index":1,"status":"completed"}),
            clears: true,
        },
        CorrectionCase {
            rejected_name: tool_names::TASK_TRACKER,
            rejected_args: json!({"action":"update","index_path":"01"}),
            correction: json!({"action":"update","index":1,"status":"completed"}),
            clears: true,
        },
    ] {
        let mut backing = TestContextBacking::new(32).await;
        backing.select_build_primary_agent();
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        handle_single_tool_call(&mut outcome_ctx, "seed", tool_names::TASK_TRACKER,
            json!({"action":"create","title":"Correction regression","items":["First operation","Different operation"]}))
            .await.unwrap();
        assert!(
            outcome_ctx
                .ctx
                .tool_registry
                .admit_public_tool_call(case.rejected_name, &case.rejected_args)
                .is_err()
        );
        validate_tool_call(outcome_ctx.ctx, "rejected", case.rejected_name, &case.rejected_args)
            .await
            .unwrap();
        assert!(!outcome_ctx.ctx.session_stats.preflight_recovery.is_clear());
        outcome_ctx.ctx.session_stats.preflight_recovery.synthesis();
        outcome_ctx.ctx.session_stats.preflight_recovery.begin_corrective_turn();
        handle_single_tool_call(&mut outcome_ctx, "correction", tool_names::TASK_TRACKER, case.correction)
            .await
            .unwrap();
        assert_eq!(outcome_ctx.ctx.session_stats.preflight_recovery.is_clear(), case.clears);
        if !case.clears {
            let persisted =
                std::fs::read_to_string(outcome_ctx.ctx.config.workspace.join(".vtcode/tasks/current_task.md"))
                    .unwrap();
            assert!(persisted.contains("- [ ] First operation"));
            assert!(persisted.contains("- [/] Different operation"), "the unrelated update must actually execute");
            assert_eq!(outcome_ctx.ctx.harness_state.consecutive_preflight_failures, 1);
            handle_single_tool_call(
                &mut outcome_ctx,
                "same-target",
                tool_names::TASK_TRACKER,
                json!({"action":"update","index":1,"status":"completed"}),
            )
            .await
            .unwrap();
            assert!(outcome_ctx.ctx.session_stats.preflight_recovery.is_clear());
        }
        assert_eq!(outcome_ctx.ctx.harness_state.consecutive_preflight_failures, 0);
        outcome_ctx.ctx.session_stats.preflight_recovery.finish_corrective_turn();
        assert!(!outcome_ctx.ctx.session_stats.preflight_recovery.exhausted());
    }
}

#[tokio::test]
async fn preflight_alias_spelling_cannot_bypass_unchanged_call_detection() {
    let mut backing = TestContextBacking::new(32).await;
    backing.select_build_primary_agent();
    let mut ctx = backing.turn_processing_context();
    let arguments = json!({"action":"list","index":1});
    validate_tool_call(&mut ctx, "alias-rejected", "track_tasks", &arguments)
        .await
        .unwrap();
    ctx.session_stats.preflight_recovery.synthesis();
    ctx.session_stats.preflight_recovery.begin_corrective_turn();
    let repeated = validate_tool_call(&mut ctx, "canonical-repeat", tool_names::TASK_TRACKER, &arguments)
        .await
        .unwrap();
    assert!(matches!(repeated, ValidationResult::Outcome(TurnHandlerOutcome::Break(_))));
    assert!(ctx.session_stats.preflight_recovery.exhausted());
}

#[tokio::test]
async fn source_inspection_refresh_survives_read_cap_without_replenishing_quota() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let sample = backing.sample_file.clone();
    std::fs::write(&sample, "before\nunseen\n").unwrap();
    let sample_path = sample.to_string_lossy().to_string();
    let read = json!({"path":sample_path,"offset":1,"limit":1,"max_tokens":4096,"condense":false});
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-before\n+after\n*** End Patch"});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read, PermissionGrant::Permanent).await;
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch, PermissionGrant::Permanent).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry
        .set_inspection_mode(vtcode_core::config::core::agent::InspectionMode::Enforce);
    for _ in 0..6 {
        ctx.harness_state.record_file_read_path_call(sample_path.clone());
    }
    let mut outcomes = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    handle_single_tool_call(&mut outcomes, "blind", tool_names::APPLY_PATCH, patch.clone())
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), "before\nunseen\n");
    assert!(outcomes.ctx.tool_registry.has_patch_recovery_read(tool_names::READ_FILE, &read));
    let mut assistant = uni::Message::assistant(String::new());
    assistant.tool_calls = Some(vec![uni::ToolCall::function(
        "refresh".into(),
        tool_names::READ_FILE.into(),
        read.to_string(),
    )]);
    outcomes.ctx.working_history.push(assistant);
    handle_single_tool_call(&mut outcomes, "refresh", tool_names::READ_FILE, read.clone())
        .await
        .unwrap();
    outcomes
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcomes.ctx.working_history);
    assert!(!outcomes.ctx.tool_registry.has_patch_recovery_read(tool_names::READ_FILE, &read));
    handle_single_tool_call(&mut outcomes, "inspected", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), "after\nunseen\n");
    let unseen = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-unseen\n+bad\n*** End Patch"});
    handle_single_tool_call(&mut outcomes, "still_unseen", tool_names::APPLY_PATCH, unseen)
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&sample).unwrap(), "after\nunseen\n");
    assert!(!outcomes.ctx.tool_registry.has_patch_recovery_read(tool_names::READ_FILE, &read));
}

#[tokio::test]
async fn placeholder_policy_failure_keeps_policy_correction_and_single_prefix() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"cleared":"tool_input"});
    super::super::handle_preflight_failure(
        &mut ctx,
        "policy-placeholder",
        tool_names::EXEC_COMMAND,
        "Tool preflight validation failed: Command security check failed: command injection",
        Some(&args),
        None,
    );
    assert_eq!(ctx.harness_state.consecutive_preflight_failures, 1);
    assert!(ctx.session_stats.preflight_recovery.is_clear(), "policy is not schema recovery");
    let replies: Vec<_> = ctx
        .working_history
        .iter()
        .filter(|m| m.tool_call_id.as_deref() == Some("policy-placeholder"))
        .collect();
    assert_eq!(replies.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&replies[0].content.as_text()).unwrap();
    assert_eq!(payload["retryable"], false);
    assert!(!payload["schema_correction"].as_str().unwrap().contains("Do not echo"));
    assert_eq!(
        payload["error"]
            .as_str()
            .unwrap()
            .matches("Tool preflight validation failed")
            .count(),
        1
    );
}

#[tokio::test]
async fn policy_preflight_denials_are_non_retryable_without_poisoning_permitted_calls() {
    for (error, at_limit) in [
        ("Command security check failed: command injection", false),
        ("tool denied by planning workflow", false),
        ("Command security check failed: command injection", true),
        ("tool denied by planning workflow", true),
    ] {
        let mut backing = TestContextBacking::new(20).await;
        backing.select_build_primary_agent();
        let mut tracker = LoopTracker::new();
        let mut files = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let attempts = if at_limit {
            max_consecutive_blocked_tool_calls_per_turn(&ctx)
        } else {
            1
        };
        for index in 0..attempts {
            super::super::handle_preflight_failure(
                &mut ctx,
                &format!("denied-{index}"),
                tool_names::EXEC_COMMAND,
                error,
                Some(&json!({"cmd":"denied"})),
                None,
            );
            let payload: serde_json::Value =
                serde_json::from_str(&ctx.working_history.last().unwrap().content.as_text()).unwrap();
            assert_eq!(payload["retryable"], false);
            assert!(payload["next_action"].as_str().unwrap().contains("Do not retry"));
            assert!(!payload["next_action"].as_str().unwrap().contains("malformed"));
            assert_eq!(payload["diagnosis"]["next_action"], payload["next_action"]);
            // The circuit arms a tool-free recovery pass, so the guidance must
            // say so instead of inviting a tool call that cannot run.
            let circuit_tripped = at_limit && index + 1 == attempts;
            assert_eq!(
                payload["next_action"]
                    .as_str()
                    .unwrap()
                    .contains("Tools are disabled for the next pass"),
                circuit_tripped,
                "policy guidance must match the armed recovery state: {error}"
            );
            assert!(
                !payload["schema_correction"]
                    .as_str()
                    .unwrap()
                    .contains("matching the declared schema")
            );
            assert!(ctx.session_stats.preflight_recovery.is_clear());
        }
        if at_limit {
            continue;
        }
        let mut outcomes = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut files,
        };
        handle_single_tool_call(
            &mut outcomes,
            "permitted",
            tool_names::EXEC_COMMAND,
            json!({"cmd":"rustc --version", "yield_time_ms":10000}),
        )
        .await
        .unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(&outcomes.ctx.working_history.last().unwrap().content.as_text()).unwrap();
        assert_eq!(payload["exit_code"], 0);
    }
}

#[tokio::test]
async fn preflight_failure_deduplicates_nested_kernel_prefixes() {
    // Regression (session-vtcode-20261019T075619Z_862610-57915): the kernel
    // error arrives as "Tool preflight validation failed for 'exec_command':
    // …" and hop rejections re-wrap it, so the payload must strip every
    // existing prefix layer before adding its own exactly once.
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"cmd":"`./script.sh --dry-run`"});
    for (index, error) in [
        "Tool preflight validation failed for 'exec_command': Command security check failed: Command injection pattern detected",
        "Tool preflight validation failed for 'exec_command': Tool preflight validation failed for 'exec_command': Command security check failed: Command injection pattern detected",
    ]
    .into_iter()
    .enumerate()
    {
        let call_id = format!("dedup-preflight-{index}");
        super::super::handle_preflight_failure(&mut ctx, &call_id, tool_names::EXEC_COMMAND, error, Some(&args), None);
        let message_text = ctx
            .working_history
            .iter()
            .rev()
            .find(|m| m.tool_call_id.as_deref() == Some(call_id.as_str()))
            .map(|m| m.content.as_text().to_string())
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&message_text).unwrap();
        let error_text = payload["error"].as_str().unwrap();
        assert_eq!(
            error_text.matches("Tool preflight validation failed").count(),
            1,
            "{error_text}"
        );
        assert!(
            error_text.contains("for 'exec_command': Command security check failed"),
            "{error_text}"
        );
    }
}

#[tokio::test]
async fn preflight_placeholder_and_invalid_arguments_allow_schema_correct_execution() {
    for (invalid, command_key) in [
        (r#"{"cleared":"tool_input"}"#, "cmd"),
        (r#"{}"#, "command"),
        (r#"{"cmd":" "}"#, "cmd"),
        (r#"{"cmd":42}"#, "command"),
        ("{", "cmd"),
    ] {
        let mut backing = TestContextBacking::new(20).await;
        backing.select_build_primary_agent();
        let mut tracker = LoopTracker::new();
        let mut files = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcomes = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut files,
        };
        let mut valid = json!({"yield_time_ms":10000});
        valid[command_key] = json!("rustc --version");
        let calls = [
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                "invalid".into(),
                tool_names::EXEC_COMMAND.into(),
                invalid.into(),
            )),
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                "corrected".into(),
                tool_names::EXEC_COMMAND.into(),
                valid.to_string(),
            )),
        ];
        assert!(handle_tool_calls(&mut outcomes, &calls).await.unwrap().is_none(), "{invalid}");
        assert!(outcomes.ctx.session_stats.preflight_recovery.is_clear(), "successful correction: {invalid}");
        for id in ["invalid", "corrected"] {
            let replies: Vec<_> = outcomes
                .ctx
                .working_history
                .iter()
                .filter(|m| m.tool_call_id.as_deref() == Some(id))
                .collect();
            assert_eq!(replies.len(), 1, "one terminal response: {invalid} / {id}");
            if id == "corrected" {
                assert!(replies[0].content.as_text().contains("rustc"));
                let payload: serde_json::Value = serde_json::from_str(&replies[0].content.as_text()).unwrap();
                assert_eq!(payload["exit_code"], 0, "the verifier must finish successfully");
            }
        }
    }
}
