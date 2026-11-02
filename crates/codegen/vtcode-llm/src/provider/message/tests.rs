use super::{AssistantPhase, ContentPart, ImageDetail, Message, MessageClearAt, MessageContent, MessageRole, ToolCall};

#[test]
fn message_content_parts_concatenate_without_extra_spaces() {
    let parts = vec![
        ContentPart::text("Andre".to_string()),
        ContentPart::text("j".to_string()),
        ContentPart::text(" Kar".to_string()),
        ContentPart::text("pathy".to_string()),
        ContentPart::text("'s".to_string()),
    ];
    let content = MessageContent::Parts(parts);

    assert_eq!(content.as_text().as_ref() as &str, "Andrej Karpathy's");
}

#[test]
fn message_content_parts_with_single_text_stays_borrowed() {
    let content = MessageContent::Parts(vec![ContentPart::text("borrowed".to_string())]);

    assert!(matches!(content.as_text(), std::borrow::Cow::Borrowed("borrowed")));
}

#[test]
fn message_content_parts_without_text_stays_borrowed_empty() {
    let content = MessageContent::Parts(vec![ContentPart::image("encoded".to_string(), "image/png".to_string())]);

    assert!(matches!(content.as_text(), std::borrow::Cow::Borrowed("")));
}

#[test]
fn assistant_phase_parses_wire_strings() {
    assert_eq!(AssistantPhase::from_wire_str("commentary"), Some(AssistantPhase::Commentary));
    assert_eq!(AssistantPhase::from_wire_str("final_answer"), Some(AssistantPhase::FinalAnswer));
    assert_eq!(AssistantPhase::from_wire_str("other"), None);
}

/// `AssistantPhase` exposes the same value through the derived serde wire
/// form, `as_str()`, and `from_wire_str()`. Lock all three so a one-sided
/// edit cannot desync the persisted phase from the wire parser.
#[test]
fn assistant_phase_string_paths_stay_in_lockstep() {
    for phase in [AssistantPhase::Commentary, AssistantPhase::FinalAnswer] {
        let expected = phase.as_str();
        assert_eq!(
            serde_json::to_value(phase).expect("serializes"),
            serde_json::json!(expected),
            "serde form drifted for {phase:?}"
        );
        assert_eq!(AssistantPhase::from_wire_str(expected), Some(phase), "from_wire_str drift");
    }
}

/// `ImageDetail` mirrors its `#[serde(rename)]` values in `as_str()` and
/// `from_str()`; lock them together.
#[test]
fn image_detail_string_paths_stay_in_lockstep() {
    for detail in [
        ImageDetail::Low,
        ImageDetail::High,
        ImageDetail::Original,
        ImageDetail::Auto,
    ] {
        let expected = detail.as_str();
        assert_eq!(
            serde_json::to_value(&detail).expect("serializes"),
            serde_json::json!(expected),
            "serde form drifted for {detail:?}"
        );
        assert_eq!(ImageDetail::from_str(expected), Some(detail), "from_str drift");
    }
}

#[test]
fn with_phase_ignores_non_assistant_roles() {
    let user = Message::user("hello".to_string()).with_phase(Some(AssistantPhase::Commentary));
    let tool =
        Message::tool_response("call_1".to_string(), "ok".to_string()).with_phase(Some(AssistantPhase::FinalAnswer));

    assert_eq!(user.role, MessageRole::User);
    assert!(user.phase.is_none());
    assert_eq!(tool.role, MessageRole::Tool);
    assert!(tool.phase.is_none());
}

#[test]
fn turn_scoped_system_message_round_trips_clear_scope() {
    let message = Message::turn_scoped_system("Only you see the output".to_string());
    let encoded = serde_json::to_value(&message).expect("message serialization");

    assert_eq!(encoded["role"], "System");
    assert_eq!(encoded["clear_at"], "next_user_message");
    assert_eq!(message.clear_at, Some(MessageClearAt::NextUserMessage));
    assert_eq!(serde_json::from_value::<Message>(encoded).expect("message deserialization"), message);
}

#[test]
fn turn_scoped_system_message_requires_non_anthropic_wire_translation() {
    let error = Message::turn_scoped_system("notice".to_string())
        .validate_for_provider("openai")
        .expect_err("raw Anthropic lifecycle fields are not valid on an OpenAI wire");
    assert!(error.contains("only supported by Anthropic"));
}

#[test]
fn validate_for_provider_accepts_recovered_tool_arguments() {
    let message = Message::assistant_with_tools(
        String::new(),
        vec![ToolCall::function(
            "call_search".to_string(),
            "code_search".to_string(),
            "{\"query\": \"persistent_memory\", \"path\": \"crates/codegen/vtcode-core/src</parameter>\n<</invoke>\n</minimax:tool_call>".to_string(),
        )],
    );

    message.validate_for_provider("anthropic").unwrap();
}

#[test]
fn validate_for_provider_requires_tool_call_id_for_meta() {
    let message = Message {
        role: MessageRole::Tool,
        content: MessageContent::text("result".to_owned()),
        ..Default::default()
    };

    let error = message
        .validate_for_provider("meta")
        .expect_err("Meta tool messages need an id");
    assert!(error.contains("tool_call_id"));
}
