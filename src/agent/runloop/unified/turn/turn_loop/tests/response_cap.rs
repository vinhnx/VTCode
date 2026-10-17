use super::*;

#[tokio::test]
async fn response_cap_preserves_commentary_as_one_completed_final_harness_message() {
    const FIRST_COMMENTARY: &str = "Let me continue analyzing the results from the first inspection.";
    const SECOND_COMMENTARY: &str = "Let me continue analyzing the results from the second inspection.";

    #[derive(Clone)]
    struct RepeatedCommentaryProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedCommentaryProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let response_text = match self.requests.fetch_add(1, Ordering::SeqCst) {
                0 => FIRST_COMMENTARY,
                1 => SECOND_COMMENTARY,
                _ => "unexpected third response",
            };
            Ok(uni::LLMResponse {
                content: Some(response_text.to_string()),
                model: request.model,
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(RepeatedCommentaryProvider { requests: requests.clone() }));
    let harness_path = backing.enable_harness_emitter();

    let mut history = vec![
        uni::Message::user("inspect the workspace and summarize the results".to_string()),
        uni::Message::assistant(String::new()).with_tool_calls(vec![uni::ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            "{}".to_string(),
        )]),
        uni::Message::tool_response("call_1".to_string(), "inspection complete".to_string()),
    ];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("the capped recovery loop should preserve the final commentary");

    // The verification gate is clear and the latest commentary was promoted
    // to the final answer, so the capped turn completes instead of blocking
    // (session-vtcode-20260912T083718Z: a verified recap was continued into
    // the cap and published as Blocked for finished work).
    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }));
    assert_eq!(requests.load(Ordering::SeqCst), 2, "the response cap must stop before a third request");
    assert!(!outcome.final_response_was_fallback);
    assert_eq!(final_answer_text(&history), SECOND_COMMENTARY);
    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1,
        "the preserved commentary must become the only final assistant message"
    );
    assert!(
        !history
            .iter()
            .any(|message| message.content.as_text().contains(COMPLETED_TURN_FALLBACK_RESPONSE)),
        "the cap must not replace substantive commentary with the generic fallback"
    );

    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches(FIRST_COMMENTARY).count(), 1);
    assert_eq!(rendered.matches(SECOND_COMMENTARY).count(), 1);

    let harness = fs::read_to_string(harness_path).expect("read capped recovery harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("capped recovery harness output should use the versioned event contract")
                .into_event()
        })
        .collect::<Vec<_>>();
    let agent_messages = events
        .iter()
        .filter_map(|event| {
            let ThreadEvent::ItemCompleted(item) = event else {
                return None;
            };
            let ThreadItemDetails::AgentMessage(message) = &item.item.details else {
                return None;
            };
            Some(message.text.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_messages, vec![SECOND_COMMENTARY]);
    assert!(events.iter().any(|event| matches!(event, ThreadEvent::TurnCompleted(_))));
    assert!(!events.iter().any(|event| matches!(event, ThreadEvent::TurnFailed(_))));
}

#[tokio::test]
async fn successful_tool_round_resets_consecutive_text_response_cap() {
    const FIRST_COMMENTARY: &str = "Let me continue by inspecting the first result.";
    const SECOND_COMMENTARY: &str = "Let me continue by checking the tool result.";
    const FINAL_RESPONSE: &str = "The requested inspection is complete and validation passed";

    #[derive(Clone)]
    struct CommentaryWithToolProgressProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for CommentaryWithToolProgressProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            let (content, tool_calls) = match request_number {
                0 => (Some(FIRST_COMMENTARY.to_string()), None),
                1 => (
                    None,
                    Some(vec![uni::ToolCall::function(
                        "productive-tool-call".to_string(),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({
                            "patch": "*** Begin Patch\n*** Add File: productive-tool-progress.txt\n+tool progress\n*** End Patch\n"
                        })
                        .to_string(),
                    )]),
                ),
                2 => (Some(SECOND_COMMENTARY.to_string()), None),
                3 => (Some(FINAL_RESPONSE.to_string()), None),
                _ => panic!("unexpected request after final response"),
            };
            Ok(uni::LLMResponse {
                content,
                model: request.model,
                tool_calls,
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
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(CommentaryWithToolProgressProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user(
        "inspect the workspace and finish the analysis".to_string(),
    )];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("tool progress should break the text-only response streak");

    assert!(matches!(outcome.result, TurnLoopResult::Completed { plan_approved_execution_pending: false }));
    assert_eq!(requests.load(Ordering::SeqCst), 4, "the final request must run after tool progress");
    assert_eq!(final_answer_text(&history), FINAL_RESPONSE);
    assert!(backing.workspace_path().join("productive-tool-progress.txt").exists());
}

#[tokio::test]
async fn blocked_mutation_does_not_reset_consecutive_text_response_cap() {
    #[derive(Clone)]
    struct TextBlockedMutationTextProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for TextBlockedMutationTextProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let request_number = self.requests.fetch_add(1, Ordering::SeqCst);
            // Texts at 0, 2, 3, 5, 6, 7 (six total = 2 * (1 + 2 auto-recovery
            // attempts)); blocked mutations at 1 and 4 must retain — never
            // reset — the text-response streak, so the cap-hits (and recovery
            // grants) land exactly on the even texts.
            let (content, tool_calls) = match request_number {
                0 => (Some("The unverified change is complete.".to_string()), None),
                1 | 4 => (
                    None,
                    Some(vec![uni::ToolCall::function(
                        "blocked-mutation".to_string(),
                        tool_names::APPLY_PATCH.to_string(),
                        json!({
                            "patch": "*** Begin Patch\n*** Add File: must-not-exist.rs\n+blocked\n*** End Patch\n"
                        })
                        .to_string(),
                    )]),
                ),
                2 => (Some("The unverified change is still complete.".to_string()), None),
                3 => (Some("The unverified change is still complete (2).".to_string()), None),
                5 => (Some("The unverified change is still complete (3).".to_string()), None),
                6 => (Some("The unverified change is still complete (4).".to_string()), None),
                7 => (Some("The unverified change is still complete (5).".to_string()), None),
                _ => panic!("blocked mutation incorrectly reset the response streak"),
            };
            Ok(uni::LLMResponse {
                content,
                model: request.model,
                tool_calls,
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
    backing.set_provider(Box::new(TextBlockedMutationTextProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("finish the pending change".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.session_stats.set_verification_snapshot((true, 0));
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("blocked mutation should retain the text-response streak");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        8,
        "the sixth text response must reach the cap after auto-recovery exhaustion"
    );
    assert!(!backing.workspace_path().join("must-not-exist.rs").exists());
}

#[tokio::test]
async fn stale_plan_pause_discards_text_without_consuming_response_budget() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct RepeatedStalePauseProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for RepeatedStalePauseProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            self.requests.fetch_add(1, Ordering::SeqCst);
            Ok(uni::LLMResponse {
                content: Some(
                    "Implementation is paused because tool use is disabled. Wait for the next turn.".to_string(),
                ),
                model: request.model,
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
    backing.set_provider(Box::new(RepeatedStalePauseProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("continue the approved implementation".to_string())];
    let turn_context = backing.turn_loop_context();
    turn_context.harness_state.set_approved_plan_execution(true);
    let outcome = run_turn_loop(&mut history, turn_context)
        .await
        .expect("turn loop should stop at the bounded stale-pause retries");

    assert!(matches!(outcome.result, TurnLoopResult::Blocked { .. }));
    assert!(outcome.final_response_was_fallback);
    // Two bounded clearing retries plus the terminal stale response that ends
    // Blocked. Discarded stale text never consumes the generic text-response
    // budget, so the generic cap is not what stops this loop.
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert!(!history.iter().any(|message| {
        message
            .content
            .as_text()
            .contains("Implementation is paused because tool use is disabled.")
    }));
}
