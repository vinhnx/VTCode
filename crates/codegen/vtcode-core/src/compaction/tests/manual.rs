use super::*;

#[tokio::test]
async fn compact_history_manual_uses_local_summary_for_plain_provider() {
    let history = sample_history();
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };

    let (compacted, mode) =
        compact_history_manual(&StubProvider, "stub-model", &history, &config, &ManualCompactionOptions::default())
            .await
            .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
    assert_eq!(compacted[1].content.as_text(), "first request");
    assert_eq!(compacted[2].content.as_text(), "working");
    assert_eq!(compacted[3].content.as_text(), "second request");
}

#[tokio::test]
async fn local_compaction_repairs_missing_tool_results_in_summary_request() {
    let history = vec![
        Message::user("Inspect the README".to_string()),
        Message::assistant_with_tools(
            "Run two reads".to_string(),
            vec![
                ToolCall::function("call_read_a".to_string(), "exec_command".to_string(), "{}".to_string()),
                ToolCall::function("call_read_b".to_string(), "exec_command".to_string(), "{}".to_string()),
            ],
        ),
        Message::user("Continue the documentation task".to_string()),
    ];
    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };

    let (_compacted, mode) =
        compact_history_manual(&provider, "stub-model", &history, &config, &ManualCompactionOptions::default())
            .await
            .expect("manual compaction should repair its summary request");

    assert_eq!(mode, CompactionMode::Local);
    let request = provider.last_request.lock().unwrap().clone().expect("captured summary request");
    let messages = request.messages.as_ref();
    assert!(
        !crate::core::agent::state::request_history_needs_normalization(messages),
        "summary request must contain a complete tool-call/result batch"
    );
    assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_read_a"));
    assert_eq!(messages[3].tool_call_id.as_deref(), Some("call_read_b"));
    assert_eq!(messages[4], history[2], "the next user message follows the repaired batch");
}

#[tokio::test]
async fn local_compaction_collects_summary_from_streaming_only_provider() {
    let provider = StreamingOnlyCompactionProvider {
        generate_calls: Mutex::new(0),
        stream_calls: Mutex::new(0),
        stream_modes: Mutex::new(Vec::new()),
    };

    let (compacted, mode) = compact_history_manual(
        &provider,
        "stub-model",
        &sample_history(),
        &CompactionConfig::default(),
        &ManualCompactionOptions::default(),
    )
    .await
    .expect("streaming-only local compaction should succeed");

    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nstreamed summary");
    assert_eq!(*provider.generate_calls.lock().unwrap(), 0, "non-streaming generation must not be attempted");
    assert_eq!(*provider.stream_calls.lock().unwrap(), 1, "summary should be collected from one normalized stream");
    assert_eq!(*provider.stream_modes.lock().unwrap(), vec![true], "stream fallback must set the request mode");
}

#[tokio::test]
async fn compact_history_manual_preserves_native_standalone_window() {
    let history = sample_history();
    let config = CompactionConfig::default();
    let provider = ManualStandaloneProvider {
        last_options: Mutex::new(None),
        output: canonical_standalone_output(),
    };

    let (compacted, mode) =
        compact_history_manual(&provider, "stub-model", &history, &config, &ManualCompactionOptions::default())
            .await
            .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Provider);
    assert_eq!(compacted, canonical_standalone_output());
}

#[tokio::test]
async fn native_compaction_preserves_canonical_window_over_session_budget() {
    let history = sample_history();
    let canonical = vec![
        Message::user("provider-retained ".repeat(20_000)),
        Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
            "type": "compaction",
            "id": "cmp_canonical",
            "encrypted_content": "opaque_state",
        })])),
    ];
    let provider = ManualStandaloneProvider {
        last_options: Mutex::new(None),
        output: canonical.clone(),
    };
    let (compacted, mode) = compact_history_manual_with_budget(
        &provider,
        "stub-model",
        &history,
        &CompactionConfig::default(),
        &ManualCompactionOptions::default(),
        Some(8_192),
    )
    .await
    .expect("native compaction");

    assert_eq!(mode, CompactionMode::Provider);
    assert_eq!(compacted, canonical, "standalone output is the canonical replay window");
    assert!(
        compacted.iter().map(Message::estimate_tokens).sum::<usize>() > 8_192,
        "fixture must prove that the local session bound was not applied"
    );
}

#[tokio::test]
async fn compact_history_manual_passes_options_to_native_standalone() {
    let history = sample_history();
    let config = CompactionConfig::default();
    let provider = ManualStandaloneProvider {
        last_options: Mutex::new(None),
        output: canonical_standalone_output(),
    };
    let options = ManualCompactionOptions {
        instructions: Some("keep only decisions".to_string()),
        max_output_tokens: Some(256),
        reasoning_effort: Some(ReasoningEffortLevel::Minimal),
        verbosity: Some(VerbosityLevel::High),
        ..ManualCompactionOptions::default()
    };

    let (_compacted, mode) = compact_history_manual(&provider, "stub-model", &history, &config, &options)
        .await
        .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Provider);
    let captured = provider.last_options.lock().unwrap().clone().expect("captured options");
    assert_eq!(captured.instructions.as_deref(), Some("keep only decisions"));
    assert_eq!(captured.max_output_tokens, Some(256));
    assert_eq!(captured.reasoning_effort, Some(ReasoningEffortLevel::Minimal));
    assert_eq!(captured.verbosity, Some(VerbosityLevel::High));
}

