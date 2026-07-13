#![allow(missing_docs)]
use super::*;
use std::time::Duration;

#[tokio::test]
async fn blocked_tool_call_guard_emits_tool_and_system_messages() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    let mut outcome = None;
    for idx in 0..=max_streak {
        outcome = enforce_blocked_tool_call_guard(
            &mut ctx,
            &format!("blocked_{idx}"),
            tool_names::READ_FILE,
            &args,
        );
    }

    assert!(matches!(
        outcome,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
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
                .contains("Consecutive blocked tool calls reached per-turn cap")
    }));
}

#[tokio::test]
async fn blocked_tool_call_guard_allows_configured_consecutive_cap() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..max_streak {
        let outcome = enforce_blocked_tool_call_guard(
            &mut ctx,
            &format!("blocked_{idx}"),
            tool_names::READ_FILE,
            &args,
        );
        assert!(
            outcome.is_none(),
            "blocked call {idx} should stay under cap"
        );
    }

    let outcome =
        enforce_blocked_tool_call_guard(&mut ctx, "blocked_over_cap", tool_names::READ_FILE, &args);
    assert!(matches!(
        outcome,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
}

#[tokio::test]
async fn blocked_tool_call_guard_caps_non_consecutive_total_churn() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let max_streak = max_consecutive_blocked_tool_calls_per_turn(&ctx);
    let args = json!({"path":"src/main.rs"});

    for idx in 0..(max_streak * 2) {
        let outcome = enforce_blocked_tool_call_guard(
            &mut ctx,
            &format!("blocked_{idx}"),
            tool_names::READ_FILE,
            &args,
        );
        assert!(
            outcome.is_none(),
            "blocked total {idx} should stay under cap"
        );
        ctx.reset_blocked_tool_call_streak();
    }

    let outcome = enforce_blocked_tool_call_guard(
        &mut ctx,
        "blocked_total_over_cap",
        tool_names::READ_FILE,
        &args,
    );
    assert!(matches!(
        outcome,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("blocked_total"))
    );
}

#[tokio::test]
async fn blocked_tool_call_guard_short_circuits_to_recovery_when_active() {
    let mut backing = TestContextBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    let args = json!({"path":"src/main.rs"});
    ctx.activate_recovery("loop detector");

    let outcome =
        enforce_blocked_tool_call_guard(&mut ctx, "blocked_recovery", tool_names::READ_FILE, &args);

    assert!(matches!(outcome, Some(TurnHandlerOutcome::Continue)));
}

#[tokio::test]
async fn unified_validation_ignores_preseeded_legacy_loop_detector_state() {
    let mut backing = TestContextBacking::new(2).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

    backing
        .autonomous_executor
        .set_loop_limit(tool_names::READ_FILE, 2);
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
    assert!(
        backing
            .autonomous_executor
            .is_hard_limit_exceeded(tool_names::READ_FILE)
    );

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_single_tool_call(
        &mut outcome_ctx,
        "legacy_detector_seeded",
        tool_names::READ_FILE,
        valid_args,
    )
    .await
    .expect("unified validation should ignore legacy detector state");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Loop detector stopped repeated")
    }));
    assert!(
        backing
            .autonomous_executor
            .is_hard_limit_exceeded(tool_names::READ_FILE)
    );
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
    assert!(ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("active primary agent policy")
    }));
    assert_eq!(ctx.harness_state.tool_calls, 0);
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
        outcome = enforce_repeated_shell_run_guard(
            &mut ctx,
            &format!("shell_{idx}"),
            tool_names::UNIFIED_EXEC,
            &args,
        );
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

    let first = enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_first",
        tool_names::TASK_TRACKER,
        &args,
    );
    assert!(first.is_none());

    let second = enforce_duplicate_task_tracker_create_guard(
        &mut ctx,
        "task_tracker_second",
        tool_names::TASK_TRACKER,
        &args,
    );
    assert!(matches!(second, Some(ValidationResult::Blocked)));
}

