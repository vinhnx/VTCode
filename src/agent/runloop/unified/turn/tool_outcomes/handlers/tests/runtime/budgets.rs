use super::*;

#[tokio::test]
async fn denied_tool_permission_emits_policy_response_without_tool_call_count() {
    let mut backing = TestContextBacking::new().await;
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
async fn prepared_tool_calls_execute_without_call_count_cap() {
    let mut backing = TestContextBacking::new().await;
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
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("Policy violation") })
    );
}

#[tokio::test]
async fn many_prepared_tool_calls_execute_without_call_count_cap() {
    // Regression: the removed per-turn call-count budget must not stop a
    // research-heavy turn mid-batch. Loop and wall-clock ceilings still bind.
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    for index in 0..40 {
        let file = backing
            .sample_file
            .parent()
            .expect("temp workspace root")
            .join(format!("other-{index}.txt"));
        std::fs::write(&file, "world\n").expect("write sample file");
    }

    let tool_calls = (0..40)
        .map(|index| {
            PreparedAssistantToolCall::new(uni::ToolCall::function(
                format!("prepared_read_{index}"),
                tool_names::READ_FILE.to_string(),
                serde_json::to_string(&json!({
                    "path": backing.sample_file.parent().expect("temp workspace root").join(format!("other-{index}.txt")).to_string_lossy()
                }))
                .expect("serialize tool args"),
            ))
        })
        .collect::<Vec<_>>();

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
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 40);
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("Policy violation") })
    );
}

#[tokio::test]
async fn end_to_end_blocked_calls_do_not_count_before_valid_calls() {
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &valid_args, PermissionGrant::Permanent).await;

    // Distinct files: an identical repeat would reuse the recent result
    // instead of executing (and counting) again.
    let second_file = backing.sample_file.parent().expect("temp workspace root").join("other.txt");
    std::fs::write(&second_file, "world\n").expect("write second sample file");
    let second_args = json!({"path": second_file.to_string_lossy()});

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

    for (call_id, args) in [("valid_1", valid_args.clone()), ("valid_2", second_args)] {
        let outcome = handle_single_tool_call(&mut outcome_ctx, call_id, tool_names::READ_FILE, args)
            .await
            .expect("valid call should execute");
        assert!(outcome.is_none());
    }
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 2);
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| { message.content.as_text().contains("Policy violation") })
    );
}

#[tokio::test]
async fn control_plane_wait_needs_no_exemption_without_call_count_cap() {
    let mut backing = TestContextBacking::new().await;
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

    let work_args = json!({"path": sample_file});
    let work = handle_single_tool_call(&mut outcome_ctx, "work_1", tool_names::READ_FILE, work_args)
        .await
        .expect("work call should execute");
    assert!(work.is_none());

    // With no per-turn call-count budget there is no exemption to apply and
    // no exhaustion gate to trip: a wait on a missing session is handled as
    // an ordinary (unguarded) outcome, never a policy rejection.
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
            .any(|message| { message.content.as_text().contains("Policy violation") })
    );
}

#[tokio::test]
async fn pending_verification_allows_more_than_six_edits_and_repairs() {
    let mut backing = TestContextBacking::new().await;
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
