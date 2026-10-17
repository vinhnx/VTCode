use super::*;

#[test]
fn coherence_force_keeps_tool_results_not_in_selection() {
    // Regression: `coherence_tool_call_pairs` force-keeps the Tool results
    // that follow a retained Assistant-with-tool_calls, but the previous
    // implementation derived its output from `selected` only, so those
    // force-kept messages silently vanished. A compacted history can then
    // carry an Assistant tool-call with no result, which providers reject.
    use super::coherence_tool_call_pairs;

    let history = vec![
        Message::user("check the code".to_string()),
        Message::assistant("Looking...".to_string()).with_tool_calls(vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: Some(crate::llm::provider::FunctionCall {
                namespace: None,
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            }),
            text: None,
            thought_signature: None,
        }]),
        Message::tool_response("call_1".to_string(), "tool result that must survive".to_string()),
        Message::assistant("Done.".to_string()),
    ];

    // Selection keeps only the assistant-with-tool-calls turn, dropping the
    // following Tool result from the budget-driven selection.
    let selected = vec![(1, history[1].clone())];
    let result = coherence_tool_call_pairs(&history, &selected);

    let tool_results = result
        .iter()
        .filter(|(_, m)| m.role == MessageRole::Tool)
        .map(|(idx, m)| (*idx, m.content.as_text().to_string()))
        .collect::<Vec<_>>();

    assert_eq!(
        tool_results,
        vec![(2usize, "tool result that must survive".to_string())],
        "force-kept tool result must survive compaction even when not selected"
    );
    // History order must be preserved with the assistant preceding its result.
    let indices = result.iter().map(|(idx, _)| *idx).collect::<Vec<_>>();
    assert_eq!(indices, vec![1, 2]);
}

#[test]
fn coherence_drops_orphaned_tool_results_of_unretained_assistant() {
    use super::coherence_tool_call_pairs;

    let history = vec![
        Message::user("check".to_string()),
        Message::assistant("Looking...".to_string()).with_tool_calls(vec![ToolCall {
            id: "call_1".to_string(),
            call_type: "function".to_string(),
            function: Some(crate::llm::provider::FunctionCall {
                namespace: None,
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            }),
            text: None,
            thought_signature: None,
        }]),
        Message::tool_response("call_1".to_string(), "orphan result".to_string()),
    ];

    // The calling assistant was NOT retained; its orphaned Tool result must
    // be dropped so we never emit a result the model never saw a call for.
    let selected = vec![(0, history[0].clone())];
    let result = coherence_tool_call_pairs(&history, &selected);

    assert_eq!(result.len(), 1, "orphaned tool result must be dropped");
    assert_eq!(result[0].0, 0);
}

#[test]
fn cache_safe_fork_reuses_parent_prefix_with_appended_prompt() {
    use super::{CompactionParentContext, build_cache_safe_compaction_history, compaction_summary_request};

    let history = sample_history();
    let forked = build_cache_safe_compaction_history(&history, "Summarize now.");
    assert_eq!(forked.len(), history.len() + 1);
    for (forked_msg, original) in forked.iter().zip(history.iter()) {
        assert_eq!(forked_msg.content.as_text(), original.content.as_text());
        assert_eq!(forked_msg.role, original.role);
    }
    let last = forked.last().expect("appended compaction prompt");
    assert_eq!(last.role, MessageRole::User);
    assert_eq!(last.content.as_text(), "Summarize now.");

    // Parent prefix reuse: same system/tools, tool calls disabled.
    let parent = CompactionParentContext {
        system_prompt: Some(Arc::from("stable system")),
        tools: Some(Arc::new(vec![crate::llm::provider::ToolDefinition::function(
            "read".to_string(),
            "read".to_string(),
            serde_json::json!({"type": "object"}),
        )])),
    };
    let request =
        compaction_summary_request("stub-model", &history, "Summarize now.", None, None, None, true, Some(&parent));
    assert_eq!(request.system_prompt.as_deref(), Some("stable system"));
    assert_eq!(request.tools.as_deref().map(Vec::len), Some(1));
    assert_eq!(request.messages.len(), history.len() + 1);
    assert!(matches!(request.tool_choice, Some(crate::llm::provider::ToolChoice::None)));
    assert!(!parent.is_empty());
    assert!(CompactionParentContext::default().is_empty());
}

#[test]
fn compaction_summary_request_strips_turn_scoped_without_native_support() {
    use super::compaction_summary_request;

    let history = vec![
        Message::user("do the thing".to_string()),
        Message::turn_scoped_system("collapsed output notice".to_string()),
    ];
    let request = compaction_summary_request("stub-model", &history, "Summarize now.", None, None, None, false, None);
    let scoped = request
        .messages
        .iter()
        .find(|message| message.content.as_text().as_ref() == "collapsed output notice")
        .expect("collapsed notice text must survive sanitization");
    assert!(scoped.clear_at.is_none(), "merge-gateway wire must not carry clear_at");
    assert_eq!(request.messages.len(), history.len() + 1);
}

#[test]
fn compaction_summary_request_preserves_turn_scoped_with_native_support() {
    use super::compaction_summary_request;

    let history = vec![
        Message::user("do the thing".to_string()),
        Message::turn_scoped_system("collapsed output notice".to_string()),
    ];
    let request = compaction_summary_request("stub-model", &history, "Summarize now.", None, None, None, true, None);
    let scoped = request
        .messages
        .iter()
        .find(|message| message.content.as_text().as_ref() == "collapsed output notice")
        .expect("collapsed notice must be present");
    assert!(scoped.clear_at.is_some(), "anthropic wire must keep clear_at");
}
