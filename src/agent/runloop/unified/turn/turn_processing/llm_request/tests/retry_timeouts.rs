use super::*;

#[tokio::test]
async fn unmatched_tool_result_noop_repair_fails_closed_without_repeating_the_request() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "merge_gateway",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Provider {
            message: "Merge Gateway error (400 Bad Request): unmatched tool result for call_1".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "result".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err());
    assert_eq!(recorded_previous_response_ids.lock().expect("provider calls").len(), 1);
}
#[tokio::test]
async fn duplicate_tool_response_stops_each_user_attempt_without_automatic_retries() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let error = || {
        ScriptedProviderOutcome::Error(uni::LLMError::Provider {
            message: "Duplicate tool response for tool_call_id='harness-auto-verify'".to_string(),
            metadata: None,
        })
    };
    let requests = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "meta",
        false,
        Arc::clone(&calls),
        vec![
            error(),
            error(),
            ScriptedProviderOutcome::Success { content: Some("recovered"), request_id: None },
        ],
    )
    .with_recorded_messages(Arc::clone(&requests));
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    for result in ["running", "exit 0"] {
        ctx.working_history.push(uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "harness-auto-verify".into(),
                "write_stdin".into(),
                "{}".into(),
            )],
        ));
        ctx.working_history
            .push(uni::Message::tool_response("harness-auto-verify".into(), result.into()));
    }
    let original = ctx.working_history.clone();
    assert!(
        execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
            .await
            .is_err()
    );
    assert_eq!(calls.lock().expect("provider calls").len(), 1);
    assert!(
        execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
            .await
            .is_err()
    );
    assert_eq!(calls.lock().expect("provider calls").len(), 2);
    let recovered = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None)
        .await
        .unwrap();
    assert_eq!(recovered.0.content.as_deref(), Some("recovered"));
    assert_eq!(calls.lock().expect("provider calls").len(), 3);
    assert_eq!(&ctx.working_history[..original.len()], original.as_slice());
    let requests = requests.lock().expect("request recorder");
    assert_eq!(requests.len(), 3);
    for request in requests.iter() {
        let calls: Vec<_> = request
            .iter()
            .filter_map(|message| message.tool_calls.as_ref())
            .flatten()
            .collect();
        let results: Vec<_> = request
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .collect();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0].id, calls[1].id);
        assert_eq!(results.len(), 2);
        for (index, expected) in ["running", "exit 0"].into_iter().enumerate() {
            assert_eq!(results[index].tool_call_id.as_deref(), Some(calls[index].id.as_str()));
            assert_eq!(results[index].content.as_text(), expected);
        }
    }
}

