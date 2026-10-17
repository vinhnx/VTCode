use crate::agent::runloop::unified::inline_events::harness::{HarnessEventEmitter, tool_outcome_observation_event};
use serde_json::Value;
use vtcode_core::core::agent::events::{
    ToolOutputPayload, error_item_completed_event, tool_invocation_completed_event, tool_output_completed_event,
    tool_output_payload_from_value, tool_started_event,
};
use vtcode_core::exec::events::{ToolCallStatus, tool_outcome_from_status};
use vtcode_core::tools::registry::ToolExecutionError;

use super::status::{ToolExecutionStatus, ToolPipelineOutcome};

pub(crate) fn emit_tool_start_if_needed(
    harness_emitter: Option<&HarnessEventEmitter>,
    already_started: bool,
    tool_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
) -> bool {
    if already_started {
        return true;
    }
    if let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_started_event(tool_item_id.to_string(), tool_name, Some(args), Some(tool_call_id)));
        return true;
    }
    false
}

pub(crate) fn emit_tool_outcome_observation(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_name: &str,
    outcome: &ToolPipelineOutcome,
) {
    let Some(emitter) = harness_emitter else {
        return;
    };
    let duration_ms = outcome.total_duration.as_millis().min(u128::from(u64::MAX)) as u64;
    let category = outcome
        .last_error_category
        .as_ref()
        .map(|category| category.as_str().to_string());
    let _ = emitter.emit(tool_outcome_observation_event(tool_name, outcome.attempts, duration_ms, category));
}

fn tool_error_output_payload(error: &ToolExecutionError) -> ToolOutputPayload {
    let mut payload = tool_output_payload_from_value(&error.to_json_value());
    let user_message = error.user_message();
    if payload.aggregated_output.is_empty() {
        payload.aggregated_output = user_message;
    } else {
        payload.aggregated_output = format!("{user_message}\n{}", payload.aggregated_output);
    }
    payload
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // event emitter, all identity/status params needed
pub(super) fn emit_tool_completion_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_execution_started: bool,
    tool_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    aggregated_output: impl Into<String>,
) {
    if !tool_started_emitted {
        return;
    }

    if let Some(emitter) = harness_emitter {
        let aggregated_output = aggregated_output.into();
        let outcome = tool_outcome_from_status(&status);
        let _ = emitter.emit(tool_invocation_completed_event(
            tool_item_id.to_string(),
            tool_name,
            Some(args),
            Some(tool_call_id),
            status.clone(),
            outcome,
        ));
        if tool_execution_started {
            let _ = emitter.emit(tool_output_completed_event(
                tool_item_id.to_string(),
                Some(tool_call_id),
                status,
                exit_code,
                spool_path,
                aggregated_output,
            ));
        } else if !aggregated_output.is_empty() {
            let _ = emitter.emit(error_item_completed_event(format!("{tool_item_id}:error"), aggregated_output));
        }
    }
}

