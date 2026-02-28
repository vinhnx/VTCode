use crate::config::core::AnthropicPromptCacheSettings;
use crate::llm::error_display;
use crate::llm::provider::{
    ContentPart, LLMError, LLMRequest, Message, MessageContent, MessageRole,
};
use crate::llm::providers::anthropic_types::{
    AnthropicContentBlock, AnthropicMessage, AnthropicToolResultBlock, AnthropicToolUseBlock,
    CacheControl, ImageSource,
};
use serde_json::{Value, json};
use std::collections::HashSet;

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

    for msg in messages_to_process {
        if msg.role == MessageRole::System {
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

                blocks.extend(content_blocks_from_message_content(&msg.content, None));

                blocks.extend(build_tool_use_blocks(msg));

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
                });
            }
            MessageRole::Tool => {
                if let Some(tool_call_id) = &msg.tool_call_id
                    && tool_use_ids.contains(tool_call_id)
                {
                    let tool_content_blocks = tool_result_blocks(msg.content.as_text().as_ref());
                    let content_val = if tool_content_blocks.len() == 1
                        && tool_content_blocks[0]["type"] == "text"
                    {
                        json!(tool_content_blocks[0]["text"])
                    } else {
                        json!(tool_content_blocks)
                    };

                    messages.push(AnthropicMessage {
                        role: "user".to_string(),
                        content: vec![AnthropicContentBlock::ToolResult(Box::new(
                            AnthropicToolResultBlock {
                                tool_use_id: tool_call_id.clone(),
                                content: content_val,
                                is_error: None,
                                cache_control: None,
                            },
                        ))],
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
                let mut cache_ctrl = None;
                let should_cache = msg.role == MessageRole::User
                    && prompt_cache_settings.cache_user_messages
                    && *breakpoints_remaining > 0
                    && msg.content.as_text().len()
                        >= prompt_cache_settings.min_message_length_for_cache;

                if should_cache && let Some(cc) = messages_cache_control.as_ref() {
                    cache_ctrl = Some(cc.clone());
                    *breakpoints_remaining -= 1;
                }

                let blocks = content_blocks_from_message_content(&msg.content, cache_ctrl);
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

    add_prefill_message(request, &mut messages);

    if messages.is_empty() {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "No convertible messages for Anthropic request",
        );
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    Ok(messages)
}

fn build_reasoning_blocks(msg: &Message) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::new();

    if let Some(details) = &msg.reasoning_details {
        for detail in details {
            if detail.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                let thinking = detail
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let signature = detail
                    .get("signature")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                if !thinking.is_empty() && !signature.is_empty() {
                    blocks.push(AnthropicContentBlock::Thinking {
                        thinking,
                        signature,
                        cache_control: None,
                    });
                }
            } else if detail.get("type").and_then(|t| t.as_str()) == Some("redacted_thinking") {
                let data = detail
                    .get("data")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                blocks.push(AnthropicContentBlock::RedactedThinking {
                    data,
                    cache_control: None,
                });
            }
        }
    }

    blocks
}

fn content_blocks_from_message_content(
    content: &MessageContent,
    cache_control: Option<CacheControl>,
) -> Vec<AnthropicContentBlock> {
    let mut blocks = Vec::new();
    let mut cache_used = false;

    match content {
        MessageContent::Text(text) => {
            if !text.is_empty() {
                blocks.push(AnthropicContentBlock::Text {
                    text: text.clone(),
                    citations: None,
                    cache_control,
                });
            }
        }
        MessageContent::Parts(parts) => {
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        if text.is_empty() {
                            continue;
                        }
                        let control = if !cache_used {
                            cache_control.clone()
                        } else {
                            None
                        };
                        cache_used = true;
                        blocks.push(AnthropicContentBlock::Text {
                            text: text.clone(),
                            citations: None,
                            cache_control: control,
                        });
                    }
                    ContentPart::Image {
                        data, mime_type, ..
                    } => {
                        blocks.push(AnthropicContentBlock::Image {
                            source: ImageSource {
                                source_type: "base64".to_owned(),
                                media_type: mime_type.clone(),
                                data: data.clone(),
                            },
                            cache_control: None,
                        });
                    }
                    ContentPart::File {
                        filename,
                        file_id,
                        file_url,
                        ..
                    } => {
                        let fallback = filename
                            .clone()
                            .or_else(|| file_id.clone())
                            .or_else(|| file_url.clone())
                            .unwrap_or_else(|| "attached file".to_string());
                        blocks.push(AnthropicContentBlock::Text {
                            text: format!("[File input not directly supported: {}]", fallback),
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
    let mut blocks = Vec::new();

    if let Some(tool_calls) = &msg.tool_calls {
        for call in tool_calls {
            if let Some(ref func) = call.function {
                let args: Value =
                    serde_json::from_str(&func.arguments).unwrap_or_else(|_| json!({}));
                blocks.push(AnthropicContentBlock::ToolUse(Box::new(
                    AnthropicToolUseBlock {
                        id: call.id.clone(),
                        name: func.name.clone(),
                        input: args,
                        cache_control: None,
                    },
                )));
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
            let tag = format!("[{}]", name);
            if !text.contains(&tag) {
                text = format!("{} {}", tag, text).trim().to_string();
            }
        }
        if !text.is_empty() {
            messages.push(AnthropicMessage {
                role: "assistant".to_string(),
                content: vec![AnthropicContentBlock::Text {
                    text,
                    citations: None,
                    cache_control: None,
                }],
            });
        }
    } else if request.character_reinforcement
        && let Some(name) = &request.character_name
    {
        messages.push(AnthropicMessage {
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock::Text {
                text: format!("[{}]", name),
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
