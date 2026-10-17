use super::*;

#[tokio::test]
async fn legacy_compaction_uses_local_for_responses_only_capability() {
    let config = CompactionConfig {
        keep_last_messages: 0,
        ..CompactionConfig::default()
    };

    // Responses capability alone is not enough for this legacy entry point:
    // the provider's `compact_history` method is only valid for standalone
    // compaction endpoints. The universal local path must remain usable.
    let compacted = compact_history(&CompatibleEndpointProvider, "stub-model", &sample_history(), &config)
        .await
        .expect("local compaction should handle responses-only providers");

    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
}

#[tokio::test]
async fn compact_history_preserves_continuity_tail_over_retention_budget() {
    let history = vec![
        Message::user("alpha beta gamma delta epsilon zeta".to_string()),
        Message::assistant("ack".to_string()),
        Message::user("newest request".to_string()),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        retained_user_message_tokens: 8,
        ..CompactionConfig::default()
    };

    let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
        .await
        .expect("compacted history");

    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[1].content.as_text(), "alpha beta gamma delta epsilon zeta");
    assert_eq!(compacted[2].content.as_text(), "ack");
    assert_eq!(compacted[3].content.as_text(), "newest request");
}

#[tokio::test]
async fn compacted_history_respects_model_context_budget() {
    let mut history = Vec::new();
    for index in 0..24 {
        history.push(Message::user(format!("request-{index} {}", "context ".repeat(1_200))));
        history.push(Message::assistant(format!("completed request {index}")));
    }
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };

    let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
        .await
        .expect("compacted history");
    let estimated_tokens = compacted.iter().map(Message::estimate_tokens).sum::<usize>();

    assert!(estimated_tokens <= 32_768 - 512);
}

#[tokio::test]
async fn compact_history_caps_retained_user_message_count() {
    let history = vec![
        Message::user("first request".to_string()),
        Message::assistant("ack".to_string()),
        Message::user("second request".to_string()),
        Message::assistant("ack".to_string()),
        Message::user("third request".to_string()),
        Message::assistant("ack".to_string()),
        Message::user("fourth request".to_string()),
        Message::assistant("ack".to_string()),
        Message::user("fifth request".to_string()),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        retained_user_messages: 4,
        ..CompactionConfig::default()
    };

    let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
        .await
        .expect("compacted history");

    let continuity_tail = compacted
        .iter()
        .skip(1)
        .map(|message| message.content.as_text().to_string())
        .collect::<Vec<_>>();
    assert_eq!(continuity_tail.len(), history.len());
    assert_eq!(continuity_tail[0], "first request");
    assert_eq!(continuity_tail[8], "fifth request");
}

#[tokio::test]
async fn compact_history_forces_local_summary_when_always_summarize_is_enabled() {
    let history = vec![
        Message::user("first request".to_string()),
        Message::assistant("working".to_string()),
        Message::user("second request".to_string()),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };

    let compacted = compact_history(&NativeCompactionProvider, "stub-model", &history, &config)
        .await
        .expect("compacted history");

    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
    assert_eq!(compacted[1].content.as_text(), "first request");
    assert_eq!(compacted[2].content.as_text(), "working");
    assert_eq!(compacted[3].content.as_text(), "second request");
}

#[test]
fn default_summary_prompt_preserves_required_compaction_context() {
    let prompt = CompactionConfig::default().summary_prompt;

    assert!(prompt.contains("acceptance criteria"));
    assert!(prompt.contains("file paths that were read or modified"));
    assert!(prompt.contains("test results and error messages"));
    assert!(prompt.contains("decisions with their reasoning"));
}

#[test]
fn continuity_tail_keeps_complete_turn_but_drops_unmatched_tool_call() {
    // A completed turn: user -> assistant(tool call) -> tool result. The
    // tail must keep the whole turn intact (the tool result makes the
    // trailing assistant tool call valid to send).
    let complete = vec![
        Message::user("do the thing".into()),
        {
            let mut m = Message::assistant("calling".into());
            m.tool_calls = Some(vec![ToolCall::function("c1".into(), "run".into(), "{}".into())]);
            m
        },
        Message::tool_response("c1".into(), "ran".into()),
    ];
    assert_eq!(continuity_tail(&complete).len(), 3);

    // An interrupted turn: user -> assistant(tool call) with no tool
    // result. Sending the trailing assistant message to a provider is
    // invalid, so the tail must drop it and keep only the user message.
    let interrupted = vec![Message::user("do the thing".into()), {
        let mut m = Message::assistant("calling".into());
        m.tool_calls = Some(vec![ToolCall::function("c1".into(), "run".into(), "{}".into())]);
        m
    }];
    let tail = continuity_tail(&interrupted);
    assert_eq!(tail.len(), 1);
    assert_eq!(tail[0].role, MessageRole::User);

    let parallel_complete = vec![
        Message::user("run both".into()),
        Message::assistant_with_tools(
            "calling".into(),
            vec![
                ToolCall::function("c1".into(), "run".into(), "{}".into()),
                ToolCall::function("c2".into(), "run".into(), "{}".into()),
            ],
        ),
        Message::tool_response("c1".into(), "first result".into()),
        Message::tool_response("c2".into(), "second result".into()),
    ];
    assert_eq!(continuity_tail(&parallel_complete).len(), 4);

    let parallel_interrupted = parallel_complete[..3].to_vec();
    let tail = continuity_tail(&parallel_interrupted);
    assert_eq!(tail.len(), 1, "a missing parallel result drops the whole assistant call");
    assert_eq!(tail[0].content.as_text(), "run both");
}

