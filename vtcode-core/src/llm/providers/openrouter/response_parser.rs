//! Response parsing for OpenRouter API
//!
//! Converts OpenRouter API JSON responses into internal LLMResponse format.

use crate::llm::error_display;
use crate::llm::provider::{FinishReason, LLMError, LLMResponse, ToolCall, Usage};
use serde_json::Value;

pub fn parse_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
    let choices = response_json
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            let formatted = error_display::format_llm_error(
                "OpenRouter",
                "Invalid response format: missing choices",
            );
            LLMError::Provider {
                message: formatted,
                metadata: None,
            }
        })?;

    if choices.is_empty() {
        let formatted = error_display::format_llm_error("OpenRouter", "No choices in response");
        return Err(LLMError::Provider {
            message: formatted,
            metadata: None,
        });
    }

    let choice = &choices[0];
    let message = choice.get("message").ok_or_else(|| {
        let formatted = error_display::format_llm_error(
            "OpenRouter",
            "Invalid response format: missing message",
        );
        LLMError::Provider {
            message: formatted,
            metadata: None,
        }
    })?;

    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());

    let tool_calls = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|calls| {
            calls
                .iter()
                .filter_map(|call| {
                    let id = call.get("id").and_then(|v| v.as_str())?;
                    let function = call.get("function")?;
                    let name = function.get("name").and_then(|v| v.as_str())?;
                    let arguments = function.get("arguments").and_then(|v| v.as_str())?;
                    Some(ToolCall::function(
                        id.to_string(),
                        name.to_string(),
                        arguments.to_string(),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .filter(|calls| !calls.is_empty());

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|fr| fr.as_str())
        .map(|fr| match fr {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            other => FinishReason::Error(other.to_string()),
        })
        .unwrap_or(FinishReason::Stop);

    let usage = response_json.get("usage").map(|usage_value| Usage {
        prompt_tokens: usage_value
            .get("prompt_tokens")
            .and_then(|pt| pt.as_u64())
            .unwrap_or(0) as u32,
        completion_tokens: usage_value
            .get("completion_tokens")
            .and_then(|ct| ct.as_u64())
            .unwrap_or(0) as u32,
        total_tokens: usage_value
            .get("total_tokens")
            .and_then(|tt| tt.as_u64())
            .unwrap_or(0) as u32,
        cached_prompt_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
    });

    // Extract reasoning: prefer native reasoning_details field, fallback to tags in content
    // OpenRouter exposes reasoning via reasoning_details for models like MiniMax-M2.5
    let native_reasoning = message
        .get("reasoning_details")
        .and_then(|rd| rd.as_array())
        .map(|details| {
            details
                .iter()
                .filter_map(|d| d.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n\n")
        });

    let (reasoning, final_content) = if let Some(reasoning_str) = native_reasoning {
        // Native reasoning_details found (OpenRouter format for reasoning models)
        (Some(reasoning_str), content)
    } else if let Some(ref content_str) = content {
        // Fallback: extract from <think></think> tags in content
        let (reasoning_parts, cleaned_content) =
            crate::llm::utils::extract_reasoning_content(content_str);
        if reasoning_parts.is_empty() {
            (None, content.clone())
        } else {
            (
                Some(reasoning_parts.join("\n\n")),
                cleaned_content.or(Some(content_str.clone())),
            )
        }
    } else {
        (None, None)
    };

    Ok(LLMResponse {
        content: final_content,
        tool_calls,
        model,
        usage,
        finish_reason,
        reasoning,
        reasoning_details: None,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    })
}
