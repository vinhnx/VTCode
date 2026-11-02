use super::*;

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
    assert!(super::super::super::is_cleared_tool_input_echo(Some(&json!({"cleared": "tool_input"}))));
    assert!(!super::super::super::is_cleared_tool_input_echo(Some(&json!({"command": "ls"}))));
    assert!(!super::super::super::is_cleared_tool_input_echo(None));
    assert!(!super::super::super::is_cleared_tool_input_echo(Some(&json!({"cleared": "other"}))));

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
