use hashbrown::HashSet;

use crate::error_display;
use crate::provider::{ContentPart, LLMError, LLMRequest, Message, MessageContent, MessageRole};
use crate::providers::anthropic::capabilities::supports_mid_conversation_system_messages;
use crate::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicMessage, AnthropicToolResultBlock, AnthropicToolUseBlock, CacheControl, ImageSource,
};
use crate::providers::common::normalize_reasoning_detail_object;
use serde_json::{Value, json};
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
        && idx > 0
    {
        let msg = messages.remove(idx);
        messages.insert(0, msg);
    }
}

pub(crate) fn build_messages(
    request: &LLMRequest,
    messages_to_process: &[Message],
    messages_cache_control: &Option<CacheControl>,
    prompt_cache_settings: &AnthropicPromptCacheSettings,
    breakpoints_remaining: &mut usize,
) -> Result<Vec<AnthropicMessage>, LLMError> {
    let mut messages = Vec::with_capacity(messages_to_process.len());
    let mut tool_use_ids = HashSet::new();
    let allow_mid_conversation_system = supports_mid_conversation_system_messages(&request.model, &request.model);
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
                blocks.extend(build_reasoning_blocks(msg));

                blocks.extend(content_blocks_from_message_content(&msg.content, None, allow_container_uploads));

                blocks.extend(build_advisor_blocks(msg));

                blocks.extend(build_tool_use_blocks(msg));

                if blocks.is_empty() {
                    blocks.push(AnthropicContentBlock::Text {
                        text: String::new(),
                        citations: None,
                        cache_control: None,
                    });
                }
                messages.push(AnthropicMessage { role: "assistant".to_string(), content: blocks });
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
                    });
                } else if !msg.content.is_empty() {
                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::Text {
                            text: msg.content.as_text().to_string(),
                            citations: None,
                            cache_control: None,
                        }],
                    });
                }
            }
            _ => {
                let blocks = content_blocks_from_message_content(&msg.content, None, allow_container_uploads);
                if blocks.is_empty() {
                    continue;
                }

                messages.push(AnthropicMessage {
                    role: msg.role.as_anthropic_str().to_string(),
                    content: blocks,
                });
            }
        }
    }

    // Rolling-anchor placement: identify qualifying user messages and place
    // breakpoints on the last two (primary + safety net anchor).
    if prompt_cache_settings.cache_user_messages
        && let Some(cc) = messages_cache_control.as_ref()
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

    add_prefill_message(request, &mut messages);

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

fn add_prefill_message(request: &LLMRequest, messages: &mut Vec<AnthropicMessage>) {
    let mut prefill_text = String::new();

    if let Some(settings) = &request.coding_agent_settings
        && settings.prefill_thought
    {
        prefill_text.push_str("<thought>");
    }

    if let Some(request_prefill) = &request.prefill {
        if !prefill_text.is_empty() && !request_prefill.is_empty() {
            prefill_text.push(' ');
        }
        prefill_text.push_str(request_prefill);
    }

    if !prefill_text.is_empty() {
        let mut text = prefill_text;
        if request.character_reinforcement
            && let Some(name) = &request.character_name
        {
            let tag = format!("[{name}]");
            if !text.contains(&tag) {
                text = format!("{tag} {text}").trim().to_string();
            }
        }
        if !text.is_empty() {
            messages.push(AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::Text { text, citations: None, cache_control: None }],
            });
        }
    } else if request.character_reinforcement
        && let Some(name) = &request.character_name
    {
        messages.push(AnthropicMessage {
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: format!("[{name}]"),
                citations: None,
                cache_control: None,
            }],
        });
    }
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
    use super::{build_advisor_blocks, build_messages, build_reasoning_blocks, content_blocks_from_message_content};
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
            control_type: "ephemeral".to_string(),
            ttl: Some("5m".to_string()),
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
            build_messages(&request, &source_messages, &cache_control, &settings, &mut breakpoints_remaining)
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
            build_messages(&request, &source_messages, &cache_control, &settings, &mut breakpoints_remaining)
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
            build_messages(&request, &source_messages, &cache_control, &settings, &mut breakpoints_remaining)
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
            control_type: "ephemeral".to_string(),
            ttl: Some("5m".to_string()),
        });
        let settings = AnthropicPromptCacheSettings::default();
        let mut breakpoints_remaining = 4usize;

        let messages =
            build_messages(&request, &source_messages, &cache_control, &settings, &mut breakpoints_remaining)
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
}
