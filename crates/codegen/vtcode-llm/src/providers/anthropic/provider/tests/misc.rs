use super::*;

#[test]
fn convert_to_anthropic_format_falls_back_to_provider_default_model() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        messages: vec![Message::user("hello".to_string())].into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");

    assert_eq!(payload["model"], models::CLAUDE_SONNET_5);
}
#[test]
fn beta_header_includes_advanced_tool_use_for_tool_search_requests() {
    let provider = AnthropicProvider::with_model("test-key".to_string(), models::CLAUDE_SONNET_5.to_string());
    let request = LLMRequest {
        model: models::CLAUDE_SONNET_5.to_string(),
        messages: vec![Message::user("find the deployment tool".to_string())].into(),
        tools: Some(std::sync::Arc::new(vec![ToolDefinition::tool_search(
            crate::provider::ToolSearchAlgorithm::Regex,
        )])),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    let beta_header = provider
        .beta_header_for_request(&request, &payload, true, None)
        .expect("beta header");

    assert!(beta_header.contains("advanced-tool-use-2025-11-20"));
}
#[test]
fn code_execution_beta_name_uses_tool_revision() {
    assert_eq!(code_execution_beta_name("code_execution_20250825").as_deref(), Some("code-execution-2025-08-25"));
    assert_eq!(code_execution_beta_name("code_execution_20250522").as_deref(), Some("code-execution-2025-05-22"));
    assert!(code_execution_beta_name("code_execution_latest").is_none());
}
#[test]
fn turn_scoped_system_notice_is_emitted_after_tool_result() {
    let model = models::anthropic::CLAUDE_FABLE_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![crate::provider::ToolCall::function(
                    "toolu_1".to_string(),
                    "exec_command".to_string(),
                    "{}".to_string(),
                )],
            ),
            Message::tool_response("toolu_1".to_string(), "exit 1".to_string()),
            Message::turn_scoped_system("Only you see that command's output".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert_eq!(payload["messages"][1]["role"], "user");
    assert_eq!(payload["messages"][2]["role"], "system");
    assert_eq!(payload["messages"][2]["clear_at"], "next_user_message");
    assert_eq!(payload["messages"][2]["content"][0]["type"], "text");
    assert_eq!(payload["messages"][2]["content"][0]["text"], "Only you see that command's output");
    assert!(
        !payload
            .get("system")
            .is_some_and(|system| { system.to_string().contains("Only you see that command's output") })
    );

    let beta_header = provider
        .beta_header_for_request(&request, &payload, false, None)
        .expect("turn-scoped beta header");
    assert_eq!(
        beta_header
            .split(", ")
            .filter(|beta| *beta == headers::MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA)
            .count(),
        1
    );
}
#[test]
fn turn_scoped_system_notice_is_promoted_for_unsupported_sonnet() {
    let model = models::CLAUDE_SONNET_5;
    let provider = AnthropicProvider::with_model("test-key".to_string(), model.to_string());
    let request = LLMRequest {
        model: model.to_string(),
        messages: vec![
            Message::user("continue".to_string()),
            Message::turn_scoped_system("do not leak output".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = provider.convert_to_anthropic_format(&request).expect("payload conversion");
    assert!(
        payload["messages"]
            .as_array()
            .is_some_and(|messages| { messages.iter().all(|message| message.get("clear_at").is_none()) })
    );
    assert!(
        payload
            .get("system")
            .is_some_and(|system| { system.to_string().contains("do not leak output") })
    );
    let beta_header = provider.beta_header_for_request(&request, &payload, false, None);
    assert!(!beta_header.is_some_and(|header| {
        header
            .split(", ")
            .any(|beta| beta == headers::MID_CONVERSATION_SYSTEM_CLEAR_AT_BETA)
    }));
}
#[test]
fn turn_scoped_system_capability_matches_supported_model_families() {
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_FABLE_5, ""));
    assert!(capabilities::supports_turn_scoped_system_messages("claude-opus-4-8", ""));
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_OPUS_5, ""));
    assert!(capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_SONNET_5_5, ""));
    assert!(!capabilities::supports_turn_scoped_system_messages(models::anthropic::CLAUDE_SONNET_5, ""));
}
