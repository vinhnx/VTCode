use super::*;

#[test]
fn session_message_converts_back_and_forth() {
    let mut original = Message::assistant("Test response".to_owned());
    original.reasoning = Some("Model thoughts".to_owned());
    original.phase = Some(AssistantPhase::FinalAnswer);
    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(original.role, restored.role);
    assert_eq!(original.content, restored.content);
    assert_eq!(original.reasoning.as_deref(), stored.reasoning.as_deref().map(|v| v.as_str()));
    assert_eq!(original.reasoning, restored.reasoning);
    assert_eq!(original.tool_call_id, restored.tool_call_id);
    assert_eq!(original.phase, stored.phase);
    assert_eq!(original.phase, restored.phase);
}
#[test]
fn session_message_roundtrip_preserves_commentary_phase() {
    let original = Message::assistant("Working".to_owned()).with_phase(Some(AssistantPhase::Commentary));
    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(stored.phase, Some(AssistantPhase::Commentary));
    assert_eq!(restored.phase, Some(AssistantPhase::Commentary));
}
#[test]
fn session_message_roundtrip_preserves_turn_scoped_clear_scope() {
    let original = Message::turn_scoped_system("Only you see the output".to_owned());
    let stored = SessionMessage::from(&original);
    let encoded = serde_json::to_vec(&stored).expect("session message serialization");
    let decoded: SessionMessage = serde_json::from_slice(&encoded).expect("session message deserialization");
    let restored = Message::from(&decoded);

    assert_eq!(decoded.clear_at, Some(MessageClearAt::NextUserMessage));
    assert_eq!(restored.clear_at, Some(MessageClearAt::NextUserMessage));
}
#[test]
fn session_message_preserves_tool_calls_reasoning_details_and_origin_tool() {
    let mut original = Message::assistant("Calling a tool".to_owned());
    original.reasoning_details = Some(vec![serde_json::json!({
        "summary": "tool call planning"
    })]);
    original.tool_calls = Some(vec![ToolCall::function(
        "call_1".to_string(),
        "command_session".to_string(),
        "{\"cmd\":\"cargo fmt\"}".to_string(),
    )]);
    original.origin_tool = Some("command_session".to_string());

    let stored = SessionMessage::from(&original);
    let restored = Message::from(&stored);

    assert_eq!(stored.reasoning_details.as_deref(), original.reasoning_details.as_ref());
    assert_eq!(stored.tool_calls.as_deref(), original.tool_calls.as_ref());
    assert_eq!(stored.origin_tool.as_deref().map(|v| v.as_str()), original.origin_tool.as_deref());
    assert_eq!(restored.reasoning_details, original.reasoning_details);
    assert_eq!(restored.tool_calls, original.tool_calls);
    assert_eq!(restored.origin_tool, original.origin_tool);
}
#[test]
fn session_message_discards_empty_sparse_metadata() {
    let mut original = Message::assistant("Calling a tool".to_owned());
    original.reasoning = Some(String::new());
    original.reasoning_details = Some(Vec::new());
    original.tool_calls = Some(Vec::new());
    original.tool_call_id = Some(String::new());
    original.origin_tool = Some(String::new());

    let stored = SessionMessage::from(&original);
    assert!(stored.reasoning.is_none());
    assert!(stored.reasoning_details.is_none());
    assert!(stored.tool_calls.is_none());
    assert!(stored.tool_call_id.is_none());
    assert!(stored.origin_tool.is_none());

    let restored = Message::from(&stored);
    assert!(restored.reasoning.is_none());
    assert!(restored.reasoning_details.is_none());
    assert!(restored.tool_calls.is_none());
    assert!(restored.tool_call_id.is_none());
    assert!(restored.origin_tool.is_none());
}
#[test]
fn session_message_deserialize_discards_empty_sparse_metadata() {
    let payload = serde_json::json!({
        "role": "Assistant",
        "content": "done",
        "reasoning": "",
        "reasoning_details": [],
        "tool_calls": [],
        "tool_call_id": "",
        "origin_tool": "",
    });

    let message: SessionMessage = serde_json::from_value(payload).expect("session message");
    assert!(message.reasoning.is_none());
    assert!(message.reasoning_details.is_none());
    assert!(message.tool_calls.is_none());
    assert!(message.tool_call_id.is_none());
    assert!(message.origin_tool.is_none());
}
#[test]
fn session_message_preserves_parts() {
    let original = Message::assistant_with_parts(vec![
        ContentPart::text("See attached image".to_owned()),
        ContentPart::text("See attached image".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
        ContentPart::image("encoded-image".to_owned(), "image/png".to_owned()),
    ]);
    let stored = SessionMessage::from(&original);

    assert_eq!(stored.content, original.content);

    let restored = Message::from(&stored);
    assert_eq!(restored.content, original.content);
}
#[test]
fn session_message_layout_is_smaller_than_unboxed_equivalent() {
    #[derive(Debug)]
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    struct UnboxedSessionMessageLike(
        MessageRole,
        MessageContent,
        Option<String>,
        Option<Vec<serde_json::Value>>,
        Option<Vec<ToolCall>>,
        Option<String>,
        Option<AssistantPhase>,
        Option<String>,
    );

    assert!(size_of::<SessionMessage>() < size_of::<UnboxedSessionMessageLike>());
}
#[test]
fn boxed_session_progress_option_is_pointer_sized() {
    assert_eq!(size_of::<Option<Box<SessionProgress>>>(), size_of::<usize>());
    assert!(size_of::<Option<SessionProgress>>() > size_of::<Option<Box<SessionProgress>>>());
}
