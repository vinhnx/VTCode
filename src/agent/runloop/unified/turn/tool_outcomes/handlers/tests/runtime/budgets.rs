use super::*;

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
