//! Response parsing for Anthropic Claude API
//!
//! Converts Anthropic API JSON responses into internal LLMResponse format,
//! handling:
//! - Text content extraction
//! - Thinking/reasoning blocks
//! - Tool use responses
//! - Usage statistics
//! - Finish reasons

use crate::llm::error_display;
use crate::llm::provider::{FinishReason, LLMError, LLMResponse, ToolCall, Usage};
use crate::llm::providers::extract_reasoning_trace;
use serde_json::{Value, json};

pub fn parse_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
    let content = response_json
        .get("content")
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            let formatted = error_display::format_llm_error(
                "Anthropic",
                "Invalid response format: missing content",
            );
            LLMError::Provider {
                message: formatted,
                metadata: None,
            }
        })?;

    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut reasoning_details_vec = Vec::new();
    let mut tool_references = Vec::new();

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                    reasoning_details_vec.push(thinking.to_string());
                    reasoning_parts.push(thinking.to_string());
                }
            }
            Some("redacted_thinking") => {
                reasoning_details_vec.push("[REDACTED THINKING]".to_string());
            }
            Some("tool_use") => {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if name == "structured_output" {
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let output_text =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{{}}".to_string());
                    text_parts.push(output_text);
                } else {
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let arguments =
                        serde_json::to_string(&input).unwrap_or_else(|_| "{{}}".to_string());
                    if !id.is_empty() && !name.is_empty() {
                        tool_calls.push(ToolCall::function(id, name, arguments));
                    }
                }
            }
            Some("server_tool_use") => {} // No-op
            Some("tool_search_tool_result") => {
                if let Some(content_block) = block.get("content")
                    && content_block.get("type").and_then(|t| t.as_str())
                        == Some("tool_search_tool_search_result")
                    && let Some(refs) = content_block
                        .get("tool_references")
                        .and_then(|r| r.as_array())
                {
                    for tool_ref in refs {
                        if let Some(tool_name) = tool_ref.get("tool_name").and_then(|n| n.as_str())
                        {
                            tool_references.push(tool_name.to_string());
                        }
                    }
                }
            }
            _ => {} // Ignore unknown block types
        }
    }

    let reasoning = if reasoning_parts.is_empty() {
        response_json
            .get("reasoning")
            .and_then(extract_reasoning_trace)
    } else {
        let joined = reasoning_parts.join("\n");
        let trimmed = joined.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    let stop_reason = response_json
        .get("stop_reason")
        .and_then(|sr| sr.as_str())
        .unwrap_or("end_turn");
    let finish_reason = parse_finish_reason(stop_reason);

    let usage = response_json.get("usage").map(parse_usage);

    Ok(LLMResponse {
        content: if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.into_iter().collect())
        },
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        model,
        usage,
        finish_reason,
        reasoning,
        reasoning_details: if reasoning_details_vec.is_empty() {
            None
        } else {
            Some(reasoning_details_vec)
        },
        tool_references,
        request_id: None,
        organization_id: None,
    })
}

pub fn parse_finish_reason(stop_reason: &str) -> FinishReason {
    match stop_reason {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "stop_sequence" => FinishReason::Stop,
        "tool_use" => FinishReason::ToolCalls,
        "compaction" => FinishReason::Pause,
        "pause_turn" => FinishReason::Pause,
        "refusal" => FinishReason::Refusal,
        other => FinishReason::Error(other.to_string()),
    }
}

pub fn parse_usage(usage_value: &Value) -> Usage {
    let cache_creation_tokens = usage_value
        .get("cache_creation_input_tokens")
        .and_then(|value| value.as_u64())
        .map(|value| value as u32);
    let cache_read_tokens = usage_value
        .get("cache_read_input_tokens")
        .and_then(|value| value.as_u64())
        .map(|value| value as u32);

    Usage {
        prompt_tokens: usage_value
            .get("input_tokens")
            .and_then(|it| it.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: usage_value
            .get("output_tokens")
            .and_then(|ot| ot.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: (usage_value
            .get("input_tokens")
            .and_then(|it| it.as_u64())
            .unwrap_or(0)
            + usage_value
                .get("output_tokens")
                .and_then(|ot| ot.as_u64())
                .unwrap_or(0)) as u32,
        cached_prompt_tokens: cache_read_tokens,
        cache_creation_tokens,
        cache_read_tokens,
    }
}
