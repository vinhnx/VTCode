use super::*;

#[test]
fn blocked_turn_final_response_explains_pending_verification() {
    let response = blocked_turn_final_response(PENDING_VERIFICATION_BLOCK_REASON);

    assert!(response.contains("Inspection-only checks do not clear the verification gate"));
    assert!(response.contains("cargo check --locked"));
    assert!(response.contains(vtcode_core::tools::tool_intent::GENERIC_VERIFIER_DESCRIPTION));
    assert!(response.contains(vtcode_core::tools::tool_intent::VERIFIER_SHELL_FORM_NOTE));
    assert!(response.contains("autonomous recovery"));
    assert!(response.contains("max_output_tokens"));
    assert!(response.contains("continue"));
    // The block fires whether or not a project verifier was detected, so the
    // notice must not claim the harness already ran one, and must not restate
    // a stricter pipe rule than the classifier applies.
    assert!(!response.contains("already tried"), "{response}");
    assert!(!response.contains("no `|`"), "{response}");
}

#[test]
fn blocked_turn_final_response_explains_context_capacity_failure() {
    let response =
        blocked_turn_final_response(&format!("recovery failed: {POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON}"));

    assert!(response.contains("context capacity"));
    assert!(response.contains("retained"));
    assert!(response.contains("resume"));
    assert!(response.contains("switch models"));
}

#[test]
fn blocked_turn_final_response_is_not_suppressed_by_prior_event_state() {
    // A prior streamed event is not evidence that this blocked outcome has a
    // final assistant item; the blocked handoff must still be selected.
    assert!(
        blocked_turn_final_response(&format!("prior event; {PENDING_VERIFICATION_BLOCK_REASON}"))
            .contains("Inspection-only checks")
    );
}

#[test]
fn blocked_turn_final_response_has_generic_fallback() {
    let response = blocked_turn_final_response("blocked");

    assert_eq!(response, GENERIC_BLOCKED_FINAL_RESPONSE);

    let empty_response = blocked_turn_final_response("");
    assert_eq!(empty_response, GENERIC_BLOCKED_FINAL_RESPONSE);
}

#[test]
fn blocked_turn_final_response_formats_tool_call_limit() {
    let response = blocked_turn_final_response("Blocked tool-call limit reached after 4 calls");
    assert_eq!(
        response,
        "The turn stopped because repeated tool calls were rejected: blocked tool-call limit reached after 4 calls. \
         Resume the request with specific guidance, or adjust permissions or tools to continue."
    );
}

#[test]
fn blocked_turn_final_response_formats_repeated_shell() {
    let response = blocked_turn_final_response("Repeated shell command detected");
    assert!(response.contains("repeated identical shell commands were detected"));
    assert!(!response.contains("retained"));
}

#[test]
fn blocked_turn_final_response_formats_specific_reason() {
    let response = blocked_turn_final_response("specific error happened");
    assert_eq!(
        response,
        "The turn stopped: specific error happened. Resume the request or give updated instructions to continue."
    );
}

#[test]
fn blocked_turn_final_response_reads_cleanly_for_sentence_reasons() {
    // Reasons are full sentences; the notice must not produce "..", keep a
    // capital after the colon, or claim anything about retained history.
    for reason in [
        ASSISTANT_TEXT_RESPONSE_CAP_REASON,
        "Provider stream ended unexpectedly.",
        "Blocked tool-call limit reached.",
    ] {
        let response = blocked_turn_final_response(reason);
        assert!(!response.contains(".."), "double period in: {response}");
        assert!(!response.contains(": T") && !response.contains(": P") && !response.contains(": B"), "{response}");
        assert!(!response.contains("before success could be confirmed"), "{response}");
        assert!(!response.contains("retained"), "{response}");
    }
    assert!(!GENERIC_BLOCKED_FINAL_RESPONSE.contains("retained"));
    assert!(!GENERIC_BLOCKED_FINAL_RESPONSE.contains("before success could be confirmed"));
}

