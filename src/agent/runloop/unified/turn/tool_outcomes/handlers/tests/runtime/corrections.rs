use super::*;

#[tokio::test]
async fn preflight_archived_tracker_fixture_stops_after_one_corrective_turn() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/tracker_preflight_session.json")).unwrap();
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
    super::super::super::handle_preflight_failure(
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
            super::super::super::handle_preflight_failure(
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
        super::super::super::handle_preflight_failure(&mut ctx, &call_id, tool_names::EXEC_COMMAND, error, Some(&args), None);
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
