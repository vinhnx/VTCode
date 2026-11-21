use super::{
    build_advisor_blocks, build_messages, build_reasoning_blocks, content_blocks_from_message_content,
    hoist_largest_user_message,
};
use crate::provider::{ContentPart, LLMRequest, Message, MessageContent};
use crate::providers::anthropic_types::{AnthropicContentBlock, CacheControl};
use serde_json::json;
use vtcode_config::core::AnthropicPromptCacheSettings;

fn message_anchor_flags(messages: &[super::AnthropicMessage]) -> Vec<bool> {
    messages
        .iter()
        .map(|msg| {
            msg.content
                .iter()
                .any(|block| matches!(block, AnthropicContentBlock::Text { cache_control: Some(_), .. }))
        })
        .collect()
}

fn rolling_anchor_fixture() -> (LLMRequest, Vec<Message>, Option<CacheControl>) {
    let request = LLMRequest::default();
    let messages = vec![
        Message::user("aaaa".to_string()),
        Message::user("bbbb".to_string()),
        Message::user("cccc".to_string()),
    ];
    let cache_control = Some(CacheControl {
        control_type: "ephemeral".into(),
        ttl: Some("5m".into()),
    });
    (request, messages, cache_control)
}

#[test]
fn build_messages_anchors_only_last_two_qualifying_messages() {
    let (request, source_messages, cache_control) = rolling_anchor_fixture();
    let settings = AnthropicPromptCacheSettings {
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let mut breakpoints_remaining = 4usize;

    let messages =
        build_messages(&request, &source_messages, cache_control.as_ref(), &settings, &mut breakpoints_remaining, "")
            .expect("build_messages");

    assert_eq!(message_anchor_flags(&messages), vec![false, true, true]);
    assert_eq!(breakpoints_remaining, 2);
}

#[test]
fn build_messages_skips_anchors_when_breakpoint_budget_exhausted() {
    let (request, source_messages, cache_control) = rolling_anchor_fixture();
    let settings = AnthropicPromptCacheSettings {
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let mut breakpoints_remaining = 0usize;

    let messages =
        build_messages(&request, &source_messages, cache_control.as_ref(), &settings, &mut breakpoints_remaining, "")
            .expect("build_messages");

    assert_eq!(message_anchor_flags(&messages), vec![false, false, false]);
    assert_eq!(breakpoints_remaining, 0);
}

#[test]
fn build_messages_anchors_newest_message_when_only_one_breakpoint_left() {
    let (request, source_messages, cache_control) = rolling_anchor_fixture();
    let settings = AnthropicPromptCacheSettings {
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let mut breakpoints_remaining = 1usize;

    let messages =
        build_messages(&request, &source_messages, cache_control.as_ref(), &settings, &mut breakpoints_remaining, "")
            .expect("build_messages");

    assert_eq!(message_anchor_flags(&messages), vec![false, false, true]);
    assert_eq!(breakpoints_remaining, 0);
}

#[test]
fn build_messages_ignores_short_messages_when_selecting_anchors() {
    let request = LLMRequest::default();
    let source_messages = vec![
        Message::user("a".repeat(300)),
        Message::user("hi".to_string()),
        Message::user("b".repeat(300)),
    ];
    let cache_control = Some(CacheControl {
        control_type: "ephemeral".into(),
        ttl: Some("5m".into()),
    });
    let settings = AnthropicPromptCacheSettings::default();
    let mut breakpoints_remaining = 4usize;

    let messages =
        build_messages(&request, &source_messages, cache_control.as_ref(), &settings, &mut breakpoints_remaining, "")
            .expect("build_messages");

    // Both long messages qualify (default threshold is 256 chars); the short
    // middle message never receives an anchor.
    assert_eq!(message_anchor_flags(&messages), vec![true, false, true]);
    assert_eq!(breakpoints_remaining, 2);
}

#[test]
fn build_reasoning_blocks_decodes_stringified_json_detail() {
    let message = Message::assistant(String::new())
        .with_reasoning_details(Some(vec![json!(r#"{"type":"thinking","thinking":"trace","signature":"sig_123"}"#)]));

    let blocks = build_reasoning_blocks(&message);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        AnthropicContentBlock::Thinking { thinking, signature, .. } => {
            assert_eq!(thinking, "trace");
            assert_eq!(signature.as_deref(), Some("sig_123"));
        }
        other => panic!("expected thinking block, got {other:?}"),
    }
}

#[test]
fn build_reasoning_blocks_preserves_omitted_thinking_with_signature() {
    let message = Message::assistant(String::new())
        .with_reasoning_details(Some(vec![json!(r#"{"type":"thinking","thinking":"","signature":"sig_omitted"}"#)]));

    let blocks = build_reasoning_blocks(&message);
    assert_eq!(blocks.len(), 1);
    match &blocks[0] {
        AnthropicContentBlock::Thinking { thinking, signature, .. } => {
            assert!(thinking.is_empty());
            assert_eq!(signature.as_deref(), Some("sig_omitted"));
        }
        other => panic!("expected thinking block, got {other:?}"),
    }
}

#[test]
fn build_messages_replays_signed_compaction_block_and_keeps_later_thinking() {
    let compaction = json!({
        "type": "compaction",
        "content": null,
        "signature": "signed-summary",
        "encrypted_content": "opaque-extension",
        "cache_control": {"type": "ephemeral"}
    });
    let first = Message::assistant(String::new()).with_reasoning_details(Some(vec![compaction.clone()]));
    let mut later = Message::assistant("continuation".to_string()).with_reasoning_details(Some(vec![json!({
        "type": "thinking",
        "thinking": "thinking from the summarized transcript",
        "signature": "old-thinking"
    })]));
    later.content = MessageContent::Text("continuation".to_string());
    let request = LLMRequest {
        model: "claude-sonnet-5".to_string(),
        messages: vec![first, later, Message::user("next".to_string())].into(),
        ..Default::default()
    };
    let mut breakpoints_remaining = 0usize;

    let messages = build_messages(
        &request,
        request.messages.as_ref(),
        None,
        &AnthropicPromptCacheSettings::default(),
        &mut breakpoints_remaining,
        "",
    )
    .expect("signed compaction should be replayable");

    assert_eq!(messages[0].role, "assistant");
    assert_eq!(messages[0].content.len(), 1);
    assert_eq!(serde_json::to_value(&messages[0].content[0]).expect("serialize block"), compaction);
    assert_eq!(messages[1].content.len(), 2);
    assert!(
        matches!(messages[1].content[0], AnthropicContentBlock::Thinking { ref thinking, .. } if thinking == "thinking from the summarized transcript")
    );
    assert!(matches!(messages[1].content[1], AnthropicContentBlock::Text { ref text, .. } if text == "continuation"));
}

#[test]
fn hoist_largest_user_message_keeps_compaction_marker_at_prefix() {
    let marker = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
        "type": "compaction",
        "content": null,
        "signature": "signed-summary",
    })]));
    let largest = Message::user("largest".repeat(256));
    let mut messages = vec![marker.clone(), Message::user("small".to_string()), largest.clone()];

    hoist_largest_user_message(&mut messages);

    assert_eq!(messages.first(), Some(&marker));
    assert_eq!(messages.get(1), Some(&largest));
}

#[test]
fn build_messages_rejects_misplaced_signed_compaction_block() {
    let history = vec![
        Message::user("older context".to_string()),
        Message::assistant(String::new()).with_reasoning_details(Some(vec![json!({
            "type": "compaction",
            "content": "summary",
            "signature": "signed-summary"
        })])),
    ];
    let request = LLMRequest {
        model: "claude-sonnet-5".to_string(),
        messages: history.clone().into(),
        ..Default::default()
    };
    let mut breakpoints_remaining = 0usize;

    let error = build_messages(
        &request,
        &history,
        None,
        &AnthropicPromptCacheSettings::default(),
        &mut breakpoints_remaining,
        "",
    )
    .expect_err("signed compaction after old messages must be rejected");
    assert!(error.to_string().contains("first Anthropic message"));
}

#[test]
fn content_blocks_from_message_content_maps_file_id_to_container_upload() {
    let blocks = content_blocks_from_message_content(
        &MessageContent::Parts(vec![ContentPart::file_from_id("file_abc123".to_string())]),
        None,
        true,
    );

    assert!(matches!(
        &blocks[0],
        AnthropicContentBlock::ContainerUpload { file_id } if file_id == "file_abc123"
    ));
}

#[test]
fn content_blocks_from_message_content_keeps_file_id_as_fallback_without_code_execution() {
    let blocks = content_blocks_from_message_content(
        &MessageContent::Parts(vec![ContentPart::file_from_id("file_abc123".to_string())]),
        None,
        false,
    );

    assert!(matches!(
        &blocks[0],
        AnthropicContentBlock::Text { text, .. }
            if text == "[File input not directly supported: file_abc123]"
    ));
}

#[test]
fn build_advisor_blocks_re_emits_preserved_advisor_blocks() {
    // `reasoning_details` stores entries as stringified JSON (`Value::String`),
    // exactly as `parse_response`/`create_stream` emit them. The builder must
    // normalize and round-trip the advisor `server_tool_use` + `advisor_tool_result`.
    let detail = json!({
        "type": "advisor",
        "blocks": [
            {
                "type": "server_tool_use",
                "id": "srvtoolu_01",
                "name": "advisor",
                "input": {}
            },
            {
                "type": "advisor_tool_result",
                "tool_use_id": "srvtoolu_01",
                "content": {"type": "advisor_result", "advisor_result": "do X"}
            }
        ]
    })
    .to_string();

    let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!(detail)]));

    let blocks = build_advisor_blocks(&message);
    assert_eq!(blocks.len(), 2);
    assert!(matches!(
        &blocks[0],
        AnthropicContentBlock::ServerToolUse { id, name, .. }
            if id == "srvtoolu_01" && name == "advisor"
    ));
    assert!(matches!(
        &blocks[1],
        AnthropicContentBlock::AdvisorToolResult { tool_use_id, .. }
            if tool_use_id == "srvtoolu_01"
    ));
}

#[test]
fn build_advisor_blocks_skips_non_advisor_details() {
    let message = Message::assistant(String::new())
        .with_reasoning_details(Some(vec![json!(r#"{"type":"thinking","thinking":"trace"}"#)]));
    assert!(build_advisor_blocks(&message).is_empty());
}

fn strip_message_anchors(messages: &mut [super::AnthropicMessage]) {
    for message in messages.iter_mut() {
        for block in message.content.iter_mut() {
            if let AnthropicContentBlock::Text { cache_control, .. } = block {
                *cache_control = None;
            }
        }
    }
}

#[test]
fn grown_history_keeps_wire_prefix_stable_across_turns() {
    // Asymmetric guard for prefix caching: a grown second turn (new long
    // user message plus a tool round) must serialize the shared prefix
    // byte-identically once rolling anchors are factored out. Anchor
    // movement is expected; content rewrites are not.
    use crate::provider::ToolCall;

    let first_turn = vec![
        Message::user("a".repeat(300)),
        Message::assistant_with_tools(
            String::new(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{\"command\":\"ls\"}".to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "a.txt".to_string()),
    ];
    let mut second_turn = first_turn.clone();
    second_turn.push(Message::user("b".repeat(300)));

    let settings = AnthropicPromptCacheSettings {
        min_message_length_for_cache: 1,
        ..AnthropicPromptCacheSettings::default()
    };
    let cache_control = Some(CacheControl {
        control_type: "ephemeral".into(),
        ttl: Some("5m".into()),
    });
    let build = |history: &[Message]| {
        let request = LLMRequest::default();
        let mut breakpoints_remaining = 4usize;
        build_messages(&request, history, cache_control.as_ref(), &settings, &mut breakpoints_remaining, "")
            .expect("build_messages")
    };

    let mut first = build(&first_turn);
    let mut second = build(&second_turn);
    strip_message_anchors(&mut first);
    strip_message_anchors(&mut second);

    let first_json = serde_json::to_value(&first).expect("serialize first turn");
    let second_json = serde_json::to_value(&second).expect("serialize second turn");
    assert!(second_json.as_array().expect("array").len() > first_json.as_array().expect("array").len());
    assert_eq!(
        &second_json.as_array().expect("array")[..first_json.as_array().expect("array").len()],
        first_json.as_array().expect("array"),
        "grown history must extend the wire prefix without rewriting it"
    );
}
