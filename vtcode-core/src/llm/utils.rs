//! Shared utilities for LLM request/response processing
//!
//! This module provides common functions used across multiple providers
//! to eliminate duplicate code and reduce allocations.

use anyhow::{Context, Result};
use serde_json::Value;

use crate::llm::provider::{
    LLMRequest, LLMResponse, LLMStreamEvent, Message, MessageContent, MessageRole, ToolCall,
};

/// Parse chat request from OpenAI-compatible format
pub fn parse_chat_request_openai_format(value: &Value, default_model: &str) -> Option<LLMRequest> {
    let messages = value.get("messages")?.as_array()?;
    let model = value
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(default_model);

    let mut parsed_messages = Vec::with_capacity(messages.len());

    for msg in messages {
        let role_str = msg.get("role")?.as_str()?;
        let content = msg.get("content")?.as_str()?;

        let role = match role_str {
            "system" => MessageRole::System,
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "tool" => MessageRole::Tool,
            _ => return None, // Invalid role
        };

        parsed_messages.push(Message {
            role,
            content: MessageContent::Text(content.to_string()),
            reasoning: None,
            reasoning_details: None,
            tool_calls: None,
            tool_call_id: None,
            origin_tool: None,
        });
    }

    let temperature = value
        .get("temperature")
        .and_then(|t| t.as_f64().map(|f| f as f32));
    let max_tokens = value
        .get("max_tokens")
        .and_then(|t| t.as_u64().map(|u| u as u32));

    Some(LLMRequest {
        messages: parsed_messages,
        system_prompt: None,
        tools: None,
        model: model.to_string(),
        max_tokens,
        temperature,
        stream: false,
        output_format: None,
        tool_choice: None,
        parallel_tool_config: None,
        reasoning_effort: None,
        parallel_tool_calls: None,
        verbosity: None,
    })
}

/// Parse response from OpenAI-compatible format
pub fn parse_response_openai_format(
    response: Value,
    _provider_name: &str,
    include_cache: bool,
    reasoning_content: Option<String>,
) -> Result<LLMResponse> {
    let choices = response
        .get("choices")
        .context("Missing choices in response")?
        .as_array()
        .context("Choices must be an array")?;

    if choices.is_empty() {
        anyhow::bail!("No choices in response");
    }

    let first_choice = &choices[0];
    let message = first_choice
        .get("message")
        .context("Missing message in choice")?;

    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    // Extract usage information
    let usage = response.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    let output_tokens = usage
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);

    // Extract function call if present
    let tool_call = message.get("function_call").and_then(|fc| {
        let name = fc.get("name")?.as_str()?;
        let arguments = fc.get("arguments")?.as_str()?;

        Some(ToolCall::function(
            "call_001".to_string(), // Generate a simple ID
            name.to_string(),
            arguments.to_string(),
        ))
    });

    // Use the provider's LLMResponse which has different structure
    let mut llm_response = LLMResponse {
        content: None,
        tool_calls: None,
        usage: Some(crate::llm::provider::Usage {
            prompt_tokens: input_tokens as u32,
            completion_tokens: output_tokens as u32,
            total_tokens: (input_tokens + output_tokens) as u32,
            cached_prompt_tokens: if include_cache {
                response
                    .get("cache_hit")
                    .and_then(|c| c.as_bool())
                    .map(|_| 0)
            } else {
                None
            },
            cache_creation_tokens: None,
            cache_read_tokens: None,
        }),
        finish_reason: crate::llm::provider::FinishReason::Stop,
        reasoning: reasoning_content,
        reasoning_details: None,
    };

    // Set content based on function call or regular content
    if let Some(tool_call) = tool_call {
        llm_response.content = None;
        llm_response.tool_calls = Some(vec![tool_call]);
    } else {
        llm_response.content = Some(content);
        llm_response.tool_calls = None;
    }

    Ok(llm_response)
}

/// Parse stream event from OpenAI-compatible format
pub fn parse_stream_event_openai_format(
    json: Value,
    _provider_name: &str,
) -> Option<LLMStreamEvent> {
    let choices = json.get("choices")?.as_array()?;
    if choices.is_empty() {
        return None;
    }

    let delta = choices[0].get("delta")?;
    let content = delta.get("content").and_then(|c| c.as_str())?;

    Some(LLMStreamEvent::Token {
        delta: content.to_string(),
    })
}

