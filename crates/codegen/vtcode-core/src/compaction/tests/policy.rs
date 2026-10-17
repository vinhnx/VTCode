use super::*;

#[test]
fn inline_compaction_edits_follow_the_documented_ladder_order() {
    use super::native_inline::anthropic_inline_compaction_edits;

    let edits = anthropic_inline_compaction_edits(Some("keep decisions"), true);
    let types: Vec<&str> = edits
        .iter()
        .filter_map(|edit| edit.get("type").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(
        types,
        vec![
            "clear_thinking_20251015",
            "clear_tool_uses_20250919",
            "compact_20260112"
        ],
        "thinking clears first, tool uses next, compaction last"
    );
    assert_eq!(edits[2]["pause_after_compaction"].as_bool(), Some(true));
    assert_eq!(edits[2]["instructions"].as_str(), Some("keep decisions"));

    let compact_only = anthropic_inline_compaction_edits(None, false);
    assert_eq!(compact_only.len(), 1);
    assert_eq!(compact_only[0]["type"].as_str(), Some("compact_20260112"));
}

#[tokio::test]
async fn native_inline_request_carries_ladder_when_provider_supports_context_edits() {
    use super::compact_history_manual_with_parent_context;

    let history = sample_history();
    let config = CompactionConfig::default();
    let provider = ContextEditsInlineProvider { last_request: Mutex::new(None) };
    let (_compacted, mode) = compact_history_manual_with_parent_context(
        &provider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
        None,
        None,
    )
    .await
    .expect("manual compaction");
    assert_eq!(mode, CompactionMode::Provider);
    let request = provider.last_request.lock().unwrap().clone().expect("captured inline request");
    let edits = request.context_management.as_ref().expect("context_management")["edits"]
        .as_array()
        .expect("edits array")
        .clone();
    let types: Vec<&str> = edits
        .iter()
        .filter_map(|edit| edit.get("type").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(
        types,
        vec![
            "clear_thinking_20251015",
            "clear_tool_uses_20250919",
            "compact_20260112"
        ]
    );
}

#[tokio::test]
async fn local_summary_retries_once_on_context_capacity_error() {
    use super::compact_history_manual;

    // Over-budget history so the halved retry is strictly smaller than
    // the first attempt (the retry is skipped when it cannot shrink).
    // Each turn is ~100k tokens against the stub's ~174k summarizer
    // budget: the first fork keeps one turn, the retry degrades to
    // previews, and the compacted output still fits the summary.
    let history = vec![
        Message::user("old ".repeat(100_000)),
        Message::user("middle ".repeat(100_000)),
        Message::user("newest ".repeat(100_000)),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };
    let provider = CapacityFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
        failures: 1,
    };
    let (compacted, mode) =
        compact_history_manual(&provider, "stub-model", &history, &config, &ManualCompactionOptions::default())
            .await
            .expect("retry must recover the summary");
    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(*provider.attempts.lock().unwrap(), 2, "exactly one retry");
    let tokens = provider.request_tokens.lock().unwrap().clone();
    assert_eq!(tokens.len(), 2);
    assert!(tokens[1] < tokens[0], "retry must shrink the fork");
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nretried summary");
}

#[tokio::test]
async fn local_summary_skips_retry_when_it_cannot_shrink() {
    use super::compact_history_manual;

    // Tiny history fits the budget verbatim, so a halved retry would
    // resend the identical fork. The capacity failure must propagate
    // without a wasted second call.
    let history = sample_history();
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };
    let provider = CapacityFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
        failures: 1,
    };
    let error = compact_history_manual(&provider, "stub-model", &history, &config, &ManualCompactionOptions::default())
        .await
        .expect_err("identical retry must not be attempted");
    assert_eq!(*provider.attempts.lock().unwrap(), 1, "no second call");
    assert!(error.to_string().contains("Failed to generate compaction summary"));
}

#[tokio::test]
async fn hierarchical_bands_retry_once_on_context_capacity_error() {
    use super::compact_history_manual_with_budget;

    // Small session budget forces over-budget bands; the failing abstract
    // pass must retry with a halved band and the detail pass must still
    // run, so exactly three generate calls happen.
    let mut history = Vec::new();
    for turn in ["first", "second", "third", "fourth", "fifth", "sixth"] {
        history.push(Message::user(format!("{turn} {}", "text ".repeat(1_500))));
    }
    history.push(Message::user(format!("latest {}", "big ".repeat(25_000))));
    let config = CompactionConfig {
        hierarchical: true,
        always_summarize: true,
        ..CompactionConfig::default()
    };
    let provider = CapacityFailOnceProvider {
        attempts: Mutex::new(0),
        request_tokens: Mutex::new(Vec::new()),
        failures: 1,
    };
    let (_compacted, mode) = compact_history_manual_with_budget(
        &provider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
        Some(3_000),
    )
    .await
    .expect("hierarchical bands must recover from a capacity error");
    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(*provider.attempts.lock().unwrap(), 3, "abstract retries once, detail runs once");
}
