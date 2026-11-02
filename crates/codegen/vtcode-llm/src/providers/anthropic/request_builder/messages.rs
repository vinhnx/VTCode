use hashbrown::HashSet;

use crate::error_display;
use crate::provider::{ContentPart, LLMError, LLMRequest, Message, MessageContent, MessageRole};
use crate::providers::anthropic::block_order::{AssistantBlockParts, assemble_assistant_blocks};
use crate::providers::anthropic::capabilities::supports_mid_conversation_system_messages;
use crate::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicMessage, AnthropicToolResultBlock, AnthropicToolUseBlock, CacheControl, ImageSource,
};
use crate::providers::common::normalize_reasoning_detail_object;
use serde_json::{Value, json};
use tracing::warn;
use vtcode_config::core::AnthropicPromptCacheSettings;

pub(crate) fn hoist_largest_user_message(messages: &mut Vec<Message>) {
    let mut max_len = 0;
    let mut max_idx = None;

    for (i, msg) in messages.iter().enumerate() {
        if msg.role == MessageRole::User {
            let len = msg.content.as_text().len();
            if len > max_len {
                max_len = len;
                max_idx = Some(i);
            }
        }
    }

    if let Some(idx) = max_idx
        && idx > messages.iter().position(has_compaction_block).map_or(0, |index| index + 1)
    {
        let msg = messages.remove(idx);
        let insert_at = messages.iter().position(has_compaction_block).map_or(0, |index| index + 1);
        messages.insert(insert_at, msg);
    }
}

fn has_compaction_block(message: &Message) -> bool {
    message.role == MessageRole::Assistant
        && message.reasoning_details.as_ref().is_some_and(|details| {
            details.iter().any(|detail| {
                normalize_reasoning_detail_object(detail)
                    .and_then(|detail| detail.get("type").and_then(Value::as_str).map(str::to_owned))
                    .as_deref()
                    == Some("compaction")
            })
        })
}

pub(crate) fn build_messages(
    request: &LLMRequest,
    messages_to_process: &[Message],
    messages_cache_control: Option<&CacheControl>,
    prompt_cache_settings: &AnthropicPromptCacheSettings,
    breakpoints_remaining: &mut usize,
    default_model: &str,
) -> Result<Vec<AnthropicMessage>, LLMError> {
    let mut messages = Vec::with_capacity(messages_to_process.len());
    let mut tool_use_ids = HashSet::new();
    let mut compaction_seen = false;
    let allow_mid_conversation_system = supports_mid_conversation_system_messages(&request.model, default_model);
    let allow_container_uploads = request
        .tools
        .as_ref()
        .is_some_and(|tools| tools.iter().any(|tool| tool.is_anthropic_code_execution()));

    // Rolling-anchor strategy: build all messages first without breakpoints,
    // then place cache_control on only the last two qualifying user messages.
    // This matches the article's recommendation: "a pair of rolling anchors on
    // the two most recent cacheable messages" where the second anchor is a safety
    // net that preserves cache coverage when the primary anchor misses.
    for msg in messages_to_process {
        if msg.role == MessageRole::System && !allow_mid_conversation_system {
            continue;
        }

        let mut blocks = Vec::new();

        match msg.role {
            MessageRole::Assistant => {
                if let Some(tool_calls) = &msg.tool_calls {
                    for call in tool_calls {
                        tool_use_ids.insert(call.id.clone());
                    }
                }

                let (compaction_blocks, has_signed_compaction) = build_compaction_blocks(msg)?;
                if !compaction_blocks.is_empty() {
                    if compaction_seen || compaction_blocks.len() > 1 {
                        let formatted_error = error_display::format_llm_error(
                            "Anthropic",
                            "A request may contain at most one compaction block",
                        );
                        return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
                    }
                    if has_signed_compaction && !messages.is_empty() {
                        let formatted_error = error_display::format_llm_error(
                            "Anthropic",
                            "A signed compaction block must be the first Anthropic message",
                        );
                        return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
                    }
                    compaction_seen = true;
                }

                // The compaction-history builder removes thinking blocks from
                // the pre-compaction continuity tail. Keep replaying thinking
                // here so responses generated after that boundary are not
                // accidentally dropped on every later request. Blocks are
                // replayed in the order the model produced them when the
                // response recorded it (interleaved thinking and text).
                blocks.extend(assemble_assistant_blocks(
                    msg,
                    AssistantBlockParts {
                        compaction: compaction_blocks,
                        reasoning: build_reasoning_blocks(msg),
                        content: content_blocks_from_message_content(&msg.content, None, allow_container_uploads),
                        advisor: build_advisor_blocks(msg),
                        tool_use: build_tool_use_blocks(msg),
                    },
                ));

                if blocks.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: String::new(),
                        citations: None,
                        cache_control: None,
                    });
                }
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: blocks,
                    clear_at: None,
                });
            }
            MessageRole::Tool => {
                if let Some(tool_call_id) = &msg.tool_call_id
                    && tool_use_ids.contains(tool_call_id)
                {
                    let tool_content_blocks = tool_result_blocks(msg.content.as_text().as_ref());
                    let content_val = if tool_content_blocks.len() == 1 && tool_content_blocks[0]["type"] == "text" {
                        json!(tool_content_blocks[0]["text"])
                    } else {
                        json!(tool_content_blocks)
                    };

                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::ToolResult(Box::new(AnthropicToolResultBlock {
                            tool_use_id: tool_call_id.clone(),
                            content: content_val,
                            is_error: None,
                            cache_control: None,
                        }))],
                        clear_at: None,
                    });
                } else if !msg.content.is_empty() {
                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::Text {
                            text: msg.content.as_text().to_string(),
                            citations: None,
                            cache_control: None,
                        }],
                        clear_at: None,
                    });
                }
            }
            MessageRole::System | MessageRole::User => {
                let blocks = content_blocks_from_message_content(&msg.content, None, allow_container_uploads);
                if blocks.is_empty() {
                    continue;
                }

                messages.push(AnthropicMessage {
                    role: msg.role.as_anthropic_str().to_string(),
                    content: blocks,
                    clear_at: msg.clear_at,
                });
            }
        }
    }

    // Rolling-anchor placement: identify qualifying user messages and place
    // breakpoints on the last two (primary + safety net anchor).
    if prompt_cache_settings.cache_user_messages
        && let Some(cc) = messages_cache_control
    {
        let qualifying: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(_, msg)| {
                msg.role == "user"
                    && msg.content.iter().any(|block| {
                        matches!(
                            block,
                            AnthropicContentBlock::Text { text, .. }
                                if text.len() >= prompt_cache_settings.min_message_length_for_cache
                        )
                    })
            })
            .map(|(idx, _)| idx)
            .collect();

        let anchor_count = qualifying.len().min(2);
        for &idx in qualifying.iter().rev().take(anchor_count) {
            if *breakpoints_remaining == 0 {
                break;
            }
            if let Some(AnthropicContentBlock::Text { cache_control, .. }) =
                messages[idx].content.iter_mut().find(|block| {
                    matches!(block, AnthropicContentBlock::Text { text, .. }
                    if text.len() >= prompt_cache_settings.min_message_length_for_cache)
                })
            {
                *cache_control = Some(cc.clone());
                *breakpoints_remaining -= 1;
            }
        }
    }

    if messages.is_empty() {
        let formatted_error =
            error_display::format_llm_error("Anthropic", "No convertible messages for Anthropic request");
        return Err(LLMError::InvalidRequest { message: formatted_error, metadata: None });
    }

    Ok(messages)
}