#[tokio::test]
async fn misconfiguration_after_tool_result_skips_post_tool_retry() {
    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "any-provider",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Error(uni::LLMError::Network {
            message: "network error: invalid endpoint in base_url".to_string(),
            metadata: None,
        })],
    );

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "result".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_err());
    assert_eq!(recorded_previous_response_ids.lock().expect("provider calls").len(), 1);
}
#[test]
fn retryable_llm_error_excludes_usage_limit_messages() {
    assert!(!is_retryable_llm_error("Provider error: you have reached your weekly usage limit"));
}
#[tokio::test]
async fn streaming_timeout_falls_back_to_non_streaming_for_supported_models() {
    use vtcode_core::utils::transcript;

    transcript::clear();

    let recorded_previous_response_ids = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider::new(
        "any-provider",
        false,
        Arc::clone(&recorded_previous_response_ids),
        vec![ScriptedProviderOutcome::Success { content: Some("recovered"), request_id: None }],
    )
    .with_streaming(true);

    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    *ctx.provider_client = Box::new(provider);
    ctx.working_history.push(uni::Message::user("continue".to_string()));

    let result = execute_llm_request(&mut ctx, 1, "noop-model", Some(320), false, None).await;

    assert!(result.is_ok());
    assert_eq!(result.expect("non-streaming recovery").0.content.as_deref(), Some("recovered"));

    transcript::clear();
}
#[test]
fn post_tool_retry_uses_non_streaming_before_compaction_when_supported() {
    assert_eq!(
        next_post_tool_retry_action(true, true, false, false),
        Some(PostToolRetryAction::SwitchToNonStreaming)
    );
}
#[test]
fn post_tool_retry_skips_non_streaming_when_unsupported() {
    assert_eq!(
        next_post_tool_retry_action(true, false, false, false),
        Some(PostToolRetryAction::CompactToolContext)
    );
}
#[test]
fn post_tool_retry_preserves_structured_context_for_responses_chaining_providers() {
    assert_eq!(next_post_tool_retry_action(false, true, false, true), None);
}
#[test]
fn compact_tool_messages_for_retry_keeps_recent_tool_outputs_only() {
    let messages = vec![
        uni::Message::user("u1".to_string()),
        uni::Message::tool_response("call_1".to_string(), "old tool".to_string()),
        uni::Message::assistant("a1".to_string()),
        uni::Message::tool_response("call_2".to_string(), "new tool".to_string()),
    ];

    let compacted = compact_tool_messages_for_retry(&messages);
    assert_eq!(
        compacted
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .count(),
        2
    );
    assert_eq!(compacted.len(), 4);
}
#[test]
fn compact_tool_messages_for_retry_keeps_all_tool_call_ids() {
    let messages = vec![
        uni::Message::tool_response("call_1".to_string(), "first".to_string()),
        uni::Message::assistant("a1".to_string()),
        uni::Message::tool_response("call_2".to_string(), "second".to_string()),
        uni::Message::assistant("a2".to_string()),
        uni::Message::tool_response("call_3".to_string(), "third".to_string()),
    ];

    let compacted = compact_tool_messages_for_retry(&messages);
    let tool_ids = compacted
        .iter()
        .filter(|message| message.role == uni::MessageRole::Tool)
        .filter_map(|message| message.tool_call_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(tool_ids, vec!["call_1", "call_2", "call_3"]);
}
#[test]
fn llm_retry_attempts_uses_default_when_unset() {
    assert_eq!(llm_retry_attempts(None), DEFAULT_LLM_RETRY_ATTEMPTS);
}
#[test]
fn llm_retry_attempts_uses_configured_retries_plus_initial_attempt() {
    assert_eq!(llm_retry_attempts(Some(2)), 3);
}
#[test]
fn llm_retry_attempts_respects_upper_bound() {
    assert_eq!(llm_retry_attempts(Some(16)), MAX_LLM_RETRY_ATTEMPTS);
}
#[test]
fn stream_timeout_error_detection_matches_common_messages() {
    assert!(is_stream_timeout_error("Stream request timed out after 75s"));
    assert!(is_stream_timeout_error("Streaming request timed out after configured timeout"));
    assert!(is_stream_timeout_error("LLM first token timed out after 120 seconds"));
}
#[test]
fn llm_first_progress_timeout_defaults_to_fifth_of_turn_budget() {
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "openai"), 60);
    assert_eq!(llm_first_progress_timeout_secs(300, false, false, "openai"), 60);
}
#[test]
fn llm_first_progress_timeout_merge_gateway_floors_to_gateway_silence() {
    // Non-streaming merge-gateway turns must not abandon before Gateway's
    // 120s first-frame silence window (double-bill risk).
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "merge-gateway"), 120);
    // Without non-streaming fallback there is no double-bill retry path.
    assert_eq!(llm_first_progress_timeout_secs(300, false, false, "merge-gateway"), 60);
    // Other providers keep the baseline.
    assert_eq!(llm_first_progress_timeout_secs(300, false, true, "anthropic"), 60);
}
#[test]
fn llm_first_progress_timeout_planning_workflow_uses_capability_floor() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "merge-gateway"), 120);
}
#[test]
fn llm_first_progress_timeout_planning_openai_uses_capability_floor() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "openai"), 90);
    assert_eq!(llm_first_progress_timeout_secs(150, true, false, "openai"), 75);
}
#[test]
fn llm_first_progress_timeout_expands_for_planning_workflow() {
    assert_eq!(llm_first_progress_timeout_secs(300, true, true, "openai"), 150);
}
#[test]
fn llm_first_progress_timeout_planning_workflow_respects_smaller_turn_budget() {
    assert_eq!(llm_first_progress_timeout_secs(180, true, true, "openai"), 90);
    // merge-gateway planning floors at documented 120s even on small budgets.
    assert_eq!(llm_first_progress_timeout_secs(180, true, true, "merge-gateway"), 120);
}
#[test]
fn llm_first_progress_timeout_planning_workflow_cap_remains_180() {
    assert_eq!(llm_first_progress_timeout_secs(1_200, true, true, "merge-gateway"), 180);
}
#[test]
fn llm_first_progress_timeout_planning_workflow_uses_capability_floor_for_openai() {
    assert_eq!(llm_first_progress_timeout_secs(150, true, true, "openai"), 90);
    assert_eq!(llm_first_progress_timeout_secs(150, true, false, "openai"), 75);
}
#[test]
fn llm_first_progress_timeout_respects_planning_workflow_cap() {
    assert_eq!(llm_first_progress_timeout_secs(1_200, true, true, "openai"), 180);
}
