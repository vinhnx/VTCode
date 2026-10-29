use super::*;

#[cfg(unix)]
#[tokio::test]
async fn invalid_and_no_match_required_grep_bound_repeated_completion_summaries() {
    use vtcode_core::config::constants::tools;
    for (command, exit) in [
        ("grep -n '---\\s\\+\\|' fixture.txt", 2),
        ("grep -n missing fixture.txt", 1),
    ] {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut cfg = vtcode_core::config::loader::VTCodeConfig::default();
        cfg.agent.harness.verification.in_turn_attempts = 0;
        backing.set_vt_cfg_for_test(cfg);
        {
            let ctx = backing.turn_processing_context();
            std::fs::write(ctx.tool_registry.workspace_root().join("fixture.txt"), "present\n").unwrap();
            ctx.tool_registry.allow_all_tools().await.unwrap();
            ctx.tool_registry
                .execute_tool(
                    tools::TASK_TRACKER,
                    serde_json::json!({
                        "action":"create", "items":[{"description":"Check", "verify":[command]}]
                    }),
                )
                .await
                .unwrap();
            ctx.tool_registry.restore_completion_verification_debt();
        }
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let mut blocked = false;
        for step in 1..=4 {
            let outcome = {
                let mut ctx = backing.turn_processing_context();
                handle_turn_processing_result(HandleTurnProcessingResultParams {
                    ctx: &mut ctx,
                    processing_result: TurnProcessingResult::TextResponse {
                        text: "The requested work is complete.".into(),
                        reasoning: vec![],
                        reasoning_details: None,
                        proposed_plan: None,
                    },
                    response_streamed: false,
                    step_count: step,
                    repeated_tool_attempts: &mut tracker,
                    turn_modified_files: &mut modified,
                    max_tool_loops: 20,
                    tool_repeat_limit: 4,
                })
                .await
                .unwrap()
            };
            match outcome {
                TurnHandlerOutcome::Continue => {}
                TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }) => {
                    blocked = true;
                    break;
                }
                _ => panic!("an unsatisfied mandatory check cannot complete"),
            }
        }
        assert!(blocked, "{command} must terminate within the configured recovery limit");
        let ctx = backing.turn_processing_context();
        assert!(ctx.tool_registry.completion_verification_pending());
        let failure = ctx.session_stats.last_verification_failure().unwrap();
        assert_eq!(failure.command, command);
        assert_eq!(failure.exit_code, Some(exit));
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 1);
        assert!(
            !ctx.working_history
                .iter()
                .any(|message| message.role == uni::MessageRole::System
                    && message.content.as_text().contains("gate is cleared"))
        );
    }
}
#[tokio::test]
async fn passing_local_check_accepts_external_limitation_without_another_recovery() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut modified = BTreeSet::new();
    let outcome = {
        let mut ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        let result = ctx
            .tool_registry
            .execute_tool("exec_command", serde_json::json!({"cmd":"rustc --version"}))
            .await
            .unwrap();
        assert_eq!(result["exit_code"], 0);
        assert!(ctx.tool_registry.has_current_completion_verification());
        handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut ctx,
            processing_result: TurnProcessingResult::TextResponse {
                text: "Updated README. Local check passed; remote image rendering remains unverified after HTTP 429."
                    .into(),
                reasoning: vec![],
                reasoning_details: None,
                proposed_plan: None,
            },
            response_streamed: false,
            step_count: 1,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
            max_tool_loops: 8,
            tool_repeat_limit: 3,
        })
        .await
        .unwrap()
    };
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(!tracker.verification_is_pending());
    let ctx = backing.turn_processing_context();
    assert!(!ctx.working_history.iter().any(|message| {
        message.tool_calls.as_ref().is_some_and(|calls| {
            calls
                .iter()
                .any(|call| call.id == super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID)
        })
    }));
}
#[tokio::test]
async fn failed_optional_check_without_mutations_cannot_create_completion_obligations() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "exec_command",
        r#"{"cmd":"rustc --invalid-flag-xyz"}"#,
    )
    .await;
    assert!(!tracker.verification_is_pending(), "optional diagnostics cannot create registry debt");
    let mut ctx = backing.turn_processing_context();
    assert!(!ctx.tool_registry.completion_verification_pending(), "no code edits or mandatory checks");
    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::TextResponse {
            text: "Inspection complete. The optional checker failed; no files were changed.".into(),
            reasoning: vec![],
            reasoning_details: None,
            proposed_plan: None,
        },
        response_streamed: false,
        step_count: 2,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
        max_tool_loops: 8,
        tool_repeat_limit: 4,
    })
    .await
    .unwrap();
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
}
#[tokio::test]
async fn retained_draft_is_discarded_for_new_steering_without_launching_checks() {
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --version");
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.restore_completion_verification_debt();
        ctx.runtime_steering
            .try_queue_follow_up_input("Change the scope first.".into())
            .unwrap();
    }
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Continue));
    let ctx = backing.turn_processing_context();
    assert!(ctx.tool_registry.completion_verification_pending());
    assert!(!ctx.working_history.iter().any(|m| m.role == uni::MessageRole::Assistant));
    assert!(!ctx.working_history.iter().any(|m| m.tool_calls.is_some()));
    assert!(!backing.rendered_inline_output().contains("Still working."));
}
#[tokio::test]
async fn retained_draft_freshness_rejects_mutation_or_applied_user_input() {
    for mutation in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        let ctx = backing.turn_processing_context();
        let draft = super::super::CompletionDraft {
            text: "Original answer".into(),
            reasoning: vec![],
            reasoning_details: None,
            response_streamed: false,
            mutation_generation: ctx.tool_registry.completion_mutation_generation(),
            user_messages: super::super::user_message_count(ctx.working_history),
        };
        assert!(draft.is_current(&ctx));
        ctx.working_history.push(uni::Message::system("Verifier result".into()));
        assert!(draft.is_current(&ctx), "tool evidence does not invalidate the answer");
        if mutation {
            ctx.tool_registry.restore_completion_verification_debt();
        } else {
            ctx.working_history.push(uni::Message::user("New scope".into()));
        }
        assert!(!draft.is_current(&ctx));
    }
}
#[tokio::test]
async fn retained_draft_cancellation_publishes_no_answer_or_verifier() {
    for exit in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        {
            let ctx = backing.turn_processing_context();
            ctx.tool_registry.restore_completion_verification_debt();
            if exit {
                ctx.ctrl_c_state.request_exit();
            } else {
                ctx.ctrl_c_state.request_local_cancel();
            }
        }
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Exit)) == exit);
        if !exit {
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        let ctx = backing.turn_processing_context();
        assert!(!ctx.working_history.iter().any(|m| m.role == uni::MessageRole::Assistant));
        assert!(ctx.tool_registry.completion_verification_pending());
        assert!(!backing.rendered_inline_output().contains("Still working."));
    }
}
#[tokio::test]
async fn retained_draft_pause_remains_interruptible_by_cancel_and_exit() {
    use crate::agent::runloop::unified::stop_requests::{request_local_cancel, request_local_exit};
    use vtcode_core::core::agent::steering::SteeringMessage;

    for exit in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_verification_override_for_test("rustc --version");
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        backing.set_steering_receiver(receiver);
        sender.send(SteeringMessage::Pause).unwrap();
        let (state, notify) = {
            let ctx = backing.turn_processing_context();
            ctx.tool_registry.restore_completion_verification_debt();
            (ctx.ctrl_c_state.clone(), ctx.ctrl_c_notify.clone())
        };
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        let outcome = {
            let waiting = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1);
            tokio::pin!(waiting);
            assert!(futures::poll!(&mut waiting).is_pending(), "Pause must defer execution");
            if exit {
                request_local_exit(&state, &notify);
            } else {
                request_local_cancel(&state, &notify);
            }
            tokio::time::timeout(std::time::Duration::from_secs(1), &mut waiting).await
        }
        .expect("a local stop must interrupt Pause without waiting for Resume");
        assert_eq!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Exit)), exit);
        if !exit {
            assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        let ctx = backing.turn_processing_context();
        assert!(!ctx.working_history.iter().any(|m| m.role == uni::MessageRole::Assistant));
        assert!(ctx.tool_registry.completion_verification_pending());
        assert!(!backing.rendered_inline_output().contains("Still working."));
    }
}
#[cfg(unix)]
#[tokio::test]
async fn yielded_verifier_announces_once_while_preserving_real_waits_and_one_final_answer() {
    use std::os::unix::fs::PermissionsExt;

    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("./rustc --version");
    let registry = {
        let ctx = backing.turn_processing_context();
        let script = ctx.tool_registry.workspace_root().join("rustc");
        std::fs::write(&script, "#!/bin/sh\nsleep 3\nprintf 'terminal evidence\\n'\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
        let mut commands = ctx.tool_registry.commands_config();
        commands.allow_list.push("./rustc".into());
        ctx.tool_registry.apply_commands_config(&commands);
        ctx.tool_registry.clone()
    };
    let launched = registry
        .execute_harness_command_session(serde_json::json!({
            "action":"run", "command":"./rustc --version", "background":true, "yield_time_ms":50
        }))
        .await
        .unwrap();
    let session_id = vtcode_core::tools::command_args::session_id_text(&launched).unwrap().to_owned();
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await;
    // Always clean up owned test resources before assertions about the outcome.
    if !registry.in_progress_exec_sessions(8).await.is_empty() {
        registry.terminate_harness_exec_session(&session_id).await.unwrap();
        registry.close_harness_exec_session(&session_id).await.unwrap();
    }
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(registry.completion_verifier_passed("./rustc --version"));
    let ctx = backing.turn_processing_context();
    let announcements = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().starts_with("Harness auto-verification:")
        })
        .count();
    assert_eq!(announcements, 1);
    let waits = ctx
        .working_history
        .iter()
        .flat_map(|message| message.tool_calls.iter().flatten())
        .filter(|call| {
            call.id == super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID
                && call.function.as_ref().is_some_and(|function| function.name == "write_stdin")
        })
        .count();
    assert!(waits >= 2, "exercise multiple real waits, observed {waits}");
    assert_eq!(
        ctx.working_history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
}
#[cfg(unix)]
#[tokio::test]
async fn retained_draft_yielded_verifier_obeys_execution_limit_and_cleans_up() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("./rustc --version");
    let registry = {
        let ctx = backing.turn_processing_context();
        let script = ctx.tool_registry.workspace_root().join("rustc");
        std::fs::write(&script, "#!/bin/sh\nprintf 'launched\\n' >> launches\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
        let mut commands = ctx.tool_registry.commands_config();
        commands.allow_list.push("./rustc".into());
        ctx.tool_registry.apply_commands_config(&commands);
        ctx.tool_registry.apply_timeout_policy(&vtcode_core::config::TimeoutsConfig {
            long_running_command_ceiling_seconds: 1,
            ..Default::default()
        });
        ctx.tool_registry.clone()
    };
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let launched = registry
        .execute_harness_command_session(serde_json::json!({
            "action":"run", "command":"./rustc --version", "background":true, "yield_time_ms":50
        }))
        .await
        .unwrap();
    assert!(vtcode_core::tools::command_args::session_id_text(&launched).is_some(), "{launched}");
    let session_id = registry
        .pending_completion_verifier_session("./rustc --version")
        .expect("owned yielded check");
    let outcome =
        tokio::time::timeout(Duration::from_secs(4), drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1))
            .await;
    let remaining = registry.in_progress_exec_sessions(8).await;
    // Always clean up the test's owned process even when testing the broken implementation.
    if !remaining.is_empty() {
        registry.terminate_harness_exec_session(&session_id).await.unwrap();
        registry.close_harness_exec_session(&session_id).await.unwrap();
    }
    let outcome = outcome.expect("a verifier must not refresh its execution allowance on every wait");
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
        if reason.contains("execution limit")));
    assert!(remaining.is_empty(), "exhausted owned verification sessions must be released");
    assert!(registry.completion_verification_pending());
    assert!(!registry.completion_verifier_passed("./rustc --version"));
    assert_eq!(std::fs::read_to_string(registry.workspace_root().join("launches")).unwrap(), "launched\n");
    let ctx = backing.turn_processing_context();
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0, "an execution stop is not a failed check");
    assert!(
        !ctx.working_history
            .iter()
            .any(|m| m.phase == Some(uni::AssistantPhase::FinalAnswer))
    );
    assert!(!backing.rendered_inline_output().contains("Still working."));
}
#[cfg(unix)]
#[tokio::test]
async fn retained_draft_denied_owned_wait_blocks_without_repeating_admission() {
    use std::os::unix::fs::PermissionsExt;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("./rustc --version");
    let registry = {
        let ctx = backing.turn_processing_context();
        let script = ctx.tool_registry.workspace_root().join("rustc");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
        let mut commands = ctx.tool_registry.commands_config();
        commands.allow_list.push("./rustc".into());
        ctx.tool_registry.apply_commands_config(&commands);
        ctx.harness_state.max_tool_wall_clock = std::time::Duration::from_secs(3);
        ctx.tool_registry.clone()
    };
    let launched = registry
        .execute_harness_command_session(serde_json::json!({
            "action":"run", "command":"./rustc --version", "background":true, "yield_time_ms":50
        }))
        .await
        .unwrap();
    let session_id = vtcode_core::tools::command_args::session_id_text(&launched).unwrap().to_owned();
    registry
        .set_tool_policy("write_stdin", vtcode_core::tool_policy::ToolPolicy::Deny)
        .await
        .unwrap();
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await;
    let still_owned = registry.pending_completion_verifier_session("./rustc --version");
    registry.terminate_harness_exec_session(&session_id).await.unwrap();
    registry.close_harness_exec_session(&session_id).await.unwrap();
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
    assert_eq!(
        still_owned.as_deref(),
        Some(session_id.as_str()),
        "a denied wait does not transfer session ownership"
    );
    let ctx = backing.turn_processing_context();
    let waits = ctx
        .working_history
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .filter(|call| call.function.as_ref().is_some_and(|function| function.name == "write_stdin"))
        .count();
    assert_eq!(waits, 1, "a denied wait must end the attempt, not retry unchanged admission");
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
    assert!(
        !ctx.working_history
            .iter()
            .any(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
    );
}
#[test]
fn running_verifier_proof_requires_fresh_matching_session_and_no_failure() {
    use super::super::verifier_response_is_running_session;
    for response in [
        r#"{"session_id":"owned","exit_code":null}"#,
        r#"{"next_wait_args":{"session_id":"owned"}}"#,
        r#"{"next_continue_args":{"s":"owned"}}"#,
    ] {
        assert!(verifier_response_is_running_session(response, "owned"), "{response}");
    }
    for response in [
        r#"{"session_id":"other"}"#,
        r#"{"session_id":"owned","exit_code":0}"#,
        r#"{"session_id":"owned","error":"denied"}"#,
        r#"{"session_id":"owned","not_executed":true}"#,
        r#"{"session_id":"owned","reused_recent_result":true}"#,
        r#"{"session_id":"owned","success":false}"#,
        r#"{"session_id":"owned","termination_requested":true}"#,
        "not JSON",
        "{}",
    ] {
        assert!(!verifier_response_is_running_session(response, "owned"), "{response}");
    }
}