/// Re-emits preserved advisor server_tool_use + advisor_tool_result blocks from a
/// previous turn. The blocks are stored verbatim in `reasoning_details` under the
/// `advisor` type so they round-trip without being re-dispatched locally.
fn build_advisor_blocks(msg: &Message) -> Vec<AnthropicContentBlock> {
    let Some(details) = &msg.reasoning_details else {
        return Vec::new();
    };

    let mut blocks = Vec::new();
    for detail in details {
        // `reasoning_details` may store entries as stringified JSON (`Value::String`),
        // so normalize first — mirroring `build_reasoning_blocks`.
        let Some(normalized) = normalize_reasoning_detail_object(detail) else {
            continue;
        };
        if normalized.get("type").and_then(|t| t.as_str()) != Some("advisor") {
            continue;
        }
        let Some(stored) = normalized.get("blocks").and_then(|b| b.as_array()) else {
            continue;
        };
        for block in stored {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("server_tool_use") => {
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    blocks.push(AnthropicContentBlock::ServerToolUse { id, name, input });
                }
                Some("advisor_tool_result") => {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let content = block.get("content").cloned().unwrap_or_else(|| json!({}));
                    blocks.push(AnthropicContentBlock::AdvisorToolResult { tool_use_id, content });
                }
                _ => {}
            }
        }
    }
    blocks
}

fn build_compaction_blocks(msg: &Message) -> Result<(Vec<AnthropicContentBlock>, bool), LLMError> {
    let Some(details) = &msg.reasoning_details else {
        return Ok((Vec::new(), false));
    };

    let mut blocks = Vec::new();
    let mut has_signed_compaction = false;
    for detail in details {
        let Some(normalized) = normalize_reasoning_detail_object(detail) else {
            continue;
        };
        if normalized.get("type").and_then(|value| value.as_str()) != Some("compaction") {
            continue;
        }

        let block = serde_json::from_value::<AnthropicContentBlock>(normalized).map_err(|error| {
            let formatted_error = error_display::format_llm_error(
                "Anthropic",
                &format!("Invalid compaction block in conversation history: {error}"),
            );
            LLMError::InvalidRequest { message: formatted_error, metadata: None }
        })?;
        if let AnthropicContentBlock::Compaction { signature, .. } = &block {
            has_signed_compaction |= signature.as_deref().is_some_and(|value| !value.trim().is_empty());
        }
        blocks.push(block);
    }

    Ok((blocks, has_signed_compaction))
}

