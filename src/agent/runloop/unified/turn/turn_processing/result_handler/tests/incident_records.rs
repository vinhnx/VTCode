use super::*;

/// Sanitized replay of session 20261019T075619: Markdown verify metadata
/// must cross the approval boundary without becoming shell substitution.
#[cfg(unix)]
#[tokio::test]
async fn incident_plan_handoff_executes_bare_verifier_and_publishes_once() {
    use crate::agent::runloop::unified::planning_workflow::{
        PlanExecutionContext, PlanExecutionTarget, allocate_plan_file_if_missing, complete_approved_plan_handoff,
        load_plan_text_for_approval, persist_plan_draft,
    };
    use std::os::unix::fs::PermissionsExt;
    use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails, VersionedThreadEvent};
    use vtcode_core::tool_policy::ToolPolicy;

    let mut backing = TestTurnProcessingBacking::new(20).await;
    let events_path = backing.enable_harness_emitter();
    backing.activate_planning_for_test();
    let plan = "## Summary\nUpdate the README locally.\n## Implementation Steps\n1. Update documentation -> files: [README.md] -> verify: [`./verify.sh`]\n## Test Cases and Validation\n- The local verifier exits 0 when README contains the new text.\n## Assumptions and Defaults\n- No network access is needed.";
    let registry = {
        let ctx = backing.turn_processing_context();
        let registry = ctx.tool_registry.clone();
        let root = registry.workspace_root();
        std::fs::write(root.join("README.md"), "old\n").unwrap();
        std::fs::write(
            root.join("verify.sh"),
            "#!/bin/sh\nprintf 'run\\n' >> verifier-runs\ngrep -qx updated README.md\n",
        )
        .unwrap();
        std::fs::set_permissions(root.join("verify.sh"), std::fs::Permissions::from_mode(0o700)).unwrap();
        registry.allow_all_tools().await.unwrap();
        let mut commands = registry.commands_config();
        commands.allow_list.push("./verify.sh".into());
        registry.apply_commands_config(&commands);
        let state = registry.planning_workflow_state();
        allocate_plan_file_if_missing(&state).await.unwrap();
        persist_plan_draft(&state, plan).await.unwrap();
        let artifact = load_plan_text_for_approval(&registry).await.unwrap();
        assert!(artifact.text.contains("[`./verify.sh`]"));
        registry.set_tool_policy("task_tracker", ToolPolicy::Deny).await.unwrap();
        let denied = complete_approved_plan_handoff(
            &registry,
            ctx.plan_session,
            ctx.handle,
            artifact.clone(),
            PlanExecutionTarget::build(PlanExecutionContext::Current, false),
        )
        .await
        .unwrap_err();
        assert!(format!("{denied:#}").contains("denied by policy"));
        assert!(registry.is_planning_active());
        assert!(state.is_active());
        assert!(!registry.tracker_adopted_for_request());
        assert!(registry.next_required_completion_verifier().is_none());
        assert_eq!(load_plan_text_for_approval(&registry).await.unwrap().text, artifact.text);
        registry.set_tool_policy("task_tracker", ToolPolicy::Allow).await.unwrap();
        // Explicit approval is a separate runtime boundary, never inferred
        // from synthesis success or restored planning state.
        complete_approved_plan_handoff(
            &registry,
            ctx.plan_session,
            ctx.handle,
            artifact,
            PlanExecutionTarget::build(PlanExecutionContext::Current, false),
        )
        .await
        .unwrap();
        assert!(registry.tracker_adopted_for_request());
        assert_eq!(registry.next_required_completion_verifier().as_deref(), Some("./verify.sh"));
        assert!(!registry.is_planning_active());
        registry
    };
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    registry.restore_completion_verification_debt();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "apply_patch",
        r#"{"input":"*** Begin Patch\n*** Update File: README.md\n@@\n-old\n+updated\n*** End Patch"}"#,
    )
    .await;
    assert_eq!(std::fs::read_to_string(registry.workspace_root().join("README.md")).unwrap(), "updated\n");
    let unrelated = registry
        .execute_tool("exec_command", serde_json::json!({"cmd":"rustc --version"}))
        .await
        .unwrap();
    assert_eq!(unrelated["exit_code"], 0, "the unrelated check must actually pass");
    assert!(registry.completion_verification_pending());
    assert!(!registry.workspace_root().join("verifier-runs").exists());
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "task_tracker",
        r#"{"action":"update","index":1,"status":"completed"}"#,
    )
    .await;
    assert!(registry.completion_verification_pending(), "tracker progress cannot replace verification");
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 6).await;
    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
        "{}",
        backing.rendered_inline_output()
    );
    assert!(registry.completion_verifier_passed("./verify.sh"));
    assert!(!registry.completion_verification_pending());
    assert_eq!(std::fs::read_to_string(registry.workspace_root().join("verifier-runs")).unwrap(), "run\n");
    let ctx = backing.turn_processing_context();
    let calls: Vec<_> = ctx
        .working_history
        .iter()
        .flat_map(|m| m.tool_calls.iter().flatten())
        .filter(|call| call.id.starts_with(super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID))
        .collect();
    assert_eq!(calls.len(), 1);
    assert!(!calls[0].function.as_ref().unwrap().arguments.contains('`'));
    assert_eq!(
        ctx.working_history
            .iter()
            .filter(|m| m.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    assert_eq!(backing.rendered_inline_output().matches("Still working.").count(), 1);
    let emitter = backing.turn_processing_context().harness_emitter.cloned().unwrap();
    emitter.finish().await.unwrap();
    let events = std::fs::read_to_string(events_path).unwrap();
    let messages: Vec<_> = events
        .lines()
        .filter_map(|line| match serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event() {
            ThreadEvent::ItemCompleted(item) => match item.item.details {
                ThreadItemDetails::AgentMessage(message) => Some(message.text),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(messages, ["Still working.\n\nHarness verification: `./verify.sh` exited 0."]);
}
#[tokio::test]
async fn tracker_polling_runs_missing_verifier_then_can_complete_without_more_lists() {
    use vtcode_core::config::constants::tools;
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry
            .execute_tool(
                tools::TASK_TRACKER,
                serde_json::json!({
                    "action":"create", "items":[{"description":"Check README", "verify":["rustc --version"]}]
                }),
            )
            .await
            .unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
    }
    for step in 1..=2 {
        let outcome = drive_tracker_poll(&mut backing, &mut tracker, &mut modified, step).await;
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        let ctx = backing.turn_processing_context();
        assert!(ctx.tool_registry.completion_verification_pending());
    }
    assert!(matches!(
        drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await,
        TurnHandlerOutcome::Continue
    ));
    let ctx = backing.turn_processing_context();
    assert!(ctx.working_history.iter().any(|m| {
        m.role == uni::MessageRole::Tool
            && m.tool_call_id
                .as_deref()
                .is_some_and(|id| id.starts_with(super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID))
    }));
    let done = ctx
        .tool_registry
        .execute_tool(
            tools::TASK_TRACKER,
            serde_json::json!({
                "action":"update", "index":1, "status":"completed"
            }),
        )
        .await
        .unwrap();
    assert_eq!(done["checklist"]["completed"], 1);
    assert!(!ctx.tool_registry.completion_verification_pending());
    assert!(!tracker.verification_is_pending());
}
#[tokio::test]
async fn tracker_polling_stops_when_verification_is_disabled_or_correction_is_ignored() {
    use vtcode_core::config::constants::tools;
    for pending in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(20).await;
        let mut cfg = vtcode_core::config::loader::VTCodeConfig::default();
        cfg.agent.harness.verification.auto_execute = false;
        backing.set_vt_cfg_for_test(cfg);
        let mut tracker = LoopTracker::new();
        let mut modified = BTreeSet::new();
        {
            let ctx = backing.turn_processing_context();
            ctx.tool_registry.allow_all_tools().await.unwrap();
            ctx.tool_registry
                .execute_tool(
                    tools::TASK_TRACKER,
                    serde_json::json!({
                        "action":"create","items":[{"description":"Check README","verify":["rustc --version"]}]
                    }),
                )
                .await
                .unwrap();
            if pending {
                ctx.tool_registry.restore_completion_verification_debt();
            }
        }
        let limit = 4;
        for step in 1..=limit {
            let outcome = drive_tracker_poll(&mut backing, &mut tracker, &mut modified, step).await;
            if step < limit {
                assert!(matches!(outcome, TurnHandlerOutcome::Continue));
            } else {
                assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
            }
        }
        let ctx = backing.turn_processing_context();
        assert!(!ctx.working_history.iter().any(|m| {
            m.tool_call_id
                .as_deref()
                .is_some_and(|id| id.starts_with(super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID))
        }));
        assert_eq!(ctx.tool_registry.completion_verification_pending(), pending);
    }
}
#[tokio::test]
async fn unrelated_passing_verifiers_cannot_starve_the_required_check() {
    use vtcode_core::config::constants::tools;
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry
            .execute_tool(
                tools::TASK_TRACKER,
                serde_json::json!({
                    "action":"create","items":[{"description":"Check README","verify":["rustc --help"]}]
                }),
            )
            .await
            .unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
    }
    for step in 1..=6 {
        let outcome = {
            let mut ctx = backing.turn_processing_context();
            let call = PreparedAssistantToolCall::new(uni::ToolCall::function(
                format!("unrelated-{step}"),
                tools::EXEC_COMMAND.into(),
                serde_json::json!({"cmd":format!("rustc --version && printf 'unrelated-{step}'")}).to_string(),
            ));
            handle_turn_processing_result(HandleTurnProcessingResultParams {
                ctx: &mut ctx,
                processing_result: TurnProcessingResult::ToolCalls {
                    tool_calls: vec![call],
                    assistant_text: String::new(),
                    reasoning: vec![],
                    reasoning_details: None,
                },
                response_streamed: false,
                step_count: step,
                repeated_tool_attempts: &mut tracker,
                turn_modified_files: &mut modified,
                max_tool_loops: 20,
                tool_repeat_limit: 3,
            })
            .await
            .unwrap()
        };
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        let ctx = backing.turn_processing_context();
        assert!(ctx.tool_registry.completion_verification_pending());
    }
    assert!(matches!(
        drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await,
        TurnHandlerOutcome::Continue
    ));
    let ctx = backing.turn_processing_context();
    assert!(ctx.tool_registry.completion_verifier_passed("rustc --help"));
    assert_eq!(
        ctx.working_history
            .iter()
            .filter(|m| m
                .tool_call_id
                .as_deref()
                .is_some_and(|id| id.starts_with(super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID)))
            .count(),
        1
    );
}
#[test]
fn suppresses_result_claims_before_run_tool_output() {
    let tool_calls = vec![prepared_command_tool_call()];
    assert!(should_suppress_pre_tool_result_claim("Found 3 clippy warnings. Let me fix them.", &tool_calls));
}
#[test]
fn keeps_non_result_preamble_for_run_tools() {
    let tool_calls = vec![prepared_command_tool_call()];
    assert!(!should_suppress_pre_tool_result_claim("Running cargo clippy now.", &tool_calls));
}
#[test]
fn records_tool_calls_on_newly_added_assistant_message() {
    let mut history = vec![uni::Message::user("u".to_string())];
    let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
        "call_1".to_string(),
        "code_search".to_string(),
        r#"{"query":"foo"}"#.to_string(),
    ))];

    let len_before_assistant = history.len();
    history.push(uni::Message::assistant("Searching now.".to_string()));

    record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

    assert_eq!(history.len(), 2);
    let last = history.last().expect("assistant message");
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(last.tool_calls.as_ref().map(|calls| calls[0].id.clone()).as_deref(), Some("call_1"));
}
#[test]
fn appends_tool_call_message_when_no_assistant_message_was_added() {
    let mut history = vec![uni::Message::user("u".to_string())];
    let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
        "call_1".to_string(),
        "code_search".to_string(),
        r#"{"query":"foo"}"#.to_string(),
    ))];

    let len_before_assistant = history.len();
    record_assistant_tool_calls(&mut history, &tool_calls, len_before_assistant);

    assert_eq!(history.len(), 2);
    let last = history.last().expect("synthetic assistant tool call message");
    assert_eq!(last.role, uni::MessageRole::Assistant);
    assert_eq!(last.content.as_text(), "");
    assert_eq!(last.phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(last.tool_calls.as_ref().map(|calls| calls[0].id.clone()).as_deref(), Some("call_1"));
}
#[tokio::test]
async fn recovery_tool_calls_break_turn_as_blocked() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");
    assert!(ctx.consume_recovery_pass());

    let tool_calls = vec![PreparedAssistantToolCall::new(uni::ToolCall::function(
        "call_1".to_string(),
        "code_search".to_string(),
        r#"{"query":"loop"}"#.to_string(),
    ))];
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = handle_turn_processing_result(HandleTurnProcessingResultParams {
        ctx: &mut ctx,
        processing_result: TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: String::new(),
            reasoning: Vec::new(),
            reasoning_details: None,
        },
        response_streamed: false,
        step_count: 1,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
        max_tool_loops: 4,
        tool_repeat_limit: 4,
    })
    .await
    .expect("recovery tool calls should be handled");

    assert!(matches!(
        outcome,
        TurnHandlerOutcome::Break(TurnLoopResult::Blocked { reason: Some(reason) })
        if reason == RECOVERY_CONTRACT_VIOLATION_REASON
    ));
}