/// Extract reasoning content from text (for providers that support reasoning)
pub fn extract_reasoning_content(content: &str) -> (Vec<String>, Option<String>) {
    // Look for reasoning tags or patterns - use static arrays
    const REASONING_PATTERNS: [&str; 3] = ["<reasoning>", "<think>", "<thought>"];
    const CLOSING_PATTERNS: [&str; 3] = ["</reasoning>", "</think>", "</thought>"];

    let mut reasoning_parts = Vec::new();
    let mut main_content = content.to_string();

    for (open_tag, close_tag) in REASONING_PATTERNS.iter().zip(CLOSING_PATTERNS.iter()) {
        if let (Some(start), Some(end)) = (content.find(open_tag), content.find(close_tag)) {
            let reasoning_start = start + open_tag.len();
            let reasoning_text = content[reasoning_start..end].trim().to_string();

            if !reasoning_text.is_empty() {
                reasoning_parts.push(reasoning_text.clone());
                // Remove reasoning section from main content
                main_content.replace_range(start..(end + close_tag.len()), "");
            }
        }
    }

    let final_content = if main_content.trim().is_empty() {
        None
    } else {
        Some(main_content.trim().to_string())
    };

    (reasoning_parts, final_content)
}

/// Estimate token count for text (rough approximation)
///
/// Note: This delegates to the centralized `CharacterRatioTokenEstimator` in `core::token_estimator`
/// for consistency across the codebase. Uses byte length / 4 with minimum of 1.
#[inline]
pub fn estimate_token_count(text: &str) -> usize {
    use crate::core::token_estimator::{CharacterRatioTokenEstimator, TokenEstimator};

    // Use the shared estimator for consistency
    static ESTIMATOR: CharacterRatioTokenEstimator = CharacterRatioTokenEstimator::new(4);
    ESTIMATOR.estimate_tokens(text)
}

/// Truncate text to approximate token limit
///
/// Returns a truncated string that fits within the approximate token limit.
/// Tries to truncate at word boundaries when possible to avoid mid-word cuts.
#[inline]
pub fn truncate_to_token_limit(text: &str, max_tokens: usize) -> String {
    if max_tokens == 0 {
        return String::new();
    }

    let max_chars = max_tokens * 4;
    if text.len() <= max_chars {
        return text.to_string();
    }

    // Try to truncate at a word boundary
    let truncated = &text[..max_chars];
    match truncated.rfind(' ') {
        Some(last_space) => truncated[..last_space].to_string(),
        None => truncated.to_string(),
    }
}

/// Create a consistent error message for LLM errors
pub fn format_llm_error(provider_name: &str, error_message: &str) -> String {
    format!("[{}] {}", provider_name, error_message.trim())
}

/// Validate that a model string is not empty and reasonable
pub fn validate_model_string(model: &str) -> Result<()> {
    if model.is_empty() {
        anyhow::bail!("Model cannot be empty")
    }

    if model.len() > 100 {
        anyhow::bail!("Model name too long (max 100 characters)")
    }

    // Basic sanity check for model name format
    if !model
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ':')
    {
        anyhow::bail!("Model contains invalid characters. Only alphanumeric, -, _, ., : allowed")
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chat_request_openai_format() {
        let json = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there"}
            ],
            "temperature": 0.7,
            "max_tokens": 100
        });

        let request = parse_chat_request_openai_format(&json, "default-model").unwrap();
        assert_eq!(request.model, "gpt-4");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, "user");
        assert_eq!(request.messages[0].content, "Hello");
        assert_eq!(request.temperature, Some(0.7));
        assert_eq!(request.max_tokens, Some(100));
    }

    #[test]
    fn test_parse_response_openai_format() {
        let response = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "Hello world",
                    "role": "assistant"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            },
            "model": "gpt-4"
        });

        let result = parse_response_openai_format(response, "test", false, None).unwrap();
        assert_eq!(result.content, "Hello world");
        assert_eq!(result.input_tokens, 10);
        assert_eq!(result.output_tokens, 5);
        assert_eq!(result.model, "gpt-4");
    }

    #[test]
    fn test_extract_reasoning_content() {
        let content = "Some text <reasoning>This is reasoning</reasoning> More text";
        let (reasoning, main) = extract_reasoning_content(content);

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0], "This is reasoning");
        assert_eq!(main.unwrap(), "Some text  More text");
    }

    #[test]
    fn test_estimate_token_count() {
        assert_eq!(estimate_token_count("Hello world"), 3); // ~11 chars / 4
        assert_eq!(estimate_token_count(""), 1); // minimum 1
        assert_eq!(estimate_token_count("a"), 1); // minimum 1
    }

    #[test]
    fn test_truncate_to_token_limit() {
        let text = "Hello world this is a longer text that should be truncated";
        let truncated = truncate_to_token_limit(text, 3);
        assert!(truncated.len() < text.len());
        assert!(!truncated.contains("truncated"));
    }

    #[test]
    fn test_format_llm_error() {
        let error = format_llm_error("OpenAI", "Rate limit exceeded");
        assert_eq!(error, "[OpenAI] Rate limit exceeded");
    }

    #[test]
    fn test_validate_model_string() {
        assert!(validate_model_string("gpt-4").is_ok());
        assert!(validate_model_string("claude-3-sonnet").is_ok());
        assert!(validate_model_string("").is_err());
        assert!(validate_model_string(&"a".repeat(101)).is_err());
        assert!(validate_model_string("invalid@model").is_err());
    }
}
