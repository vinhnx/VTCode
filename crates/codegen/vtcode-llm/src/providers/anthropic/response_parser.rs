//! Response parsing for Anthropic Claude API
//!
//! Converts Anthropic API JSON responses into internal LLMResponse format,
//! handling:
//! - Text content extraction
//! - Thinking/reasoning blocks
//! - Tool use responses
//! - Usage statistics
//! - Finish reasons

use crate::error_display;
use crate::provider::{FinishReason, LLMError, LLMResponse, ToolCall, Usage};
use crate::providers::extract_reasoning_trace;
use hashbrown::HashSet;
use serde_json::{Value, json};

use super::block_order::{BlockOrderRecorder, BlockSlot};

pub fn parse_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
    let content = response_json.get("content").and_then(|c| c.as_array()).ok_or_else(|| {
        let formatted = error_display::format_llm_error("Anthropic", "Invalid response format: missing content");
        LLMError::Provider { message: formatted, metadata: None }
    })?;

    let block_count = content.len();
    let mut text_parts = Vec::with_capacity(block_count);
    let mut reasoning_parts = Vec::with_capacity(block_count);
    let mut tool_calls = Vec::new();
    let mut reasoning_details_vec = Vec::with_capacity(block_count);
    let mut tool_references = Vec::new();
    let mut compaction: Option<String> = None;
    // Raw advisor server_tool_use + advisor_tool_result blocks, preserved verbatim
    // for faithful round-trip on subsequent turns (transport: reasoning_details).
    let mut advisor_blocks: Vec<Value> = Vec::new();
    // Interleaved blocks (thinking between text and tool use) must be
    // replayed in this order; see `block_order`.
    let mut block_order = BlockOrderRecorder::default();
    let declined = declined_partial_positions(content);

    for (position, block) in content.iter().enumerate() {
        if declined[position] {
            continue;
        }
        match block.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    block_order.push(BlockSlot::Text { len: text.len() });
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
                    let signature = block.get("signature").and_then(|value| value.as_str());
                    // Replay skips thinking blocks without text or signature.
                    if !thinking.is_empty() || signature.is_some_and(|value| !value.trim().is_empty()) {
                        block_order.push(BlockSlot::Reasoning);
                    }
                    if let Some(signature) = signature
                        && let Some(obj) = detail.as_object_mut()
                    {
                        obj.insert("signature".to_string(), Value::String(signature.to_string()));
                    }
                    reasoning_details_vec.push(detail.to_string());
                    reasoning_parts.push(thinking);
                }
            }
            Some("redacted_thinking") => {
                block_order.push(BlockSlot::Reasoning);
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
                let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();

                if name == "structured_output" {
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let output_text = serde_json::to_string(&input).unwrap_or_else(|_| "{{}}".to_string());
                    block_order.push(BlockSlot::Text { len: output_text.len() });
                    text_parts.push(output_text);
                } else {
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{{}}".to_string());
                    if !id.is_empty() && !name.is_empty() {
                        block_order.push(BlockSlot::ToolUse { id: id.clone() });
                        tool_calls.push(ToolCall::function(id, name, arguments));
                    }
                }
            }
            Some("server_tool_use") => {
                // The advisor tool is the only supported server-side tool. Preserve
                // the block verbatim so it can be round-tripped on the next turn.
                if block.get("name").and_then(|n| n.as_str()).is_some_and(|name| name == "advisor") {
                    block_order.push(BlockSlot::Advisor);
                    advisor_blocks.push(block.clone());
                }
            }
            Some("advisor_tool_result") => {
                // Preserve the advisor result verbatim (advisor_result,
                // advisor_redacted_result, or advisor_tool_result_error).
                block_order.push(BlockSlot::Advisor);
                advisor_blocks.push(block.clone());
            }
            Some("tool_search_tool_result") => {
                if let Some(content_block) = block.get("content")
                    && content_block.get("type").and_then(|t| t.as_str()) == Some("tool_search_tool_search_result")
                    && let Some(refs) = content_block.get("tool_references").and_then(|r| r.as_array())
                {
                    for tool_ref in refs {
                        if let Some(tool_name) = tool_ref.get("tool_name").and_then(|n| n.as_str()) {
                            tool_references.push(tool_name.to_string());
                        }
                    }
                }
            }
            Some("compaction") => {
                // Keep the complete opaque block for a faithful replay. The
                // public `compaction` field is only the usable summary text;
                // signatures, cache controls, and provider extensions belong
                // in reasoning_details so the next request can send them back.
                compaction = block.get("content").and_then(|t| t.as_str()).map(str::to_owned);
                block_order.push(BlockSlot::Compaction);
                reasoning_details_vec.push(block.to_string());
            }
            Some("fallback") => {
                // Fallback content block marks model boundary - preserve for conversation continuity
                if let Some(from) = block.get("from").and_then(|v| v.get("model").and_then(|m| m.as_str()))
                    && let Some(to) = block.get("to").and_then(|v| v.get("model").and_then(|m| m.as_str()))
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
        response_json.get("reasoning").and_then(extract_reasoning_trace)
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

    if let Some(detail) = response_json.get("stop_details").and_then(stop_details_reasoning_detail) {
        reasoning_details_vec.push(detail);
    }

    let usage = response_json.get("usage").map(parse_usage);

    if !advisor_blocks.is_empty() {
        let detail = json!({
            "type": "advisor",
            "blocks": advisor_blocks,
        });
        reasoning_details_vec.push(detail.to_string());
    }

    if let Some(detail) = block_order.into_detail() {
        reasoning_details_vec.push(detail);
    }

    super::speed::log_served_speed(&model, response_json.get("usage").and_then(super::speed::parse_served_speed));
    Ok(LLMResponse {
        content: if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.into_iter().collect())
        },
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
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

/// Marks the blocks a refused model produced before the final `fallback`
/// block of a mid-output server-side fallback. Echoing them back is invalid:
/// thinking, redacted thinking, tool use, a `server_tool_use` without its
/// result, and unrecognized model-internal blocks are dropped, while text,
/// compaction, paired server-tool blocks, and everything after the boundary
/// are kept. The API already omits the declined partial from non-streaming
/// responses, so this only enforces the echo rule if one ever appears.
fn declined_partial_positions(content: &[Value]) -> Vec<bool> {
    fn block_type(block: &Value) -> Option<&str> {
        block.get("type").and_then(Value::as_str)
    }
    let mut declined = vec![false; content.len()];
    let Some(boundary) = content.iter().rposition(|block| block_type(block) == Some("fallback")) else {
        return declined;
    };
    let paired_tool_use_ids: HashSet<&str> = content
        .iter()
        .filter(|block| block_type(block).is_some_and(|kind| kind.ends_with("_tool_result")))
        .filter_map(|block| block.get("tool_use_id").and_then(Value::as_str))
        .collect();
    for (flag, block) in declined.iter_mut().zip(&content[..boundary]) {
        *flag = match block_type(block) {
            Some("text" | "compaction" | "fallback") => false,
            Some("server_tool_use") => !block
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| paired_tool_use_ids.contains(id)),
            Some(kind) => !kind.ends_with("_tool_result"),
            None => true,
        };
    }
    declined
}