#[test]
fn blocked_turn_final_response_keeps_acronym_case_after_colon() {
    let response = blocked_turn_final_response("API quota exhausted");
    assert!(response.starts_with("The turn stopped: API quota exhausted."), "{response}");
}

#[test]
fn blocked_turn_final_response_returns_refusal_notice_verbatim() {
    let reason = vtcode_core::core::agent::refusal::refusal_reason(&uni::LLMResponse {
        finish_reason: uni::FinishReason::Refusal,
        reasoning_details: Some(vec![
            serde_json::json!({
                "type": "stop_details",
                "category": "cyber",
                "explanation": "This looks like malware development."
            })
            .to_string(),
        ]),
        ..uni::LLMResponse::default()
    });

    let response = blocked_turn_final_response(&reason);

    assert_eq!(
        response,
        "The model declined this request (category: cyber): this looks like malware development. \
         The request was not retried; rephrase it or switch models."
    );
}

#[tokio::test]
async fn context_capacity_blocked_recovery_publishes_one_actionable_handoff() {
    #[derive(Clone)]
    struct ContextCapacityProvider {
        requests: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl uni::LLMProvider for ContextCapacityProvider {
        fn name(&self) -> &str {
            "openai"
        }

        fn supports_streaming(&self) -> bool {
            false
        }

        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            if self.requests.fetch_add(1, Ordering::SeqCst) == 0 {
                let patch =
                    "*** Begin Patch\n*** Add File: context-capacity-sequence.txt\n+effective edit\n*** End Patch\n";
                return Ok(uni::LLMResponse {
                    content: None,
                    model: request.model,
                    tool_calls: Some(vec![uni::ToolCall::function(
                        "context-capacity-edit".to_string(),
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
                });
            }

            Err(uni::LLMError::InvalidRequest {
                message: "maximum context length is 114688 tokens".to_string(),
                metadata: None,
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
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(ContextCapacityProvider { requests: requests.clone() }));

    let mut history = vec![uni::Message::user("apply the change".to_string())];
    let outcome = run_turn_loop(&mut history, backing.turn_loop_context())
        .await
        .expect("context-capacity recovery should return a blocked outcome");

    assert!(matches!(
        outcome.result,
        TurnLoopResult::Blocked {
            reason: Some(ref reason)
        } if reason == POST_TOOL_CONTEXT_COMPACTION_FAILED_REASON
    ));
    assert_eq!(requests.load(Ordering::SeqCst), 2, "context failure should stop after the bounded retry path");
    assert!(history.iter().any(|message| {
        message.role == uni::MessageRole::System && message.content.as_text().contains(POST_TOOL_RESUME_DIRECTIVE)
    }));
    assert_blocked_response_surfaces(&mut backing, &history, &harness_path, CONTEXT_CAPACITY_RESPONSE_MARKER);
}

#[tokio::test]
async fn refused_turn_renders_refusal_notice_over_stale_final_answer() {
    let stale_final = "Here is the answer to the earlier part of this turn.";
    let notice = "The model declined this request (category: cyber). \
                  The request was not retried; rephrase it or switch models.";
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let mut history = vec![
        uni::Message::user("refused request".to_string()),
        uni::Message::assistant(stale_final.to_string()).with_phase(Some(uni::AssistantPhase::FinalAnswer)),
    ];
    let history_before = history.clone();
    let blocked = TurnLoopResult::Blocked { reason: Some(notice.to_string()) };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.mark_turn_refused();
        // The stale final was already shown; the refusal must still render.
        context.harness_state.mark_final_response_rendered();
        ensure_blocked_turn_response(&mut context, &mut history, 0, notice).expect("refusal notice");
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    assert_eq!(history, history_before, "the refusal notice must not enter model-visible history");
    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches("The model declined this request").count(), 1, "{rendered}");

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let agent_messages = harness
        .lines()
        .filter_map(|line| {
            let event = serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event();
            let ThreadEvent::ItemCompleted(item) = event else {
                return None;
            };
            let ThreadItemDetails::AgentMessage(message) = item.item.details else {
                return None;
            };
            Some(message.text)
        })
        .collect::<Vec<_>>();
    assert_eq!(agent_messages, vec![notice.to_string()]);
}

#[tokio::test]
async fn blocked_recovery_does_not_duplicate_prior_harness_agent_message() {
    let prior_text = "A streamed progress message was already published.";
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();
    backing.emit_harness_assistant_message_for_test(prior_text);

    let mut history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some(PENDING_VERIFICATION_BLOCK_REASON.to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.mark_final_response_event_emitted();
        ensure_blocked_turn_response(&mut context, &mut history, 1, PENDING_VERIFICATION_BLOCK_REASON)
            .expect("blocked recovery handoff");
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    assert_eq!(
        history
            .iter()
            .filter(|message| message.phase == Some(uni::AssistantPhase::FinalAnswer))
            .count(),
        1
    );
    let final_text = final_answer_text(&history);
    assert!(final_text.contains(PENDING_VERIFICATION_RESPONSE_MARKER));
    let rendered = backing.rendered_inline_output();
    assert_eq!(rendered.matches(PENDING_VERIFICATION_RESPONSE_MARKER).count(), 1);

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
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
    assert_eq!(agent_messages, vec![prior_text]);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ThreadEvent::TurnFailed(_)))
            .count(),
        1
    );
    assert!(!events.iter().any(|event| matches!(event, ThreadEvent::TurnCompleted(_))));
}

#[tokio::test]
async fn finalize_turn_blocked_event_populates_fuse_telemetry() {
    use crate::agent::runloop::unified::run_loop_context::BlockedToolRecoveryTelemetry;

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some("blocked tool-call limit reached".to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.arm_blocked_tool_recovery(
            "blocked tool-call limit reached",
            BlockedToolRecoveryTelemetry {
                last_tool: "exec_command 'cargo check'".to_string(),
                consecutive_cap: 3,
                total_cap: 6,
                blocked_streak: 4,
                blocked_total: 5,
            },
        );
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();
    let blocked_events = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnBlocked(blocked) => Some(blocked),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocked_events.len(), 1, "exactly one turn.blocked event");
    let blocked_event = blocked_events[0];
    assert_eq!(blocked_event.last_tool.as_deref(), Some("exec_command 'cargo check'"));
    assert_eq!(blocked_event.consecutive_cap, 3);
    assert_eq!(blocked_event.total_cap, 6);
    assert_eq!(blocked_event.blocked_streak, 4);
    assert_eq!(blocked_event.blocked_total, 5);
}

#[tokio::test]
async fn finalize_turn_blocked_event_without_fuse_keeps_legacy_fields() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some(PENDING_VERIFICATION_BLOCK_REASON.to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.consecutive_blocked_tool_calls = 2;
        context.harness_state.blocked_tool_calls = 7;
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();
    let blocked_events = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::TurnBlocked(blocked) => Some(blocked),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocked_events.len(), 1, "exactly one turn.blocked event");
    let blocked_event = blocked_events[0];
    // Blocks that did not trip the fuse keep the live counters and the
    // previous `None`/zero fields.
    assert_eq!(blocked_event.last_tool, None);
    assert_eq!(blocked_event.blocked_streak, 2);
    assert_eq!(blocked_event.blocked_total, 7);
    assert_eq!(blocked_event.consecutive_cap, 0);
    assert_eq!(blocked_event.total_cap, 0);
}

#[tokio::test]
async fn finalize_turn_closes_streamed_items_that_never_reached_the_pipeline() {
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let harness_path = backing.enable_harness_emitter();

    let history = vec![uni::Message::user("resume the request".to_string())];
    let blocked = TurnLoopResult::Blocked {
        reason: Some("blocked tool-call limit reached".to_string()),
    };
    {
        let mut context = backing.turn_loop_context();
        context.harness_state.remember_streamed_tool_call_items([(
            "call_dangling".to_string(),
            StreamedToolCallItem {
                item_id: "turn-step-1-assistant-stream-1-tool-call-call_dangling".to_string(),
                tool_name: "exec_command".to_string(),
            },
        )]);
        finalize_turn(&mut context, &history, &blocked, &HarnessUsage::default()).await;
    }

    let harness = fs::read_to_string(harness_path).expect("read harness events");
    let events = harness
        .lines()
        .map(|line| {
            serde_json::from_str::<VersionedThreadEvent>(line)
                .expect("versioned harness event")
                .into_event()
        })
        .collect::<Vec<_>>();

    let mut invocation_closed = false;
    let mut output_closed = false;
    for event in &events {
        let ThreadEvent::ItemCompleted(completed) = event else {
            continue;
        };
        match &completed.item.details {
            ThreadItemDetails::ToolInvocation(invocation) => {
                assert_eq!(completed.item.id, "turn-step-1-assistant-stream-1-tool-call-call_dangling");
                assert_eq!(invocation.tool_name, "exec_command");
                assert_eq!(invocation.tool_call_id.as_deref(), Some("call_dangling"));
                assert_eq!(invocation.status, ToolCallStatus::Failed);
                assert_eq!(invocation.outcome, Some(vtcode_core::exec::events::ToolOutcome::Cancelled));
                invocation_closed = true;
            }
            ThreadItemDetails::ToolOutput(output) => {
                assert_eq!(completed.item.id, "turn-step-1-assistant-stream-1-tool-call-call_dangling:output");
                assert_eq!(output.tool_call_id.as_deref(), Some("call_dangling"));
                assert_eq!(output.status, ToolCallStatus::Failed);
                assert!(output.output.contains("ended when the turn finished"));
                output_closed = true;
            }
            _ => {}
        }
    }
    assert!(invocation_closed, "dangling invocation must be closed with Failed status");
    assert!(output_closed, "dangling invocation must get a terminal tool_output");
}

#[tokio::test]
async fn completed_and_interrupted_turns_close_unexecuted_calls_without_error_outcomes() {
    use crate::agent::runloop::unified::run_loop_context::StreamedToolCallItem;
    for result in [
        TurnLoopResult::Completed { plan_approved_execution_pending: false },
        TurnLoopResult::Cancelled,
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let path = backing.enable_harness_emitter();
        let mut ctx = backing.turn_loop_context();
        ctx.harness_state.remember_streamed_tool_call_items([(
            "pending-call".into(),
            StreamedToolCallItem {
                item_id: "pending-item".into(),
                tool_name: "exec_command".into(),
            },
        )]);
        finalize_turn(&mut ctx, &[], &result, &HarnessUsage::default()).await;
        assert!(ctx.harness_state.take_all_streamed_tool_call_item_ids().is_empty());
        let records = fs::read_to_string(path).unwrap();
        let events = records
            .lines()
            .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event())
            .collect::<Vec<_>>();
        let mut invocation_count = 0;
        let mut output_count = 0;
        for event in events {
            if let ThreadEvent::ItemCompleted(item) = event {
                match item.item.details {
                    ThreadItemDetails::ToolInvocation(invocation) => {
                        assert_eq!(invocation.tool_call_id.as_deref(), Some("pending-call"));
                        assert_eq!(invocation.outcome, Some(vtcode_core::exec::events::ToolOutcome::Cancelled));
                        invocation_count += 1;
                    }
                    ThreadItemDetails::ToolOutput(output) => {
                        assert_eq!(output.exit_code, None);
                        assert!(output.output.contains("before it could execute"));
                        output_count += 1;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!((invocation_count, output_count), (1, 1));
    }
}