#[test]
fn continuity_tail_keeps_newest_complete_groups_within_budget() {
    let mut history = Vec::new();
    for group_index in 0..12 {
        history.push(Message::user(format!("group-{group_index} {}", "alpha beta gamma delta ".repeat(2_000))));
        history.push(Message::assistant(format!("completed group {group_index}")));
    }

    let tail = continuity_tail(&history);
    let estimated_tokens = tail.iter().map(Message::estimate_tokens).sum::<usize>();

    assert!(estimated_tokens <= CONTINUITY_TAIL_TARGET_TOKENS);
    assert_eq!(tail.first().map(|message| message.role), Some(MessageRole::User));
    assert!(tail.iter().any(|message| message.content.as_text().contains("group-11")));
    assert!(!tail.iter().any(|message| message.content.as_text().contains("group-0 ")));
    assert_eq!(tail.len() % 2, 0, "protocol groups must remain atomic");
}

#[test]
fn continuity_tail_bounds_an_oversized_newest_group() {
    let history = vec![
        Message::user("u".repeat(100_000)),
        Message::assistant("a".repeat(100_000)),
    ];

    let tail = continuity_tail(&history);

    assert_eq!(tail.len(), 2);
    assert!(tail.iter().map(Message::estimate_tokens).sum::<usize>() <= CONTINUITY_TAIL_TARGET_TOKENS);
}

#[test]
fn continuity_tail_bounds_tool_call_metadata_without_breaking_correlation() {
    let mut assistant = Message::assistant(String::new());
    assistant.tool_calls = Some(vec![ToolCall::function(
        "call-large".into(),
        "run_command".into(),
        format!("{{\"command\":\"{}\"}}", "x".repeat(300_000)),
    )]);
    let history = vec![
        Message::user("run the command".into()),
        assistant,
        Message::tool_response("call-large".into(), "completed".into()),
    ];

    let tail = continuity_tail(&history);
    assert!(tail.iter().map(Message::estimate_tokens).sum::<usize>() <= CONTINUITY_TAIL_TARGET_TOKENS);
    let call = tail[1].tool_calls.as_ref().expect("tool call should remain").first().unwrap();
    assert_eq!(call.id, "call-large");
    assert_eq!(call.function.as_ref().unwrap().arguments, "{}");
    assert_eq!(tail[2].tool_call_id.as_deref(), Some("call-large"));
}

/// History-growth verify-item: the local summarization prompt must exclude
/// `Message.reasoning` / `reasoning_details` and serialize only the visible
/// text content (`content.as_text()`). Reasoning traces are large and
/// ephemeral; including them in every compaction summary would bloat the
/// post-compaction context and re-inject stale chain-of-thought. The
/// continuity tail preserves provider-protocol reasoning (Anthropic
/// thinking signatures, OpenAI reasoning items) separately and is NOT
/// summarized, so stripping reasoning here is safe and correct. This test
/// pins that invariant so a change to `build_summary_prompt` cannot
/// accidentally start including reasoning.
#[test]
fn build_summary_prompt_excludes_reasoning_traces() {
    use super::build_summary_prompt;

    let instructions = "Summarize the conversation.";
    // Assistant message carrying a reasoning trace alongside its visible
    // content. If `build_summary_prompt` ever reads `reasoning`, the
    // summary would contain "SECRET_REASONING" and "raw-only reasoning".
    let history = vec![
        Message::user("What is 2+2?".to_string()),
        Message::assistant("The answer is 4.".to_string())
            .with_reasoning(Some("SECRET_REASONING: I computed 2+2=4.".to_string())),
    ];

    let prompt = build_summary_prompt(&history, instructions);

    assert!(prompt.contains("The answer is 4."), "visible assistant text must appear in the summary prompt");
    assert!(
        !prompt.contains("SECRET_REASONING"),
        "Message.reasoning must NOT be included in the summary prompt -- \
         it would bloat every compaction pass with ephemeral chain-of-thought"
    );
    assert!(prompt.contains("Summarize the conversation."), "instructions must appear in the summary prompt");
}
