use super::*;

#[tokio::test]
async fn turn_loop_preserves_legacy_loop_detector_state() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_loop_limit(tool_names::READ_FILE, 2);
    let seeded_args = json!({"path":"sample.txt"});
    assert!(backing.record_tool_call(tool_names::READ_FILE, &seeded_args).is_none());
    let _ = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    let warning = backing.record_tool_call(tool_names::READ_FILE, &seeded_args);
    assert!(warning.is_some());
    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));

    let mut history = vec![uni::Message::user("continue".to_string())];
    run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("turn loop should complete");

    assert!(backing.is_hard_limit_exceeded(tool_names::READ_FILE));
}

#[tokio::test]
async fn resumed_turn_cannot_complete_while_verification_is_pending() {
    #[derive(Clone)]
    struct TextOnlyProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextOnlyProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some("The requested work is complete.".to_string()),
                model: request.model.clone(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(8).await;
    backing.set_provider(Box::new(TextOnlyProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("continue the implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("a resumed unverified turn should produce a blocked handoff");

    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason.contains("Verification is still pending")
    ));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "missing verification ends this completion attempt without reminders"
    );
    assert!(outcome.final_response_was_fallback);
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System
            && message
                .content
                .as_text()
                .contains(crate::agent::runloop::unified::turn::tool_outcomes::helpers::ANTI_BLIND_EDITING_DIRECTIVE)
    }));
    assert!(!history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("Verification recovery (")
    }));
    assert!(
        !history
            .iter()
            .any(|message| { message.content.as_text().contains("The requested work is complete.") })
    );
}

#[tokio::test]
async fn harness_auto_verification_completes_text_only_turn_without_manual_continue() {
    #[derive(Clone)]
    struct TextOnlyProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextOnlyProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some("The requested work is complete.".to_string()),
                model: request.model.clone(),
                tool_calls: None,
                usage: None,
                finish_reason: uni::FinishReason::Stop,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            })
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(12).await;
    backing.set_provider(Box::new(TextOnlyProvider { requests: requests.clone() }));
    // Hermetic passing verifier: `rustc --version` exits 0 with no files.
    backing.set_verification_override_for_test("rustc --version");

    let mut history = vec![uni::Message::user("continue the implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("harness auto-verification should complete the turn");

    // The provider claims completion on its FIRST text, so the claim fast
    // path fires immediately: the harness runs `rustc --version` itself, the
    // gate clears and the retained response completes without resampling.
    assert!(
        matches!(outcome.result, TurnLoopResult::Completed { .. }),
        "expected Completed, got: {:?}",
        outcome.result
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(!outcome.final_response_was_fallback);
    assert!(
        !history.iter().any(|message| {
            message.role == uni::MessageRole::System && message.content.as_text().contains("Verification recovery (")
        }),
        "fast path must skip directive rounds entirely"
    );
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains("gate is cleared")
    }));
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::Assistant
            && message.content.as_text().contains("The requested work is complete.")
    }));
}

#[tokio::test]
async fn anti_blind_guard_stops_outer_loop_after_bounded_pending_stale_plan_pause_responses() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct RepeatedTextAfterMutationsProvider {
        requests: Arc<AtomicUsize>,
        text_responses: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedTextAfterMutationsProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let response = if request_number < 6 {
                let path = format!("anti-blind-regression-{request_number}.rs");
                let patch =
                    format!("*** Begin Patch\n*** Add File: {path}\n+mutation {request_number}\n*** End Patch\n");
                uni::LLMResponse {
                    content: None,
                    model: request.model.clone(),
                    tool_calls: Some(vec![uni::ToolCall::function(
                        format!("mutation-{request_number}"),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({"patch": patch}).to_string(),
                    )]),
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            } else {
                self.text_responses.fetch_add(1, Ordering::SeqCst);
                uni::LLMResponse {
                    content: Some(
                        "Implementation is paused because tool use is disabled. Wait for the next turn.".to_string(),
                    ),
                    model: request.model.clone(),
                    tool_calls: None,
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            };
            Ok(response)
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let text_responses = Arc::new(AtomicUsize::new(0));
    let mut backing = TestTurnProcessingBacking::new(16).await;
    backing.set_provider(Box::new(RepeatedTextAfterMutationsProvider {
        requests: requests.clone(),
        text_responses: text_responses.clone(),
    }));

    let mut history = vec![uni::Message::user("apply and verify the requested change".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("turn loop should stop at the pending-verification response cap");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason.contains("Verification is still pending")
    ));
    assert_eq!(text_responses.load(Ordering::SeqCst), 1);
    assert_eq!(requests.load(Ordering::SeqCst), 7, "six edits then one completion attempt");
    assert!(outcome.final_response_was_fallback);
    assert!(history.iter().any(|message| {
        message.phase == Some(uni::AssistantPhase::FinalAnswer)
            && message.content.as_text().contains("Completion remains unverified")
    }));
    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    assert!(!history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Implementation is paused because tool use is disabled.")
    }));
}

