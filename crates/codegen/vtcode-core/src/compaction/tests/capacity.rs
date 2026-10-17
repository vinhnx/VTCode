use super::*;

#[tokio::test]
async fn capacity_retry_halves_progressively_until_recovery() {
    use super::generate_summary_with_capacity_retry;

    // Each turn is tens of thousands of tokens against a 30k budget: the
    // first fork keeps one turn, then two halved retries shrink strictly
    // before the third attempt succeeds.
    let history = vec![
        Message::user("a ".repeat(20_000)),
        Message::user("b ".repeat(20_000)),
        Message::user("c ".repeat(20_000)),
    ];
    let provider = CapacityFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
        failures: 2,
    };
    let summary = generate_summary_with_capacity_retry(
        &provider,
        "stub-model",
        &history,
        "test instructions",
        &history,
        Some(30_000),
        2,
        "Failed to generate compaction summary",
        |source| compaction_summary_request("stub-model", source, "test instructions", None, None, None, true, None),
    )
    .await
    .expect("two retries must recover the summary");
    assert_eq!(summary, "retried summary");
    assert_eq!(*provider.attempts.lock().unwrap(), 3, "two failures then success");
    let tokens = provider.request_tokens.lock().unwrap().clone();
    assert_eq!(tokens.len(), 3);
    assert!(tokens[0] > tokens[1] && tokens[1] > tokens[2], "each retry must shrink strictly, got {tokens:?}");
}

#[tokio::test]
async fn capacity_retry_recovers_with_unknown_budget_fallback() {
    use super::generate_summary_with_capacity_retry;

    // Unknown window (`None` budget) sends the first fork verbatim. A
    // capacity rejection must still shrink via a fallback derived from
    // the failing attempt instead of resending the identical fork.
    // Asymmetric vs the verbatim/no-retry case: oldest vs newest text
    // differ so trimming the oldest is observable.
    let history = vec![
        Message::user(format!("oldest {}", "old ".repeat(20_000))),
        Message::user(format!("newest {}", "new ".repeat(20_000))),
    ];
    let provider = UnknownBudgetFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
    };
    let summary = generate_summary_with_capacity_retry(
        &provider,
        "stub-model",
        &history,
        "test instructions",
        &history,
        None,
        1,
        "Failed to generate compaction summary",
        |source| compaction_summary_request("stub-model", source, "test instructions", None, None, None, true, None),
    )
    .await
    .expect("unknown-budget capacity error must recover via fallback");
    assert_eq!(summary, "retried summary");
    assert_eq!(*provider.attempts.lock().unwrap(), 2, "one failure then success");
    let tokens = provider.request_tokens.lock().unwrap().clone();
    assert_eq!(tokens.len(), 2);
    assert!(tokens[1] < tokens[0], "fallback retry must shrink, got {tokens:?}");
}

#[tokio::test]
async fn empty_summary_fails_with_diagnostic() {
    use super::generate_summary_with_capacity_retry;

    let history = sample_history();
    let provider = EmptySummaryProvider;
    let error = generate_summary_with_capacity_retry(
        &provider,
        "stub-model",
        &history,
        "test instructions",
        &history,
        Some(30_000),
        1,
        "Failed to generate compaction summary",
        |source| compaction_summary_request("stub-model", source, "test instructions", None, None, None, true, None),
    )
    .await
    .expect_err("empty provider summary must fail");
    let message = format!("{error:#}");
    assert!(message.contains("Failed to generate compaction summary"), "outer context preserved: {message}");
    assert!(message.contains("empty summary"), "empty cause surfaced: {message}");
    assert!(message.contains("empty-summary"), "provider identity surfaced: {message}");
}

#[tokio::test]
async fn capacity_error_context_carries_route_diagnostics() {
    // Tiny fitting history with a capacity failure cannot shrink, so the
    // original failure propagates. The propagated error must carry the
    // route diagnostics needed to debug `/compact` without guessing.
    let history = sample_history();
    let provider = CapacityFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
        failures: 1,
    };
    let error = compact_history_manual(
        &provider,
        "stub-model",
        &history,
        &CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        },
        &ManualCompactionOptions::default(),
    )
    .await
    .expect_err("identical retry must not be attempted");
    let message = format!("{error:#}");
    assert!(message.contains("Failed to generate compaction summary"), "outer context: {message}");
    assert!(message.contains("capacity-fail-once"), "provider name: {message}");
    assert!(message.contains("stub-model"), "model name: {message}");
    assert!(message.contains("input_tokens="), "token counts: {message}");
}

#[tokio::test]
async fn compact_history_manual_falls_back_to_local_when_inline_compaction_not_fired() {
    let history = sample_history();
    let config = CompactionConfig::default();

    // NativeCompactionProvider is inline-capable but its `generate` returns a
    // normal `Stop` with no compaction block, so the inline attempt cannot
    // fire and the dispatch must transparently fall back to Local.
    let (compacted, mode) = compact_history_manual(
        &NativeCompactionProvider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
    )
    .await
    .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
}

