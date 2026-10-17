use super::*;

#[test]
fn signed_provider_compaction_strips_old_thinking_but_preserves_other_details() {
    let old_assistant = Message::assistant_with_tools(
        "old answer".to_string(),
        vec![ToolCall::function(
            "call-1".to_string(),
            "lookup".to_string(),
            "{}".to_string(),
        )],
    )
    .with_reasoning_details(Some(vec![
        json!({
            "type": "thinking",
            "thinking": "old trace",
            "signature": "old-signature"
        }),
        json!({"type": "provider_extension", "state": "keep"}),
    ]));
    let history = vec![
        Message::user("old request".to_string()),
        old_assistant,
        Message::tool_response("call-1".to_string(), "lookup result".to_string()),
        Message::user("latest request ".repeat(100_000)),
    ];

    let compacted = build_provider_compacted_history(
        &history,
        json!({
            "type": "compaction",
            "content": "summary",
            "signature": "new-signature"
        }),
        &CompactionConfig::default(),
        true,
        20_000,
    );

    let old = compacted
        .iter()
        .find(|message| message.content.as_text() == "old answer")
        .expect("retained action history should keep the old answer");
    let details = old.reasoning_details.as_ref().expect("opaque detail should survive");
    assert_eq!(details, &vec![json!({"type": "provider_extension", "state": "keep"})]);
}

#[test]
fn provider_compaction_replaces_old_provider_marker_in_continuity_tail() {
    let old_marker = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
        "type": "compaction",
        "content": "old summary",
        "signature": "old-signature",
    })]));
    let history = vec![
        Message::user("old request".to_string()),
        old_marker,
        Message::user("latest request".to_string()),
        Message::assistant("latest response".to_string()),
    ];

    let compacted = build_provider_compacted_history(
        &history,
        json!({
            "type": "compaction",
            "content": "new summary",
            "signature": "new-signature"
        }),
        &CompactionConfig::default(),
        true,
        20_000,
    );
    let markers = compacted
        .iter()
        .flat_map(|message| message.reasoning_details.as_deref().unwrap_or(&[]))
        .filter(|detail| detail.get("type").and_then(serde_json::Value::as_str) == Some("compaction"))
        .collect::<Vec<_>>();

    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0]["content"], "new summary");
}

#[test]
fn collect_retained_keeps_tool_result_with_its_assistant() {
    // When the assistant tool-call turn is retained, its tool result must
    // survive so the turn stays coherent (the model sees each call's return).
    let history = vec![
        Message::user("u1".to_string()),
        assistant_with_calls("calling tool", "c1"),
        Message::tool_response("c1".to_string(), "r1".to_string()),
        Message::user("u2".to_string()),
    ];
    let retained = collect_retained_user_messages(&history, 20_000, 4);
    assert!(retained.iter().any(|m| m.content.as_text().contains("u1")));
    assert!(retained.iter().any(|m| m.content.as_text().contains("u2")));
    assert!(
        retained.iter().any(|m| m.content.as_text().contains("r1")),
        "tool result paired with its retained assistant must survive"
    );
}

#[test]
fn collect_retained_drops_orphaned_tool_result() {
    // If the assistant tool-call turn is dropped (over the retention cap),
    // its tool result is orphaned — the model never saw the call — and must
    // not survive compaction, because an orphaned result is invalid to send
    // to a provider.
    let history = vec![
        Message::user("u1".to_string()),
        assistant_with_calls("calling tool", "c1"),
        Message::tool_response("c1".to_string(), "r1".to_string()),
        Message::user("u2".to_string()),
    ];
    let retained = collect_retained_user_messages(&history, 20_000, 3);
    assert!(retained.iter().any(|m| m.content.as_text().contains("u1")));
    assert!(retained.iter().any(|m| m.content.as_text().contains("u2")));
    assert!(!retained.iter().any(|m| m.content.as_text().contains("r1")), "orphaned tool result must be dropped");
}

#[tokio::test]
async fn manual_compaction_strategy_picks_local_for_plain_provider() {
    assert_eq!(manual_compaction_strategy(&StubProvider, "stub-model"), CompactionStrategy::Local);
}

#[tokio::test]
async fn manual_compaction_strategy_picks_native_standalone_for_manual_provider() {
    let provider = ManualStandaloneProvider {
        last_options: Mutex::new(None),
        output: canonical_standalone_output(),
    };
    assert_eq!(manual_compaction_strategy(&provider, "stub-model"), CompactionStrategy::NativeStandalone);
}

#[tokio::test]
async fn manual_compaction_strategy_picks_native_inline_for_responses_capable_provider() {
    let provider = InlinePauseProvider {
        last_request: Mutex::new(None),
        include_compaction_detail: true,
    };
    assert_eq!(manual_compaction_strategy(&provider, "stub-model"), CompactionStrategy::NativeInline);
}

#[tokio::test]
async fn manual_compaction_strategy_picks_local_for_compatible_endpoint() {
    // OpenAI-compatible custom endpoints report `supports_responses_compaction`
    // but cannot serve standalone `/responses/compact` or Anthropic inline
    // compaction; they must route to Local, not NativeInline.
    assert_eq!(manual_compaction_strategy(&CompatibleEndpointProvider, "stub-model"), CompactionStrategy::Local);
}