#[tokio::test]
async fn blocked_anti_blind_recovery_publishes_one_actionable_handoff() {
    #[derive(Clone)]
    struct VerificationRecoveryProvider {
        requests: Arc<AtomicUsize>,
        steps: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for VerificationRecoveryProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            // Bounded tool-failure diagnosis issues auxiliary requests on the
            // same provider client (main-model route when no lightweight model
            // is configured). Serve them with a valid diagnosis JSON without
            // consuming the scripted main-loop sequence below.
            if request
                .system_prompt
                .as_deref()
                .is_some_and(|prompt| prompt.contains("failure-diagnosis checkpoint"))
            {
                return Ok(uni::LLMResponse {
                    content: Some(
                        r#"{"observed":"the scripted tool call failed","likely_cause":"intentional test failure","next_action":"inspect the tool output and retry with corrected arguments"}"#
                            .to_string(),
                    ),
                    model: request.model.clone(),
                    tool_calls: None,
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                });
            }
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let tool_call = |label: &str, tool_name: &str, args: serde_json::Value| {
                self.steps.lock().expect("step trace lock").push(label.to_string());
                uni::LLMResponse {
                    content: None,
                    model: request.model.clone(),
                    tool_calls: Some(vec![uni::ToolCall::function(
                        format!("verification-{request_number}"),
                        tool_name.to_string(),
                        args.to_string(),
                    )]),
                    usage: None,
                    finish_reason: uni::FinishReason::Stop,
                    reasoning: None,
                    reasoning_details: None,
                    organization_id: None,
                    request_id: None,
                    tool_references: Vec::new(),
                    compaction: None,
                }
            };

            let patch = |path: &str, contents: &str| {
                format!("*** Begin Patch\n*** Add File: {path}\n+{contents}\n*** End Patch\n")
            };

            let response = match request_number {
                0..=4 => tool_call(
                    "successful_edit",
                    tool_names::APPLY_PATCH,
                    json!({"patch": patch(&format!("anti-blind-sequence-{request_number}.rs"), "effective edit")}),
                ),
                5 => tool_call(
                    "failed_patch",
                    tool_names::APPLY_PATCH,
                    json!({
                        "patch": "*** Begin Patch\n*** Update File: missing-target.rs\n@@\n-old\n+new\n*** End Patch\n"
                    }),
                ),
                6 => tool_call(
                    "inspection",
                    tool_names::EXEC_COMMAND,
                    json!({"cmd": "rg -n 'effective edit' . || true"}),
                ),
                7 => tool_call(
                    "link_check",
                    tool_names::EXEC_COMMAND,
                    json!({"cmd": "rg -n '\\[[^]]+\\]\\([^)]*\\)' . || true"}),
                ),
                8 => tool_call("diff_check", tool_names::EXEC_COMMAND, json!({"cmd": "git diff --check"})),
                9 => tool_call(
                    "successful_edit",
                    tool_names::APPLY_PATCH,
                    json!({"patch": patch("anti-blind-sequence-final.rs", "last effective edit")}),
                ),
                _ => {
                    self.steps.lock().expect("step trace lock").push("unverified_text".to_string());
                    uni::LLMResponse {
                        content: Some("The edits are complete, but verification was not run.".to_string()),
                        model: request.model.clone(),
                        tool_calls: None,
                        usage: None,
                        finish_reason: uni::FinishReason::Stop,
                        reasoning: None,
                        reasoning_details: None,
                        organization_id: None,
                        request_id: None,
                        tool_references: Vec::new(),
                        compaction: None,
                    }
                }
            };
            Ok(response)
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".to_string()]
        }

        fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
    }

    let requests = Arc::new(AtomicUsize::new(0));
    let steps = Arc::new(Mutex::new(Vec::new()));
    let mut backing = TestTurnProcessingBacking::new(20).await;
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(VerificationRecoveryProvider { requests: requests.clone(), steps: steps.clone() }));

    let mut history = vec![uni::Message::user("apply the change and verify it".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    turn_context.session_stats.preflight_recovery.reject(
        tool_names::TASK_TRACKER,
        crate::agent::runloop::unified::state::ValidationCategory::Arguments,
        Some(&json!({"action":"list","index":1})),
        6,
        false,
    );
    turn_context.session_stats.preflight_recovery.synthesis();
    turn_context.session_stats.preflight_recovery.begin_corrective_turn();
    turn_context.session_stats.preflight_recovery.finish_corrective_turn();
    assert!(turn_context.session_stats.preflight_recovery.exhausted());
    turn_context.session_stats.record_tracker_continuation_turn_with_limit(3);
    turn_context.session_stats.record_plan_continuation_turn_with_limit(3);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("anti-blind recovery should return a blocked outcome");
    let execution_state = backing.turn_loop_context();
    assert!(execution_state.session_stats.preflight_recovery.is_clear());
    assert_eq!(execution_state.session_stats.tracker_continuation_turns(), 1);
    assert_eq!(execution_state.session_stats.plan_continuation_turns(), 1);

    assert!(
        matches!(
            outcome.result,
            TurnLoopResult::Blocked {
                reason: Some(ref reason)
            } if reason.contains("Verification is still pending")
        ),
        "expected PENDING_VERIFICATION_BLOCK_REASON, got: {:?}, steps: {:?}",
        outcome.result,
        steps.lock().expect("step trace lock").as_slice()
    );
    assert_eq!(requests.load(Ordering::SeqCst), 11, "ten tool steps then one completion attempt");
    assert_eq!(
        steps.lock().expect("step trace lock").as_slice(),
        [
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "successful_edit",
            "failed_patch",
            "inspection",
            "link_check",
            "diff_check",
            "successful_edit",
            "unverified_text",
        ]
    );
    assert_blocked_response_surfaces(&mut backing, &history, &harness_path, PENDING_VERIFICATION_RESPONSE_MARKER);
}

struct RetainedCompletionProvider {
    calls: Arc<AtomicUsize>,
    streaming: bool,
}

#[async_trait::async_trait]
impl uni::LLMProvider for RetainedCompletionProvider {
    fn name(&self) -> &str {
        "openai"
    }
    fn supports_streaming(&self) -> bool {
        self.streaming
    }
    async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(uni::LLMResponse {
            content: Some("WebMCP inspection is complete.".into()),
            model: request.model,
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
            compaction: None,
        })
    }
    async fn stream(&self, request: uni::LLMRequest) -> Result<uni::LLMStream, uni::LLMError> {
        let response = self.generate(request).await?;
        Ok(Box::pin(async_stream::stream! {
            yield Ok(uni::LLMStreamEvent::Token { delta: "WebMCP inspection ".into() });
            yield Ok(uni::LLMStreamEvent::Token { delta: "is complete.".into() });
            yield Ok(uni::LLMStreamEvent::Completed { response: Box::new(response) });
        }))
    }
    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".into()]
    }
    fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[tokio::test]