fn build_reasoning_blocks(msg: &Message) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::with_capacity(msg.reasoning_details.as_ref().map_or(0, |d| d.len()));

    if let Some(details) = &msg.reasoning_details {
        for detail in details {
            let Some(normalized) = normalize_reasoning_detail_object(detail) else {
                continue;
            };

            if normalized.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                let thinking = normalized.get("thinking").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let signature = normalized
                    .get("signature")
                    .and_then(|t| t.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                if !thinking.is_empty() || signature.is_some() {
                    blocks.push(AnthropicContentBlock::Thinking { thinking, signature, cache_control: None });
                }
            } else if normalized.get("type").and_then(|t| t.as_str()) == Some("redacted_thinking") {
                let data = normalized.get("data").and_then(|d| d.as_str()).unwrap_or("").to_string();
                blocks.push(AnthropicContentBlock::RedactedThinking { data, cache_control: None });
            }
        }
    }

    blocks
}

fn content_blocks_from_message_content(
    content: &MessageContent,
    cache_control: Option<CacheControl>,
    allow_container_uploads: bool,
) -> Vec<AnthropicContentBlock> {
    let capacity = match content {
        MessageContent::Text(_) => 1,
        MessageContent::Parts(parts) => parts.len(),
    };
    let mut blocks = Vec::with_capacity(capacity);
    let mut cache_used = false;

    match content {
        MessageContent::Text(text) => {
            if !text.is_empty() {
                blocks.push(AnthropicContentBlock::Text { text: text.clone(), citations: None, cache_control });
            }
        }
        MessageContent::Parts(parts) => {
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if text.is_empty() {
                            continue;
                        }
                        let control = if !cache_used { cache_control.clone() } else { None };
                        cache_used = true;
                        blocks.push(AnthropicContentBlock::Text {
                            text: text.clone(),
                            citations: None,
                            cache_control: control,
                        });
                    }
                    ContentPart::Image { data, mime_type, .. } => {
                        blocks.push(AnthropicContentBlock::Image {
                            source: ImageSource {
                                source_type: "base64".to_owned(),
                                media_type: mime_type.clone(),
                                data: data.clone(),
                            },
                            cache_control: None,
                        });
                    }
                    ContentPart::File { filename, file_id, file_url, .. } => {
                        if allow_container_uploads && let Some(file_id) = file_id {
                            blocks.push(AnthropicContentBlock::ContainerUpload { file_id: file_id.clone() });
                            continue;
                        }

                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        blocks.push(AnthropicContentBlock::Text {
                            text: format!("[File input not directly supported: {fallback}]"),
                            citations: None,
                            cache_control: None,
                        });
                    }
                }
            }
        }
    }

    blocks
}

fn build_tool_use_blocks(msg: &Message) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::with_capacity(msg.tool_calls.as_ref().map_or(0, |tc| tc.len()));

    if let Some(tool_calls) = &msg.tool_calls {
        for call in tool_calls {
            if let Some(ref func) = call.function {
                let args: Value = call.parsed_arguments().unwrap_or_else(|_| json!({}));
                blocks.push(AnthropicContentBlock::ToolUse(Box::new(AnthropicToolUseBlock {
                    id: call.id.clone(),
                    name: func.name.clone(),
                    input: args,
                    cache_control: None,
                })));
            }
        }
    }

    blocks
}

pub fn tool_result_blocks(content: &str) -> Vec<Value> {
    if content.trim().is_empty() {
        return vec![json!({"type": "text", "text": ""})];
    }

    if let Ok(parsed) = serde_json::from_str::<Value>(content) {
        let text = match parsed {
            Value::String(text) => text,
            other => serde_json::to_string(&other).unwrap_or_else(|_| "{}".to_string()),
        };
        vec![json!({"type": "text", "text": text})]
    } else {
        vec![json!({"type": "text", "text": content})]
    }
}

#[cfg(test)]
mod tests {
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

        let messages = build_messages(
            &request,
            &source_messages,
            cache_control.as_ref(),
            &settings,
            &mut breakpoints_remaining,
            "",
        )
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

        let messages = build_messages(
            &request,
            &source_messages,
            cache_control.as_ref(),
            &settings,
            &mut breakpoints_remaining,
            "",
        )
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

        let messages = build_messages(
            &request,
            &source_messages,
            cache_control.as_ref(),
            &settings,
            &mut breakpoints_remaining,
            "",
        )
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

        let messages = build_messages(
            &request,
            &source_messages,
            cache_control.as_ref(),
            &settings,
            &mut breakpoints_remaining,
            "",
        )
        .expect("build_messages");

        // Both long messages qualify (default threshold is 256 chars); the short
        // middle message never receives an anchor.
        assert_eq!(message_anchor_flags(&messages), vec![true, false, true]);
        assert_eq!(breakpoints_remaining, 2);
    }

    #[test]
    fn build_reasoning_blocks_decodes_stringified_json_detail() {
        let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!(
            r#"{"type":"thinking","thinking":"trace","signature":"sig_123"}"#
        )]));

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
        let message = Message::assistant(String::new()).with_reasoning_details(Some(vec![json!(
            r#"{"type":"thinking","thinking":"","signature":"sig_omitted"}"#
        )]));

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
        assert!(
            matches!(messages[1].content[1], AnthropicContentBlock::Text { ref text, .. } if text == "continuation")
        );
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
}
