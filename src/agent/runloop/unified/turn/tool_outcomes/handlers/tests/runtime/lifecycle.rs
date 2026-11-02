use super::*;

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
