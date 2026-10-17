use super::*;

#[tokio::test]
async fn local_summary_fork_omits_clear_at_for_gateway_routes() {
    use super::compact_history_manual_with_parent_context;

    let history = vec![
        Message::user("do the thing".to_string()),
        Message::turn_scoped_system("collapsed output notice".to_string()),
        Message::user("continue".to_string()),
    ];
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };
    // CapturingProvider uses the trait default
    // `supports_turn_scoped_system_messages == false`, like merge-gateway.
    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let (compacted, mode) = compact_history_manual_with_parent_context(
        &provider,
        "openai/gpt-6-luna",
        &history,
        &config,
        &ManualCompactionOptions::default(),
        None,
        None,
    )
    .await
    .expect("gateway local compaction must succeed");
    assert_eq!(mode, CompactionMode::Local);
    let captured = provider.last_request.lock().unwrap().clone().expect("captured request");
    assert!(
        captured.messages.iter().all(|message| message.clear_at.is_none()),
        "gateway summary fork must not carry clear_at"
    );
    assert!(
        captured
            .messages
            .iter()
            .any(|message| message.content.as_text().as_ref() == "collapsed output notice"),
        "collapsed notice text must survive as ordinary directive"
    );
    assert!(!compacted.is_empty());
}

#[tokio::test]
async fn manual_compaction_with_parent_context_reuses_prefix() {
    use super::{CompactionParentContext, compact_history_manual_with_parent_context};

    let history = sample_history();
    let config = CompactionConfig {
        always_summarize: true,
        ..CompactionConfig::default()
    };
    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let parent = CompactionParentContext {
        system_prompt: Some(Arc::from("parent system")),
        tools: None,
    };
    let (compacted, mode) = compact_history_manual_with_parent_context(
        &provider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
        None,
        Some(&parent),
    )
    .await
    .expect("parent-aware compaction");
    assert_eq!(mode, CompactionMode::Local);
    let captured = provider.last_request.lock().unwrap().clone().expect("captured request");
    assert_eq!(captured.system_prompt.as_deref(), Some("parent system"));
    assert_eq!(captured.messages.len(), history.len() + 1);
    assert_eq!(compacted[0].content.as_text(), "Previous conversation summary:\nsummary");
}

#[tokio::test]
async fn hierarchical_bands_reuse_parent_prefix_without_tool_calls() {
    use super::{CompactionParentContext, compact_history_manual_with_parent_context};

    let history = (0..12)
        .map(|index| Message::user(format!("hierarchical request {index}")))
        .collect::<Vec<_>>();
    let config = CompactionConfig {
        always_summarize: true,
        hierarchical: true,
        ..CompactionConfig::default()
    };
    let provider = CapturingProvider { last_request: Mutex::new(None) };
    let parent = CompactionParentContext {
        system_prompt: Some(Arc::from("parent system")),
        tools: None,
    };
    let (compacted, mode) = compact_history_manual_with_parent_context(
        &provider,
        "stub-model",
        &history,
        &config,
        &ManualCompactionOptions::default(),
        None,
        Some(&parent),
    )
    .await
    .expect("hierarchical compaction");
    assert_eq!(mode, CompactionMode::Local);
    // CapturingProvider keeps the last request, which is the detail band.
    let captured = provider.last_request.lock().unwrap().clone().expect("captured detail request");
    assert_eq!(captured.system_prompt.as_deref(), Some("parent system"));
    assert!(matches!(captured.tool_choice, Some(crate::llm::provider::ToolChoice::None)));
    assert!(!compacted.is_empty());
}
