use super::*;

#[test]
fn push_tool_response_replaces_existing_tool_call_entry() {
    let mut history = vec![uni::Message::tool_response(
        "call_1".to_string(),
        "{\"output\":\"first\"}".to_string(),
    )];

    let update = push_tool_response(&mut history, "call_1".to_string(), None, "{\"output\":\"latest\"}".to_string());

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text_borrowed(), Some("{\"output\":\"latest\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Replaced { previous_text_len: "{\"output\":\"first\"}".len() });
}
#[test]
fn push_tool_response_sets_origin_tool_when_provided() {
    let mut history = Vec::new();

    let update =
        push_tool_response(&mut history, "call_1".to_string(), Some("read_file"), "{\"output\":\"first\"}".to_string());

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].origin_tool.as_deref(), Some("read_file"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}
#[test]
fn push_tool_response_refreshes_origin_tool_when_replacing_same_call() {
    let mut history = vec![uni::Message::tool_response("call_1".to_string(), "old".to_string())];

    let update = push_tool_response(&mut history, "call_1".to_string(), Some("exec_command"), "new".to_string());

    assert_eq!(update, ToolResponseHistoryUpdate::Replaced { previous_text_len: 3 });
    assert_eq!(history[0].origin_tool.as_deref(), Some("exec_command"));
}
#[test]
fn push_tool_response_appends_when_id_reused_across_assistant_boundary() {
    // Fabricated ids can collide across turns (e.g. index-based fallbacks).
    // A later assistant message re-declaring the same id must not cause a
    // new result to clobber the earlier, unrelated Tool response.
    let mut history = vec![
        uni::Message::assistant_with_tools(
            "first".into(),
            vec![uni::ToolCall::function(
                "call_1".into(),
                "file_operation".into(),
                "{}".into(),
            )],
        ),
        uni::Message::tool_response("call_1".to_string(), "{\"output\":\"first\"}".into()),
        uni::Message::assistant_with_tools(
            "second".into(),
            vec![uni::ToolCall::function(
                "call_1".into(),
                tools::CODE_SEARCH.into(),
                "{}".into(),
            )],
        ),
    ];

    let update = push_tool_response(
        &mut history,
        "call_1".to_string(),
        Some(tools::CODE_SEARCH),
        "{\"output\":\"second\"}".to_string(),
    );

    let tool_messages: Vec<&uni::Message> = history
        .iter()
        .filter(|message| matches!(message.role, uni::MessageRole::Tool))
        .collect();
    assert_eq!(tool_messages.len(), 2, "must append, not overwrite");
    assert_eq!(
        tool_messages[0].content.as_text_borrowed(),
        Some("{\"output\":\"first\"}"),
        "earlier unrelated Tool result must remain intact"
    );
    assert_eq!(tool_messages[1].content.as_text_borrowed(), Some("{\"output\":\"second\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}
#[test]
fn push_tool_response_appends_when_assistant_has_no_tool_calls() {
    // When an Assistant message has no tool_calls (e.g. commentary-only
    // message between tool calls), the boundary must STILL stop the scan.
    // Otherwise a later Tool response with a colliding fabricated id would
    // overwrite an earlier, unrelated Tool result.
    let mut history = vec![
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_0".into(),
                "file_operation".into(),
                "{}".into(),
            )],
        ),
        uni::Message::tool_response("call_0".to_string(), "{\"output\":\"file content\"}".into()),
        // Commentary Assistant with no tool_calls — must act as boundary
        uni::Message::assistant("I need to retry.".into()),
        uni::Message::assistant_with_tools(
            String::new(),
            vec![uni::ToolCall::function(
                "call_0".into(),
                "apply_patch".into(),
                "{}".into(),
            )],
        ),
    ];

    let update = push_tool_response(
        &mut history,
        "call_0".to_string(),
        Some("apply_patch"),
        "{\"output\":\"patch result\"}".to_string(),
    );

    let tool_messages: Vec<&uni::Message> = history
        .iter()
        .filter(|message| matches!(message.role, uni::MessageRole::Tool))
        .collect();
    assert_eq!(tool_messages.len(), 2, "must append, not overwrite the earlier file read");
    assert_eq!(
        tool_messages[0].content.as_text_borrowed(),
        Some("{\"output\":\"file content\"}"),
        "earlier file read result must remain intact"
    );
    assert_eq!(tool_messages[1].content.as_text_borrowed(), Some("{\"output\":\"patch result\"}"));
    assert_eq!(update, ToolResponseHistoryUpdate::Appended);
}