#[tokio::test]
async fn validate_tool_call_blocks_when_wall_clock_budget_exhausted() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.turn_started_at = Instant::now()
        .checked_sub(Duration::from_secs(
            ctx.harness_state.max_tool_wall_clock.as_secs() + 1,
        ))
        .unwrap();

    let result = validate_tool_call(
        &mut ctx,
        "wall_clock_exhausted",
        tool_names::READ_FILE,
        &json!({"path": sample_path}),
    )
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
    let second = validate_tool_call(
        &mut ctx,
        "wall_clock_exhausted_2",
        tool_names::READ_FILE,
        &json!({"path": sample_path}),
    )
    .await
    .expect("validate second wall-clock-exhausted tool call");
    assert!(matches!(second, ValidationResult::Blocked));
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
    assert_eq!(
        policy_violation_count, 1,
        "full policy message must be emitted exactly once per turn"
    );
    assert!(
        ctx.working_history
            .iter()
            .any(|message| { message.content.as_text().contains("call skipped") })
    );

    // Flushing after the batch pushes exactly one "synthesize now" directive so
    // the model produces a final answer from gathered context instead of stalling.
    flush_wall_clock_directive(&mut ctx);
    let directive_count = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains("Synthesize your final answer now")
        })
        .count();
    assert_eq!(
        directive_count, 1,
        "synthesis directive must be pushed once"
    );

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
    flush_wall_clock_directive(&mut ctx);
    let directive_count_after = ctx
        .working_history
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message
                    .content
                    .as_text()
                    .contains("Synthesize your final answer now")
        })
        .count();
    assert_eq!(directive_count_after, 1, "directive must not be duplicated");
}

#[tokio::test]
async fn start_planning_clears_task_tracker_create_signatures() {
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let enter_args = json!({});
    cache_tool_permission(
        &mut backing,
        tool_names::START_PLANNING,
        &enter_args,
        PermissionGrant::Permanent,
    )
    .await;

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

    let result = validate_tool_call(
        &mut ctx,
        "start_planning_call",
        tool_names::START_PLANNING,
        &enter_args,
    )
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
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let args = json!({
        "action": "read",
        "path": backing.sample_file.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(
        &mut outcome_ctx,
        "read_once",
        tool_names::UNIFIED_FILE,
        args.clone(),
    )
    .await
    .expect("first readonly call should succeed");

    assert!(first.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);

    let second = handle_single_tool_call(
        &mut outcome_ctx,
        "read_twice",
        tool_names::UNIFIED_FILE,
        args,
    )
    .await
    .expect("duplicate readonly call should be reused");

    assert!(second.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), 1);
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("\"reused_recent_result\":true")
    }));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("\"result_ref_only\":true")
    }));
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
    std::fs::write(
        &sample_file,
        (1..=16)
            .map(|idx| format!("line {idx}\n"))
            .collect::<String>(),
    )
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
            "action": "read",
            "path": sample_path.clone(),
            "line_start": line,
            "line_end": line
        })
    };

    // Paginated reads: each variant targets a different line range, so each
    // gets a distinct family key (`unified_file::read::<path>::off=<line>`).
    for idx in 1..=read_family_cap {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("read_variant_{idx}"),
            tool_names::UNIFIED_FILE,
            build_paginated_read_args(idx),
        )
        .await
        .expect("paginated read variant should complete");

        assert!(
            outcome.is_none(),
            "paginated read {idx} must not be blocked by the family cap"
        );
    }

    // No pagination burst should have tripped recovery: the streak resets on
    // every distinct slice, so it never reaches the cap.
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .consecutive_same_file_read_family_calls,
        1,
        "paginated reads must reset the family streak, not accumulate it"
    );
    assert!(
        !outcome_ctx.ctx.is_recovery_active(),
        "recovery must not activate for legitimate pagination"
    );
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
    std::fs::write(
        &sample_file,
        (1..=16)
            .map(|idx| format!("line {idx}\n"))
            .collect::<String>(),
    )
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
        "action": "read",
        "path": sample_path.clone(),
        "offset": 0,
        "limit": 4
    });

    for idx in 1..read_family_cap {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("read_repeat_{idx}"),
            tool_names::UNIFIED_FILE,
            identical_args.clone(),
        )
        .await
        .expect("identical read should complete");
        assert!(outcome.is_none(), "read {idx} below cap should not block");
    }
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .consecutive_same_file_read_family_calls,
        read_family_cap - 1,
    );

    let execution_history_len_before_block = outcome_ctx.ctx.tool_registry.execution_history_len();
    let tool_calls_before_block = outcome_ctx.ctx.harness_state.tool_calls;

    let blocked = handle_single_tool_call(
        &mut outcome_ctx,
        "read_repeat_blocked",
        tool_names::UNIFIED_FILE,
        identical_args.clone(),
    )
    .await
    .expect("read-family cap attempt should be handled");

    assert!(matches!(blocked, Some(TurnHandlerOutcome::Continue)));
    assert_eq!(
        outcome_ctx.ctx.tool_registry.execution_history_len(),
        execution_history_len_before_block
    );
    assert_eq!(
        outcome_ctx.ctx.harness_state.tool_calls,
        tool_calls_before_block
    );
    assert_eq!(
        outcome_ctx
            .ctx
            .harness_state
            .consecutive_same_file_read_family_calls,
        read_family_cap
    );
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
async fn denied_tool_permission_emits_policy_response_without_budget_burn() {
    let mut backing = TestContextBacking::new(2).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let denial_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &denial_args,
        PermissionGrant::Denied,
    )
    .await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut tp_ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let outcome = handle_single_tool_call(
        &mut outcome_ctx,
        "denied",
        tool_names::READ_FILE,
        denial_args,
    )
    .await
    .expect("denied permission should be handled");

    assert!(outcome.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("execution denied by policy")
    }));
}