#[tokio::test]
async fn compact_history_manual_uses_native_inline_when_pause_and_compaction_present() {
    let history = sample_history();
    let config = CompactionConfig::default();
    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: true,
    };

    let (compacted, mode) =
        compact_history_manual(&provider, "stub-model", &history, &config, &ManualCompactionOptions::default())
            .await
            .expect("manual compaction");

    assert_eq!(mode, CompactionMode::Provider);
    assert_eq!(compacted.len(), 4);
    assert_eq!(compacted[0].role, MessageRole::Assistant);
    assert!(compacted[0].content.as_text().is_empty());
    let detail = compacted[0]
        .reasoning_details
        .as_ref()
        .and_then(|details| details.first())
        .expect("provider compaction detail");
    let detail: serde_json::Value = serde_json::from_value(detail.clone()).expect("provider detail");
    assert_eq!(detail["signature"], "provider-signature");
    assert_eq!(detail["opaque_extension"], "preserve-me");
    assert_eq!(compacted[1].content.as_text(), "first request");
    assert_eq!(compacted[2].content.as_text(), "working");
    assert_eq!(compacted[3].content.as_text(), "second request");

    // The inline request must carry the `compact_20260112` edit with a forced
    // pause so the provider actually performs compaction on demand.
    let captured = provider.last_request.lock().unwrap().clone().expect("captured inline request");
    let context_management = captured
        .context_management
        .as_ref()
        .expect("context_management set on inline compaction request");
    let edit = &context_management["edits"][0];
    assert_eq!(edit["type"].as_str(), Some("compact_20260112"));
    assert_eq!(edit["pause_after_compaction"].as_bool(), Some(true));
    assert_eq!(edit["trigger"]["value"].as_u64(), Some(50_000));
}

#[tokio::test]
async fn inline_summary_without_opaque_detail_falls_back_to_local_mode() {
    let history = sample_history();
    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: false,
    };

    let (compacted, mode) = compact_history_manual_with_budget(
        &provider,
        "stub-model",
        &history,
        &CompactionConfig::default(),
        &ManualCompactionOptions::default(),
        Some(8_192),
    )
    .await
    .expect("manual compaction should fall back to local mode");

    assert_eq!(mode, CompactionMode::Local);
    assert_eq!(compacted[0].role, MessageRole::System);
    assert!(compacted[0].content.as_text().starts_with("Previous conversation summary:"));
}

#[tokio::test]
async fn compact_history_manual_inline_request_carries_instructions_when_provided() {
    let history = sample_history();
    let config = CompactionConfig::default();
    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: true,
    };
    let options = ManualCompactionOptions {
        instructions: Some("  keep only decisions  ".to_string()),
        ..ManualCompactionOptions::default()
    };

    let (_compacted, _mode) = compact_history_manual(&provider, "stub-model", &history, &config, &options)
        .await
        .expect("manual compaction");

    let captured = provider.last_request.lock().unwrap().clone().expect("captured inline request");
    let edit = &captured.context_management.as_ref().expect("context_management")["edits"][0];
    assert_eq!(edit["instructions"].as_str(), Some("keep only decisions"));
}

#[tokio::test]
async fn native_inline_fork_reuses_parent_prefix_when_supplied() {
    use super::{CompactionParentContext, compact_history_manual_with_parent_context};

    let history = sample_history();
    let config = CompactionConfig::default();
    let options = ManualCompactionOptions::default();

    // Asymmetric arms on the same history: without a parent the inline
    // request carries no fork fields; with one it carries all three while
    // the compaction edit stays intact in both.
    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: true,
    };
    let (_compacted, mode) =
        compact_history_manual_with_parent_context(&provider, "stub-model", &history, &config, &options, None, None)
            .await
            .expect("manual compaction");
    assert_eq!(mode, CompactionMode::Provider);
    let bare = provider.last_request.lock().unwrap().clone().expect("captured inline request");
    assert!(bare.system_prompt.is_none());
    assert!(bare.tools.is_none());
    assert!(bare.context_management.is_some());

    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: true,
    };
    let parent = CompactionParentContext {
        system_prompt: Some(Arc::from("parent system")),
        tools: None,
    };
    let (_compacted, mode) = compact_history_manual_with_parent_context(
        &provider,
        "stub-model",
        &history,
        &config,
        &options,
        None,
        Some(&parent),
    )
    .await
    .expect("manual compaction");
    assert_eq!(mode, CompactionMode::Provider);
    let forked = provider.last_request.lock().unwrap().clone().expect("captured inline request");
    assert_eq!(forked.system_prompt.as_deref(), Some("parent system"));
    assert!(forked.tools.is_none(), "empty parent catalog must stay off the wire");
    assert!(matches!(forked.tool_choice, Some(crate::llm::provider::ToolChoice::None)));
    let edit = &forked.context_management.as_ref().expect("context_management")["edits"][0];
    assert_eq!(edit["type"].as_str(), Some("compact_20260112"));
    assert_eq!(edit["pause_after_compaction"].as_bool(), Some(true));
}
