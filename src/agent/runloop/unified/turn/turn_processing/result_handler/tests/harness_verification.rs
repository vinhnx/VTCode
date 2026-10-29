use super::*;

#[tokio::test]
async fn harness_auto_verification_success_publishes_draft_once() {
    // `rustc --version` exits 0 without touching workspace files: a
    // hermetic passing verifier for the harness-executed path.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --version");
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    // The text cap fires one harness execution before directive retries.
    let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    assert!(
        !repeated_tool_attempts.verification_is_pending(),
        "harness-executed `rustc --version` must clear the gate"
    );
    assert_eq!(repeated_tool_attempts.consecutive_mutations, 0);
    assert!(
        !repeated_tool_attempts.should_auto_execute_verifier(),
        "a cleared gate must not re-arm harness execution"
    );
    assert!(
        backing.last_history_message_contains("Still working.\n\nHarness verification: `rustc --version` exited 0."),
        "success publishes the retained draft and attributed terminal result"
    );
}
#[tokio::test]
async fn exhausted_budget_blocks_verification_without_reminder_rounds() {
    let mut backing = TestTurnProcessingBacking::new(1).await;
    backing.set_verification_override_for_test("rustc --version");
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.tool_calls = 1;
    let outcome = ctx
        .handle_pending_verification_text_response(&mut tracker, "The change is complete.")
        .unwrap();
    let super::super::PendingVerificationTextOutcome::Block { reason } = outcome else {
        panic!("exhausted execution budget must produce a verification block");
    };
    assert!(reason.contains("Tool-call budget"));
    let final_text = crate::agent::runloop::unified::turn::turn_loop::format_blocked_turn_final_response(&reason);
    assert!(!super::super::helpers::is_pending_verification_block(&reason));
    assert!(!super::super::helpers::verification_block_allows_auto_recovery(&reason, &final_text));
    assert!(reason.starts_with("Tool-call budget exhausted for this turn (1/1)"));
    assert!(final_text.contains("Changes remain unverified"));
    assert!(!final_text.contains("bounded autonomous recovery"));
    assert_eq!(tracker.verification_auto_recovery_attempts(), 0);
    assert!(!tracker.auto_verification_executed);
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
}
#[tokio::test]
async fn wall_clock_exhaustion_has_priority_over_pending_verification_and_tool_calls() {
    let mut backing = TestTurnProcessingBacking::new(1).await;
    let mut tracker = LoopTracker::new();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.tool_calls = 1;
    ctx.harness_state.max_tool_wall_clock = std::time::Duration::from_secs(600);
    ctx.harness_state.turn_started_at = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(601))
        .unwrap();
    let super::super::PendingVerificationTextOutcome::Block { reason } =
        ctx.handle_pending_verification_text_response(&mut tracker, "Done").unwrap()
    else {
        panic!("wall-clock exhaustion must stop completion without scheduling checks");
    };
    assert!(reason.starts_with("Tool wall-clock budget exhausted for this turn (600s)."));
    assert!(reason.contains("no additional verifier was scheduled"));
    assert!(!super::super::helpers::is_pending_verification_block(&reason));
    assert!(!tracker.auto_verification_executed);
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
}
#[tokio::test]
async fn required_verifier_recovery_advances_checks_instead_of_repeating_the_default() {
    use vtcode_core::config::constants::tools;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    let mut cfg = vtcode_core::config::loader::VTCodeConfig::default();
    cfg.agent.harness.verification.auto_execute = true;
    cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".into());
    backing.set_vt_cfg_for_test(cfg);
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut ctx = backing.turn_processing_context();
    ctx.tool_registry.allow_all_tools().await.unwrap();
    let output = ctx.tool_registry.execute_tool(tools::TASK_TRACKER, serde_json::json!({
        "action":"create", "items":[{"description":"Check README", "verify":["rustc --version", "rustc --help"]}]
    })).await.unwrap();
    assert!(output.get("error").is_none(), "{output}");
    ctx.tool_registry.restore_completion_verification_debt();
    let first = ctx
        .tool_registry
        .execute_tool(tools::EXEC_COMMAND, serde_json::json!({"cmd":"rustc --version"}))
        .await
        .unwrap();
    assert_eq!(first["exit_code"], 0);
    assert_eq!(
        super::super::helpers::resolve_pending_verifier_command(ctx.vt_cfg, ctx.tool_registry, ctx.working_history)
            .as_deref(),
        Some("rustc --help")
    );
    let super::super::PendingVerificationTextOutcome::AutoVerify { command } = ctx
        .handle_pending_verification_text_response(&mut tracker, "Still checking.")
        .unwrap()
    else {
        panic!("completion should run the missing check");
    };
    assert_eq!(command, "rustc --help");
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
}
#[tokio::test]
async fn harness_required_verifiers_report_progress_and_finish_without_repeating_checks() {
    use vtcode_core::config::constants::tools;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --invalid-fallback");
    {
        let ctx = backing.turn_processing_context();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        let output = ctx
            .tool_registry
            .execute_tool(
                tools::TASK_TRACKER,
                serde_json::json!({
                    "action":"create", "items":[{"description":"Check", "status":"completed", "verify":["rustc --version", "rustc --help"]}]
                }),
            )
            .await
            .unwrap();
        assert!(output.get("error").is_none(), "{output}");
        ctx.tool_registry.restore_completion_verification_debt();
    }
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut modified = BTreeSet::new();
    for command in ["rustc --version", "rustc --help"] {
        let mut ctx = backing.turn_processing_context();
        let history_start = ctx.working_history.len();
        let outcome = super::super::execute_harness_auto_verification(
            &mut ctx,
            &mut tracker,
            &mut modified,
            1,
            8,
            4,
            command.into(),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, TurnHandlerOutcome::Continue));
        assert!(ctx.tool_registry.completion_verifier_passed(command));
        if command == "rustc --version" {
            assert!(tracker.verification_is_pending());
            assert!(ctx.working_history[history_start..].iter().any(|message| {
                message.role == uni::MessageRole::System
                    && message
                        .content
                        .as_text()
                        .contains("Next required tracker check: `rustc --help`")
            }));
            assert!(tracker.should_auto_execute_verifier());
            assert!(!ctx.working_history[history_start..].iter().any(|message| {
                message.role == uni::MessageRole::System && message.content.as_text().contains("gate is cleared")
            }));
        } else {
            assert!(!tracker.verification_is_pending());
            assert!(!ctx.tool_registry.completion_verification_pending());
        }
        assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
    }
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
            step_count: 3,
            repeated_tool_attempts: &mut tracker,
            turn_modified_files: &mut modified,
            max_tool_loops: 8,
            tool_repeat_limit: 4,
        })
        .await
        .unwrap()
    };
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })));
    let ctx = backing.turn_processing_context();
    let calls = ctx
        .working_history
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .filter(|call| call.id == super::super::helpers::HARNESS_AUTO_VERIFY_CALL_ID)
        .count();
    assert_eq!(calls, 2, "completion must reuse both passes without a third verifier");
}
#[cfg(unix)]
#[tokio::test]
async fn required_custom_verifiers_cross_the_interactive_gate_and_preserve_repair_accounting() {
    use std::os::unix::fs::PermissionsExt;
    use vtcode_core::config::constants::tools;

    for harness_issued in [false, true] {
        for exit_code in [0, 7] {
            let mut backing = TestTurnProcessingBacking::new(8).await;
            let marker = {
                let ctx = backing.turn_processing_context();
                let script = ctx.tool_registry.workspace_root().join("verify.sh");
                let marker = ctx.tool_registry.workspace_root().join("verifier-ran");
                std::fs::write(&script, format!("#!/bin/sh\ntouch verifier-ran\nexit {exit_code}\n")).unwrap();
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
                ctx.tool_registry.allow_all_tools().await.unwrap();
                let mut commands = ctx.tool_registry.commands_config();
                commands.allow_list.push("./verify.sh".into());
                ctx.tool_registry.apply_commands_config(&commands);
                let tracker = ctx
                    .tool_registry
                    .execute_tool(
                        tools::TASK_TRACKER,
                        serde_json::json!({
                            "action":"create", "title":"Custom check", "items":[{
                                "description":"Verify", "status":"completed", "verify":["./verify.sh"]
                            }]
                        }),
                    )
                    .await
                    .unwrap();
                assert!(tracker.get("error").is_none(), "{tracker}");
                ctx.tool_registry.restore_completion_verification_debt();
                marker
            };
            let mut tracker = LoopTracker::new();
            tracker.mark_verification_pending();
            let mut modified = BTreeSet::new();
            if harness_issued {
                let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 6).await;
                assert_eq!(
                    matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
                    exit_code == 0,
                    "exit={exit_code}"
                );
            } else {
                drive_model_tool_call(
                    &mut backing,
                    &mut tracker,
                    &mut modified,
                    tools::EXEC_COMMAND,
                    r#"{"cmd":"./verify.sh"}"#,
                )
                .await;
            }
            assert!(marker.exists(), "the required verifier must execute through the pending gate");
            assert_eq!(tracker.verification_is_pending(), exit_code != 0);
            assert_eq!(
                backing
                    .turn_processing_context()
                    .tool_registry
                    .completion_verification_pending(),
                exit_code != 0
            );
            assert_eq!(tracker.consecutive_mutations, 0, "a declared check is not a repair edit");
        }
    }
}
#[cfg(unix)]
#[tokio::test]
async fn harness_waits_for_a_running_verifier_instead_of_starting_another() {
    use std::os::unix::fs::PermissionsExt;
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("./rustc --version");
    {
        let ctx = backing.turn_processing_context();
        let script = ctx.tool_registry.workspace_root().join("rustc");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'launched\\n' >> launches\nwhile [ ! -f release ]; do sleep 0.02; done\nprintf 'verifier-finished\\n'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        ctx.tool_registry.allow_all_tools().await.unwrap();
        ctx.tool_registry.restore_completion_verification_debt();
        let mut commands = ctx.tool_registry.commands_config();
        commands.allow_list.push("./rustc".to_string());
        ctx.tool_registry.apply_commands_config(&commands);
    }
    let mut tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    let mut modified = BTreeSet::new();
    drive_model_tool_call(
        &mut backing,
        &mut tracker,
        &mut modified,
        "exec_command",
        r#"{"cmd":"./rustc --version","background":true,"yield_time_ms":250}"#,
    )
    .await;
    assert!(tracker.verification_is_pending());
    assert!(
        tracker.pending_verifier_session_id.is_some(),
        "{:?}",
        backing.turn_processing_context().working_history
    );
    let harness_path = backing.enable_harness_emitter();
    let registry = backing.turn_processing_context().tool_registry.clone();
    let release_registry = registry.clone();
    let release = tokio::spawn(async move {
        let observed = tokio::time::timeout(std::time::Duration::from_secs(4), async {
            loop {
                let events = std::fs::read_to_string(&harness_path).unwrap_or_default();
                if events
                    .lines()
                    .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                    .any(|row| {
                        row["event"]["type"] == "item.completed"
                            && row["event"]["item"]["type"] == "tool_invocation"
                            && row["event"]["item"]["tool_name"] == "write_stdin"
                            && row["event"]["item"]["status"] == "completed"
                    })
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .is_ok();
        std::fs::write(release_registry.workspace_root().join("release"), "ready").unwrap();
        observed
    });
    // Recreate the turn-local tracker to exercise live-session rediscovery.
    tracker = LoopTracker::new();
    tracker.mark_verification_pending();
    backing.set_verification_override_for_test("./rustc --version");
    backing.turn_processing_context().harness_state.max_tool_wall_clock = std::time::Duration::from_secs(5);
    let outcome = drive_pending_texts(&mut backing, &mut tracker, &mut modified, 1).await;
    assert!(release.await.unwrap(), "the first wait must execute before releasing the checker");
    let ctx = backing.turn_processing_context();
    assert!(
        matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Completed { .. })),
        "recent verifier history: {:?}",
        ctx.working_history.iter().rev().take(4).collect::<Vec<_>>()
    );
    assert_eq!(
        ctx.working_history
            .iter()
            .filter(|m| m.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    assert!(!tracker.verification_is_pending());
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 0);
    let calls: Vec<_> = ctx
        .working_history
        .iter()
        .filter_map(|message| message.tool_calls.as_ref())
        .flatten()
        .filter(|call| call.id == "harness-auto-verify")
        .collect();
    assert!(calls.len() >= 2, "the yielded check needs fresh repeated waits");
    assert!(
        !ctx.working_history
            .iter()
            .filter(|message| message.tool_call_id.as_deref() == Some("harness-auto-verify"))
            .any(|message| serde_json::from_str::<serde_json::Value>(&message.content.as_text())
                .ok()
                .is_some_and(
                    |output| output.get("reused_recent_result").and_then(serde_json::Value::as_bool) == Some(true)
                )),
        "an in-progress session must never be replayed as a read result"
    );
    assert!(calls.iter().all(|call| call.function.as_ref().unwrap().name == "write_stdin"));
    assert_eq!(
        std::fs::read_to_string(registry.workspace_root().join("launches")).unwrap(),
        "launched\n",
        "all harness polls must reuse the single model-launched session"
    );
}
#[test]
fn verification_failure_attribution_requires_an_observed_exit() {
    assert!(super::super::verifier_response_has_nonzero_exit(r#"{"exit_code":1}"#));
    for response in [
        r#"{"exit_code":0}"#,
        r#"{"session_id":"run-live","lifecycle_state":"running"}"#,
        r#"{"error":"budget exhausted","failure_kind":"policy"}"#,
    ] {
        assert!(!super::super::verifier_response_has_nonzero_exit(response));
    }
}
#[tokio::test]
async fn harness_auto_verification_failure_blocks_attempt_and_records_episode() {
    // Unrecognized rustc flag: fast deterministic non-zero exit, no files.
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_verification_override_for_test("rustc --invalid-flag-xyz");
    let mut repeated_tool_attempts = LoopTracker::new();
    repeated_tool_attempts.consecutive_mutations =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::BLIND_EDITING_THRESHOLD;
    repeated_tool_attempts.mark_verification_pending();
    backing
        .turn_processing_context()
        .tool_registry
        .restore_completion_verification_debt();
    let mut turn_modified_files = BTreeSet::new();

    let outcome = drive_pending_texts(&mut backing, &mut repeated_tool_attempts, &mut turn_modified_files, 6).await;
    assert!(matches!(outcome, TurnHandlerOutcome::Break(TurnLoopResult::Blocked { .. })));
    assert!(repeated_tool_attempts.verification_is_pending(), "a failed verifier must keep the gate pending");

    assert!(
        repeated_tool_attempts.auto_verification_executed,
        "the one-shot flag must stick while the gate stays pending"
    );
    assert!(
        backing.last_history_message_contains("did not clear the gate"),
        "failure must leave an explicit fix-window note"
    );
    let ctx = backing.turn_processing_context();
    assert_eq!(ctx.session_stats.verification_consecutive_failures(), 1);
}