/// Serializes a response's `stop_details` (refusal category, explanation,
/// fallback-credit fields and `recommended_model`) into a `reasoning_details`
/// entry. Shared by the
/// non-streaming parser and the stream decoder, which receives the same object
/// on `message_delta`. Returns `None` for an absent or `null` value.
pub(crate) fn stop_details_reasoning_detail(stop_details: &Value) -> Option<String> {
    let sd = stop_details.as_object()?;
    let category = sd.get("category").and_then(Value::as_str).unwrap_or("");
    let explanation = sd.get("explanation").and_then(Value::as_str).unwrap_or("");
    let credit_token = sd.get("fallback_credit_token").and_then(Value::as_str).unwrap_or("");
    let has_prefill = sd.get("fallback_has_prefill_claim").and_then(Value::as_bool);
    // Present only when a server-side fallback attempt could not run (rate
    // limited or overloaded); a direct retry on that model may succeed.
    let recommended_model = sd.get("recommended_model").and_then(Value::as_str).map(str::trim).unwrap_or("");
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
    if !recommended_model.is_empty() {
        detail["recommended_model"] = Value::String(recommended_model.to_string());
    }
    Some(detail.to_string())
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

#[allow(
    clippy::unnecessary_filter_map,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // non-object array elements produce None iter_type but are still collected
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
    let iterations = usage_value.get("iterations").and_then(|iters| iters.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|iter| {
                let iter_type = iter.get("type").and_then(|t| t.as_str());
                let input_tokens = iter.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let output_tokens = iter.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let cache_creation = iter
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let cache_read = iter.get("cache_read_input_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
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
        prompt_tokens: usage_value.get("input_tokens").and_then(|it| it.as_u64()).unwrap_or(0) as u32,
        completion_tokens: usage_value.get("output_tokens").and_then(|ot| ot.as_u64()).unwrap_or(0) as u32,
        total_tokens: (usage_value.get("input_tokens").and_then(|it| it.as_u64()).unwrap_or(0)
            + usage_value.get("output_tokens").and_then(|ot| ot.as_u64()).unwrap_or(0)) as u32,
        cached_prompt_tokens: cache_read_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        iterations,
    }
}
