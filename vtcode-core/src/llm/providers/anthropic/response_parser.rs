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

    let block_count = content.len();
    let mut text_parts = Vec::with_capacity(block_count);
    let mut reasoning_parts = Vec::with_capacity(block_count);
    let mut tool_calls = Vec::new();
    let mut reasoning_details_vec = Vec::with_capacity(block_count);
    let mut tool_references = Vec::new();
    let mut compaction: Option<String> = None;

    for block in content {
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").and_then(|t| t.as_str()) {
                    let thinking = thinking.to_string();
                    let mut detail = json!({
                        "type": "thinking",
                        "thinking": thinking,
                    });
                    if let Some(signature) = block.get("signature").and_then(|value| value.as_str())
                        && let Some(obj) = detail.as_object_mut()
                    {
                        obj.insert(
                            "signature".to_string(),
                            Value::String(signature.to_string()),
                        );
                    }
                    reasoning_details_vec.push(detail.to_string());
                    reasoning_parts.push(thinking);
                }
            }
            Some("redacted_thinking") => {
                reasoning_details_vec.push(
                    json!({
                        "type": "redacted_thinking",
                        "data": block
                            .get("data")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default(),
                    })
                    .to_string(),
                );
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
            Some("compaction") => {
                compaction = block
                    .get("content")
                    .and_then(|t| t.as_str())
                    .map(|s| s.to_string());
            }
            Some("fallback") => {
                // Fallback content block marks model boundary - preserve for conversation continuity
                if let Some(from) = block
                    .get("from")
                    .and_then(|v| v.get("model").and_then(|m| m.as_str()))
                    && let Some(to) = block
                        .get("to")
                        .and_then(|v| v.get("model").and_then(|m| m.as_str()))
                {
                    let detail = json!({
                        "type": "fallback",
                        "from": { "model": from },
                        "to": { "model": to },
                    });
                    reasoning_details_vec.push(detail.to_string());
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

    // Parse stop_details for refusal/fallback credit information
    if let Some(sd) = response_json.get("stop_details") {
        let category = sd.get("category").and_then(|c| c.as_str()).unwrap_or("");
        let explanation = sd.get("explanation").and_then(|e| e.as_str()).unwrap_or("");
        let credit_token = sd
            .get("fallback_credit_token")
            .and_then(|t| t.as_str())
            .unwrap_or("");
        let has_prefill = sd
            .get("fallback_has_prefill_claim")
            .and_then(|v| v.as_bool());
        let mut detail = json!({
            "type": "stop_details",
            "category": category,
        });
        if !explanation.is_empty() {
            detail["explanation"] = Value::String(explanation.to_string());
        }
        if !credit_token.is_empty() {
            detail["fallback_credit_token"] = Value::String(credit_token.to_string());
        }
        if let Some(prefill) = has_prefill {
            detail["fallback_has_prefill_claim"] = Value::Bool(prefill);
        }
        reasoning_details_vec.push(detail.to_string());
    }

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
        compaction,
    })
}

pub fn parse_finish_reason(stop_reason: &str) -> FinishReason {
    match stop_reason {
        "end_turn" => FinishReason::Stop,
        "max_tokens" => FinishReason::Length,
        "model_context_window_exceeded" => FinishReason::Length,
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

    // Parse iterations for fallback tracking
    let iterations = usage_value
        .get("iterations")
        .and_then(|iters| iters.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|iter| {
                    let iter_type = iter.get("type").and_then(|t| t.as_str());
                    let input_tokens = iter
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let output_tokens = iter
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let cache_creation = iter
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    let cache_read = iter
                        .get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32);
                    let model = iter.get("model").and_then(|v| v.as_str()).unwrap_or("");

                    Some(json!({
                        "type": iter_type,
                        "model": model,
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens,
                        "cache_creation_input_tokens": cache_creation,
                        "cache_read_input_tokens": cache_read,
                    }))
                })
                .collect::<Vec<_>>()
        });

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
        iterations,
    }
}
