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
    let mut pending_tool_use_ids: Vec<String> = Vec::new();
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
                        if tool_use_ids.insert(call.id.clone()) {
                            pending_tool_use_ids.push(call.id.clone());
                        }
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
                    pending_tool_use_ids.retain(|pending| pending != tool_call_id);
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

    // Messages requires every tool_use to be answered by a matching
    // tool_result. An interrupted turn can persist the call without its
    // response; synthesize the aborted result so resumed history replays as
    // a valid request, mirroring the OpenAI builder's convention.
    for call_id in pending_tool_use_ids {
        messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: vec![AnthropicContentBlock::ToolResult(Box::new(AnthropicToolResultBlock {
                tool_use_id: call_id,
                content: json!("aborted"),
                is_error: Some(true),
                cache_control: None,
            }))],
            clear_at: None,
        });
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
                blocks.push(AnthropicContentBlock::ToolUse(Box::new(AnthropicToolUseBlock::function(
                    call.id.clone(),
                    func.name.clone(),
                    args,
                ))));
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
mod tests;
