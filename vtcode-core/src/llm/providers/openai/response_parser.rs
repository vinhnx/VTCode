//! Chat Completions response parsing for OpenAI-compatible APIs.

use crate::llm::error_display;
use crate::llm::provider;
use crate::llm::providers::extract_reasoning_trace;
use crate::llm::providers::shared::parse_openai_tool_calls;
use serde_json::Value;

pub(crate) fn parse_chat_response(
    response_json: Value,
    include_cached_prompt_tokens: bool,
) -> Result<provider::LLMResponse, provider::LLMError> {
    let choices = response_json
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or_else(|| {
            let formatted_error = error_display::format_llm_error(
                "OpenAI",
                "Invalid response format: missing choices",
            );
            provider::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            }
        })?;

    if choices.is_empty() {
        let formatted_error = error_display::format_llm_error("OpenAI", "No choices in response");
        return Err(provider::LLMError::Provider {
            message: formatted_error,
            metadata: None,
        });
    }

    let choice = &choices[0];
    let message = choice.get("message").ok_or_else(|| {
        let formatted_error =
            error_display::format_llm_error("OpenAI", "Invalid response format: missing message");
        provider::LLMError::Provider {
            message: formatted_error,
            metadata: None,
        }
    })?;

    let content = match message.get("content") {
        Some(Value::String(text)) => Some(text.to_string()),
        Some(Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    };

    let tool_calls = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|calls| parse_openai_tool_calls(calls))
        .filter(|calls| !calls.is_empty());

    let reasoning = message
        .get("reasoning_content")
        .and_then(extract_reasoning_trace)
        .or_else(|| message.get("reasoning").and_then(extract_reasoning_trace))
        .or_else(|| {
            choice
                .get("reasoning_content")
                .and_then(extract_reasoning_trace)
        })
        .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
        .or_else(|| {
            content.as_ref().and_then(|c| {
                let (reasoning_parts, _) = crate::llm::utils::extract_reasoning_content(c);
                if reasoning_parts.is_empty() {
                    None
                } else {
                    Some(reasoning_parts.join("\n\n"))
                }
            })
        });

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|fr| fr.as_str())
        .map(|fr| match fr {
            "stop" => crate::llm::provider::FinishReason::Stop,
            "length" => crate::llm::provider::FinishReason::Length,
            "tool_calls" => crate::llm::provider::FinishReason::ToolCalls,
            "content_filter" => crate::llm::provider::FinishReason::ContentFilter,
            other => crate::llm::provider::FinishReason::Error(other.to_string()),
        })
        .unwrap_or(crate::llm::provider::FinishReason::Stop);

    Ok(provider::LLMResponse {
        content,
        tool_calls,
        usage: response_json.get("usage").map(|usage_value| {
            let cached_prompt_tokens = if include_cached_prompt_tokens {
                usage_value
                    .get("prompt_tokens_details")
                    .and_then(|details| details.get("cached_tokens"))
                    .and_then(|value| value.as_u64())
                    .map(|value| value as u32)
            } else {
                None
            };

            provider::Usage {
                prompt_tokens: usage_value
                    .get("prompt_tokens")
                    .and_then(|pt| pt.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0),
                completion_tokens: usage_value
                    .get("completion_tokens")
                    .and_then(|ct| ct.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0),
                total_tokens: usage_value
                    .get("total_tokens")
                    .and_then(|tt| tt.as_u64())
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0),
                cached_prompt_tokens,
                cache_creation_tokens: None,
                cache_read_tokens: None,
            }
        }),
        finish_reason,
        reasoning,
        reasoning_details: None,
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
    })
}