pub(crate) fn emit_tool_completion_for_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_execution_started: bool,
    tool_item_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    args: &Value,
    tool_status: &ToolExecutionStatus,
) {
    let no_matches = matches!(tool_status, ToolExecutionStatus::Success { output, .. }
        if crate::agent::runloop::unified::turn::tool_outcomes::is_grep_style_no_match(tool_name, args, output));
    let (status, exit_code, mut output_payload) = match tool_status {
        ToolExecutionStatus::Success { output, command_success, .. } => (
            if *command_success || no_matches {
                ToolCallStatus::Completed
            } else {
                ToolCallStatus::Failed
            },
            output
                .get("exit_code")
                .and_then(Value::as_i64)
                .and_then(|code| i32::try_from(code).ok()),
            tool_output_payload_from_value(output),
        ),
        ToolExecutionStatus::Failure { error } => (ToolCallStatus::Failed, None, tool_error_output_payload(error)),
        ToolExecutionStatus::Timeout { error } => (ToolCallStatus::Failed, None, tool_error_output_payload(error)),
        ToolExecutionStatus::Cancelled => (
            ToolCallStatus::Failed,
            None,
            ToolOutputPayload {
                aggregated_output: "Tool execution cancelled".to_string(),
                spool_path: None,
            },
        ),
    };
    if no_matches {
        output_payload.aggregated_output =
            format!("No matching results (exit code 1).\n{}", output_payload.aggregated_output);
    }
    emit_tool_completion_status(
        harness_emitter,
        tool_started_emitted,
        tool_execution_started,
        tool_item_id,
        tool_call_id,
        tool_name,
        args,
        status,
        exit_code,
        output_payload.spool_path.as_deref(),
        output_payload.aggregated_output,
    );
    if tool_execution_started
        && tool_started_emitted
        && let Some(emitter) = harness_emitter
        && let ToolExecutionStatus::Success { output, .. } = tool_status
    {
        let _ = emitter.emit_exec_session_output(tool_item_id, tool_name, args, output);
    }
    if tool_execution_started
        && tool_started_emitted
        && let Some(emitter) = harness_emitter
        && let ToolExecutionStatus::Success { output, command_success: true, .. } = tool_status
        && let Some(event) =
            vtcode_core::core::agent::events::file_change_completed_event(tool_item_id, tool_name, args, output)
    {
        let _ = emitter.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn canonical_search_completion_preserves_exit_codes_and_rejects_ambiguous_no_matches() {
        use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails, VersionedThreadEvent};
        for (args, output, expected) in [
            (json!({"cmd":"grep absent sample.txt"}), json!({"exit_code":1}), ToolCallStatus::Completed),
            (json!({"cmd":"rg absent sample.txt"}), json!({"exit_code":1,"stdout":""}), ToolCallStatus::Completed),
            (
                json!({"cmd":"grep --invalid sample.txt"}),
                json!({"exit_code":2,"stderr":"invalid option"}),
                ToolCallStatus::Failed,
            ),
            (
                json!({"cmd":"grep absent sample.txt"}),
                json!({"exit_code":1,"stderr":"read failed"}),
                ToolCallStatus::Failed,
            ),
            (
                json!({"cmd":"grep absent sample.txt"}),
                json!({"exit_code":1,"output_truncated":true}),
                ToolCallStatus::Failed,
            ),
            (json!({"cmd":"grep absent sample.txt; false"}), json!({"exit_code":1}), ToolCallStatus::Failed),
            (json!({"cmd":"cargo check --locked"}), json!({"exit_code":1}), ToolCallStatus::Failed),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("events.jsonl");
            let emitter = HarnessEventEmitter::new(path.clone()).unwrap();
            let status = ToolExecutionStatus::Success {
                output: output.clone(),
                stdout: None,
                modified_files: vec![],
                command_success: false,
            };
            emit_tool_completion_for_status(Some(&emitter), true, true, "item", "call", "exec_command", &args, &status);
            assert!(status.is_failure_like(), "presentation must not rewrite execution or safety accounting");
            let records = std::fs::read_to_string(path).unwrap();
            let mut invocations = 0;
            let mut outputs = 0;
            for line in records.lines() {
                let event = serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event();
                if let ThreadEvent::ItemCompleted(item) = event {
                    match item.item.details {
                        ThreadItemDetails::ToolInvocation(invocation) => {
                            assert_eq!(invocation.status, expected);
                            assert_eq!(invocation.arguments, Some(args.clone()));
                            invocations += 1;
                        }
                        ThreadItemDetails::ToolOutput(payload) => {
                            assert_eq!(payload.status, expected);
                            assert_eq!(payload.exit_code.map(i64::from), output["exit_code"].as_i64());
                            assert_eq!(
                                payload.output.contains("No matching results"),
                                expected == ToolCallStatus::Completed
                            );
                            if let Some(stderr) = output.get("stderr").and_then(Value::as_str) {
                                assert!(payload.output.contains(stderr));
                            }
                            outputs += 1;
                        }
                        _ => {}
                    }
                }
            }
            assert_eq!((invocations, outputs), (1, 1));
        }
    }

    #[tokio::test]
    async fn verifier_poll_completion_updates_the_launch_through_canonical_output() {
        use vtcode_core::exec::events::{InputOrigin, ThreadEvent, TurnStartedEvent};
        use vtcode_memory::explanation::ExplanationScope;
        let workspace = tempfile::tempdir().unwrap();
        let emitter = HarnessEventEmitter::new_async(workspace.path(), "interactive-verifier", None)
            .await
            .unwrap();
        emitter.begin_task_turn("turn", "Verify changes", InputOrigin::User).unwrap();
        emitter.emit(ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        let launch_args = json!({"cmd":"cargo check --locked"});
        assert!(emit_tool_start_if_needed(
            Some(&emitter),
            false,
            "launch",
            "launch-call",
            "exec_command",
            &launch_args
        ));
        emit_tool_completion_for_status(
            Some(&emitter),
            true,
            true,
            "launch",
            "launch-call",
            "exec_command",
            &launch_args,
            &ToolExecutionStatus::Success {
                output: json!({"session_id":"running-verifier","output":"checking"}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
        );
        let pending = emitter.explanation(ExplanationScope::Task).await.unwrap();
        assert_eq!(pending.verification.len(), 1);
        assert_eq!(pending.verification[0].fact.status, "pending");
        let poll_args = json!({"session_id":"running-verifier","chars":""});
        assert!(emit_tool_start_if_needed(Some(&emitter), false, "poll", "poll-call", "write_stdin", &poll_args));
        emit_tool_completion_for_status(
            Some(&emitter),
            true,
            true,
            "poll",
            "poll-call",
            "write_stdin",
            &poll_args,
            &ToolExecutionStatus::Success {
                output: json!({"session_id":"running-verifier","exit_code":0,"output":"finished"}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
        );
        let verified = emitter.explanation(ExplanationScope::Task).await.unwrap();
        assert_eq!(verified.verification.len(), 1);
        assert_eq!(verified.verification[0].fact.status, "passed");
        assert!(verified.verification[0].fresh);
        assert_eq!(verified.verification[0].exit_code, Some(0));
        let source = emitter
            .evidence(verified.verification[0].fact.evidence.clone(), 0)
            .await
            .unwrap();
        assert!(source.text.contains("launch:exec-session-output"));
        assert!(source.text.contains("finished"));
        emitter.finish().await.unwrap();
    }

    #[tokio::test]
    async fn background_verifier_completion_keeps_raw_and_derived_facts_in_the_original_task() {
        use vtcode_core::exec::events::{
            InputOrigin, ItemCompletedEvent, ThreadEvent, ThreadItem, ThreadItemDetails, TurnStartedEvent,
        };
        use vtcode_memory::explanation::ExplanationScope;
        let workspace = tempfile::tempdir().unwrap();
        let emitter = HarnessEventEmitter::new_async(workspace.path(), "background-verifier", None)
            .await
            .unwrap();
        let original = emitter
            .begin_task_turn("old-turn", "Verify first task", InputOrigin::User)
            .unwrap();
        emitter.emit(ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        let args = json!({"cmd":"cargo check --locked"});
        assert!(emit_tool_start_if_needed(Some(&emitter), false, "launch", "call", "exec_command", &args));
        emit_tool_completion_for_status(
            Some(&emitter),
            true,
            true,
            "launch",
            "call",
            "exec_command",
            &args,
            &ToolExecutionStatus::Success {
                output: json!({"session_id":"running-verifier"}),
                stdout: None,
                modified_files: vec![],
                command_success: true,
            },
        );
        let current = emitter
            .begin_task_turn("new-turn", "Inspect second task", InputOrigin::User)
            .unwrap();
        emitter.emit(ThreadEvent::TurnStarted(TurnStartedEvent::default())).unwrap();
        emitter
            .emit(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: "raw-exit".into(),
                    context: None,
                    details: ThreadItemDetails::Harness(Box::new(
                        serde_json::from_value(json!({
                            "event":"background_subprocess_completed", "task_id":"exec:running-verifier",
                            "session_id":"running-verifier", "exec_session_id":"running-verifier", "exit_code":0,
                            "message":"Earlier verifier exited",
                        }))
                        .unwrap(),
                    )),
                },
            }))
            .unwrap();
        let task = emitter.explanation(ExplanationScope::Task).await.unwrap();
        assert_eq!(task.task_id.as_deref(), Some(current.as_str()));
        assert!(task.actions.is_empty());
        assert!(task.verification.is_empty());
        let session = emitter.explanation(ExplanationScope::Session).await.unwrap();
        assert_eq!(session.verification.len(), 1);
        assert_eq!(session.verification[0].fact.status, "passed");
        assert!(
            session
                .actions
                .iter()
                .all(|action| action.task_id.as_deref() == Some(original.as_str()))
        );
        emitter.finish().await.unwrap();
    }

    #[test]
    fn aggregates_command_output_without_duplicates() {
        let output = json!({
            "output": "same",
            "stdout": "same",
            "stderr": "warn"
        });

        let payload = tool_output_payload_from_value(&output);
        assert_eq!(payload.aggregated_output, "same\n[stderr]\nwarn");
        assert_eq!(payload.spool_path, None);
    }

    #[test]
    fn includes_content_when_command_stream_fields_absent() {
        let output = json!({
            "content": "file body",
            "path": "README.md"
        });

        let payload = tool_output_payload_from_value(&output);
        assert!(payload.aggregated_output.starts_with("file body\nStructured output:"));
        assert!(payload.aggregated_output.contains("\"path\": \"README.md\""));
        assert_eq!(payload.spool_path, None);
    }

    #[test]
    fn prefers_spool_reference_over_inline_output() {
        let output = json!({
            "output": "preview",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt"
        });

        let payload = tool_output_payload_from_value(&output);
        assert_eq!(payload.aggregated_output, "");
        assert_eq!(payload.spool_path.as_deref(), Some(".vtcode/context/tool_outputs/run-1.txt"));
    }

    #[test]
    fn structured_list_output_emits_compact_summary() {
        let output = json!({
            "items": [
                {"path": "vtcode-tui/src/app.rs", "type": "file"},
                {"path": "vtcode-tui/src/core_tui", "type": "directory"}
            ],
            "count": 2,
            "total": 11
        });

        let payload = tool_output_payload_from_value(&output);
        assert!(payload.aggregated_output.contains("Listed 11 items"));
        assert!(payload.aggregated_output.contains("vtcode-tui/src/app.rs"));
    }

    #[test]
    fn non_zero_command_marks_failed_completion() {
        let status = ToolExecutionStatus::Success {
            output: json!({
                "stdout": "boom",
                "exit_code": 1
            }),
            stdout: Some("boom".to_string()),
            modified_files: vec![],
            command_success: false,
        };

        let (event_status, exit_code, output_payload) = match &status {
            ToolExecutionStatus::Success { output, command_success, .. } => (
                if *command_success {
                    ToolCallStatus::Completed
                } else {
                    ToolCallStatus::Failed
                },
                output
                    .get("exit_code")
                    .and_then(Value::as_i64)
                    .and_then(|code| i32::try_from(code).ok()),
                tool_output_payload_from_value(output),
            ),
            _ => panic!("success status expected"),
        };

        assert_eq!(event_status, ToolCallStatus::Failed);
        assert_eq!(exit_code, Some(1));
        assert_eq!(output_payload.aggregated_output, "boom");
        assert_eq!(output_payload.spool_path, None);
    }

    #[test]
    fn failure_payload_preserves_structured_error_context() {
        let error = ToolExecutionError::new(
            "write_file",
            vtcode_core::tools::registry::ToolErrorType::ExecutionError,
            "write failed",
        )
        .with_partial_state(true, false)
        .with_surface("unified_runloop")
        .with_attempt(2);
        let payload = tool_error_output_payload(&error);

        assert!(payload.aggregated_output.contains(&error.user_message()));
        assert!(payload.aggregated_output.contains("\"partial_state_possible\": true"));
        assert!(payload.aggregated_output.contains("\"surface\": \"unified_runloop\""));
        assert!(payload.aggregated_output.contains("\"attempt\": 2"));
        assert!(payload.aggregated_output.contains("\"original_error\": null"));
        assert_eq!(payload.spool_path, None);
    }
}