#[tokio::test]
async fn compact_history_manual_falls_back_to_local_when_inline_request_errors() {
    let history = sample_history();
    let config = CompactionConfig::default();

    // A provider dispatched to NativeInline that rejects the Anthropic
    // `compact_20260112` edit must not abort the whole command; the dispatch
    // falls back to Local summarization (the manual `/compact` contract:
    // always succeeds).
    let (compacted, mode) = compact_history_manual(
        &InlineRejectingProvider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
    )
    .await
    .expect("manual compaction should fall back to local");

    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
}

#[tokio::test]
async fn compact_history_manual_applies_options_to_local_summary_request() {
    let history = sample_history();
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };
    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let options = ManualCompactionOptions {
        instructions: Some("KEEP DECISIONS ONLY".to_string()),
        max_output_tokens: Some(128),
        reasoning_effort: Some(ReasoningEffortLevel::Minimal),
        verbosity: Some(VerbosityLevel::High),
        ..ManualCompactionOptions::default()
    };

    let (compacted, mode) = compact_history_manual(&provider, "stub-model", &history, &config, &options)
        .await
        .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Local);
    let captured = provider.last_request.lock().unwrap().clone().expect("captured summary request");
    assert_eq!(captured.max_tokens, Some(128));
    assert_eq!(captured.reasoning_effort, Some(ReasoningEffortLevel::Minimal));
    assert_eq!(captured.verbosity, Some(VerbosityLevel::High));
    // Cache-safe forking: the parent history prefix is reused verbatim and
    // the custom instructions are appended as the only new turn.
    assert_eq!(captured.messages.len(), history.len() + 1);
    for (sent, original) in captured.messages.iter().zip(history.iter()) {
        assert_eq!(sent.content.as_text(), original.content.as_text());
    }
    let prompt = captured.messages.last().expect("compaction prompt").content.as_text();
    assert!(prompt.contains("KEEP DECISIONS ONLY"));
    assert!(!prompt.contains("acceptance criteria"));
    // The fork must not invite tool calls: same tools may be present for
    // prefix reuse, but the choice disables invocation.
    assert!(matches!(captured.tool_choice, Some(crate::llm::provider::ToolChoice::None)));
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
}

#[tokio::test]
async fn compact_history_manual_blocks_unsupported_reasoning_before_summary() {
    let provider = LimitedReasoningProvider { last_request: Mutex::new(None) };
    let options = ManualCompactionOptions {
        reasoning_effort: Some(ReasoningEffortLevel::Max),
        ..ManualCompactionOptions::default()
    };
    let error = compact_history_manual(
        &provider,
        "stub-model",
        &sample_history(),
        &CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        },
        &options,
    )
    .await
    .expect_err("unsupported compaction effort must block");

    assert!(
        error
            .downcast_ref::<crate::llm::reasoning_effort::ReasoningEffortUnsupported>()
            .is_some(),
        "strict compaction failure should preserve the capability diagnostic: {error:#}"
    );
    assert!(provider.last_request.lock().unwrap().is_none(), "provider must not receive a blocked request");
}

#[tokio::test]
async fn compact_history_manual_downgrades_only_when_explicitly_enabled() {
    let provider = LimitedReasoningProvider { last_request: Mutex::new(None) };
    let options = ManualCompactionOptions {
        reasoning_effort: Some(ReasoningEffortLevel::Max),
        allow_reasoning_effort_downgrade: true,
        ..ManualCompactionOptions::default()
    };
    let config = CompactionConfig {
        always_summarize: true,
        hierarchical: true,
        ..CompactionConfig::default()
    };
    compact_history_manual(&provider, "stub-model", &sample_history(), &config, &options)
        .await
        .expect("explicit downgrade should permit compaction");

    let captured = provider.last_request.lock().unwrap().clone().expect("summary request captured");
    assert_eq!(captured.reasoning_effort, Some(ReasoningEffortLevel::High));
}

#[tokio::test]
async fn compact_history_manual_returns_empty_for_empty_history() {
    let (compacted, mode) = compact_history_manual(
        &StubProvider,
        "stub-model",
        &[],
        &CompactionConfig::default(),
        &ManualCompactionOptions::default(),
    )
    .await
    .expect("manual compaction");

    assert!(compacted.is_empty());
    assert_eq!(mode, CompactionMode::Local);
}

#[tokio::test]
async fn compact_history_rebuilds_history_around_summary_and_important_messages() {
    let history = vec![
        Message::assistant("setup".to_string()),
        Message::user("first request".to_string()),
        Message::assistant("working".to_string()),
        Message::tool_response("call-1".to_string(), "done".to_string()),
        Message::user("second request".to_string()),
        Message::assistant("final reply".to_string()),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };

    let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
        .await
        .expect("compacted history");

    // Summary plus the complete newest protocol groups. The assistant/tool
    // messages remain paired with their user anchors in the continuity
    // tail.
    assert_eq!(compacted.len(), 6);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
    assert_eq!(compacted[1].content.as_text(), "first request");
    assert_eq!(compacted[2].content.as_text(), "working");
    assert_eq!(compacted[3].content.as_text(), "done");
    assert_eq!(compacted[4].content.as_text(), "second request");
    assert_eq!(compacted[5].content.as_text(), "final reply");
}
