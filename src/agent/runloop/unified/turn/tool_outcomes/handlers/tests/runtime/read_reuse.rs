use super::*;

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