#[tokio::test]
async fn prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

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
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("exceeded max tool calls per turn")
    }));
}

#[tokio::test]
async fn multiple_prepared_tool_calls_respect_unlimited_budget_when_cap_disabled() {
    let mut backing = TestContextBacking::new(0).await;
    backing.select_build_primary_agent();
    let second_file = backing
        .sample_file
        .parent()
        .expect("temp workspace root")
        .join("other.txt");
    std::fs::write(&second_file, "world\n").expect("write second sample file");

    let tool_calls = vec![
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_search_1".to_string(),
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::to_string(&json!({
                "action": "grep",
                "path": ".",
                "pattern": "hello"
            }))
            .expect("serialize tool args"),
        )),
        PreparedAssistantToolCall::new(uni::ToolCall::function(
            "prepared_search_2".to_string(),
            tool_names::UNIFIED_SEARCH.to_string(),
            serde_json::to_string(&json!({
                "action": "grep",
                "path": ".",
                "pattern": "world"
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
    assert!(!outcome_ctx.ctx.working_history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("exceeded max tool calls per turn")
    }));
}

#[tokio::test]
async fn end_to_end_blocked_calls_do_not_burn_budget_before_valid_call() {
    let mut backing = TestContextBacking::new(1).await;
    backing.select_build_primary_agent();
    let valid_file = backing.sample_file.clone();
    let valid_args = json!({"path": valid_file.to_string_lossy()});
    cache_tool_permission(
        &mut backing,
        tool_names::READ_FILE,
        &valid_args,
        PermissionGrant::Permanent,
    )
    .await;

    let mut turn_modified_files: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut tp_ctx = backing.turn_processing_context();

    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut tp_ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let blocked_args = json!({"path":"/var/db/shadow"});
    let first = handle_single_tool_call(
        &mut outcome_ctx,
        "blocked_1",
        tool_names::READ_FILE,
        blocked_args.clone(),
    )
    .await
    .expect("first blocked call should not fail hard");
    assert!(first.is_none());

    let second = handle_single_tool_call(
        &mut outcome_ctx,
        "blocked_2",
        tool_names::READ_FILE,
        blocked_args,
    )
    .await
    .expect("second blocked call should not fail hard");
    assert!(second.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 0);
    assert!(!outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let third = handle_single_tool_call(
        &mut outcome_ctx,
        "valid_1",
        tool_names::READ_FILE,
        valid_args.clone(),
    )
    .await
    .expect("valid call should execute");
    assert!(third.is_none());
    assert_eq!(outcome_ctx.ctx.harness_state.tool_calls, 1);
    assert!(outcome_ctx.ctx.harness_state.tool_budget_exhausted());

    let exhausted = handle_single_tool_call(
        &mut outcome_ctx,
        "exhausted",
        tool_names::READ_FILE,
        valid_args,
    )
    .await
    .expect("exhausted-budget call should return structured outcome");
    assert!(matches!(
        exhausted,
        Some(TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. }))
    ));
    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains("\"continue\" or provide a new instruction")
    }));
}

#[tokio::test]
async fn repeated_read_only_guard_dedups_plan_file_in_planning_mode() {
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();

    // Create a plan file inside the temporary workspace.
    let workspace = backing.sample_file.parent().unwrap().to_path_buf();
    let plans_dir = workspace.join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_path = plans_dir.join("modular-dreaming-pixel.md");
    let plan_content = "# Plan\n\n1. Fix planning-mode clarity\n2. Throttle memory envelopes\n";
    std::fs::write(&plan_path, plan_content).expect("write plan file");

    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.enable_planning();

    let args = json!({
        "action": "read",
        "path": plan_path.to_string_lossy()
    });

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    let first = handle_single_tool_call(
        &mut outcome_ctx,
        "read_plan_1",
        tool_names::UNIFIED_FILE,
        args.clone(),
    )
    .await
    .expect("first plan-file read should succeed");
    assert!(first.is_none(), "first read should execute normally");

    let second = handle_single_tool_call(
        &mut outcome_ctx,
        "read_plan_2",
        tool_names::UNIFIED_FILE,
        args,
    )
    .await
    .expect("second plan-file read should be deduplicated");
    assert!(
        second.is_none(),
        "duplicate read should be handled by guard"
    );

    assert!(outcome_ctx.ctx.working_history.iter().any(|message| {
        message.role == uni::MessageRole::Tool
            && message
                .content
                .as_text()
                .contains("\"reused_recent_result\":true")
    }));
}