async fn retained_completion_runs_missing_checks_and_publishes_once_without_model_follow_up() {
    for streaming in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let mut backing = TestTurnProcessingBacking::new(8).await;
        backing.set_provider(Box::new(RetainedCompletionProvider { calls: calls.clone(), streaming }));
        let harness_path = backing.enable_harness_emitter();
        {
            let ctx = backing.turn_processing_context();
            ctx.tool_registry.allow_all_tools().await.unwrap();
            ctx.tool_registry
                .execute_tool(
                    tool_names::TASK_TRACKER,
                    json!({
                        "action":"create", "items":[{"description":"Verify", "status":"completed",
                        "verify":["rustc --version", "rustc --help"]}]
                    }),
                )
                .await
                .unwrap();
            ctx.tool_registry.restore_completion_verification_debt();
        }
        let mut history = vec![uni::Message::user("Explain WebMCP.".into())];
        run_turn_loop(&mut history, backing.turn_loop_context()).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1, "verification must not resample the answer");
        let expected = "WebMCP inspection is complete.\n\nHarness verification: `rustc --version` exited 0; `rustc --help` exited 0.";
        let finals: Vec<_> = history
            .iter()
            .filter(|m| m.phase == Some(uni::AssistantPhase::FinalAnswer))
            .collect();
        assert_eq!(finals.len(), 1);
        assert_eq!(finals[0].content.as_text(), expected);
        assert_eq!(
            backing
                .rendered_inline_output()
                .matches("WebMCP inspection is complete.")
                .count(),
            1
        );
        let commands: Vec<_> = history
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .flatten()
            .filter(|call| call.id == "harness-auto-verify")
            .collect();
        assert_eq!(commands.len(), 2);
        let events = fs::read_to_string(harness_path).unwrap();
        let messages: Vec<_> = events
            .lines()
            .filter_map(|line| {
                let event = serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event();
                match event {
                    ThreadEvent::ItemCompleted(item) => match item.item.details {
                        ThreadItemDetails::AgentMessage(message) => Some(message.text),
                        _ => None,
                    },
                    _ => None,
                }
            })
            .collect();
        assert_eq!(messages, vec![expected.to_string()], "canonical history must never expose the unchecked draft");
    }
}
