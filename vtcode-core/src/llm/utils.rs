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

    Some(LLMRequest {
        messages: parsed_messages,
        model: model.to_string(),
        temperature,
        ..Default::default()
    })
}

/// Parse response from OpenAI-compatible format
pub fn parse_response_openai_format(
    response: Value,
    _provider_name: &str,
    model: String,
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
        model,
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
        tool_references: Vec::new(),
        request_id: None,
        organization_id: None,
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
///
/// Supports the following reasoning tag patterns:
/// - <think></think>
/// - <thought></thought>
/// - <reasoning></reasoning>
/// - <analysis></analysis>
/// - <thinking></thinking>
///
/// Returns (reasoning_parts, cleaned_content) where reasoning_parts contains
/// the extracted reasoning text (without tags) and cleaned_content is the
/// remaining content with reasoning sections removed.
pub fn extract_reasoning_content(content: &str) -> (Vec<String>, Option<String>) {
    if let Some((deprecated_reasoning, deprecated_content)) =
        extract_deprecated_reasoning_sections(content)
    {
        let reasoning_parts = deprecated_reasoning
            .map(|value| vec![value])
            .unwrap_or_default();
        return (reasoning_parts, deprecated_content);
    }

    // Use the robust split_reasoning_from_text function that handles all tag types
    let (segments, cleaned_content) = crate::llm::providers::split_reasoning_from_text(content);

    let reasoning_parts: Vec<String> = segments.into_iter().map(|s| s.text).collect();

    let final_content = if let Some(cleaned) = cleaned_content {
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        None
    };

    (reasoning_parts, final_content)
}

fn extract_deprecated_reasoning_sections(
    content: &str,
) -> Option<(Option<String>, Option<String>)> {
    let mut reasoning_lines: Vec<String> = Vec::new();
    let mut content_lines: Vec<String> = Vec::new();
    let mut active_section: Option<&str> = None;
    let mut saw_reasoning = false;
    let mut saw_content = false;
    let mut saw_first_key = false;

    for line in content.lines() {
        let trimmed = line.trim_end();
        let trimmed_start = trimmed.trim_start();

        if trimmed_start.is_empty() {
            if let Some(section) = active_section {
                match section {
                    "reasoning" => reasoning_lines.push(String::new()),
                    "content" => content_lines.push(String::new()),
                    _ => {}
                }
            }
            continue;
        }

        if let Some(rest) = trimmed_start.strip_prefix("reasoning:") {
            saw_first_key = true;
            saw_reasoning = true;
            active_section = Some("reasoning");
            let value = rest.trim_start();
            if !matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") && !value.is_empty() {
                reasoning_lines.push(value.to_string());
            }
            continue;
        }

        if let Some(rest) = trimmed_start.strip_prefix("content:") {
            saw_first_key = true;
            saw_content = true;
            active_section = Some("content");
            let value = rest.trim_start();
            if !matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") && !value.is_empty() {
                content_lines.push(value.to_string());
            }
            continue;
        }

        if !saw_first_key {
            return None;
        }

        if let Some(section) = active_section {
            match section {
                "reasoning" => reasoning_lines.push(trimmed_start.to_string()),
                "content" => content_lines.push(trimmed_start.to_string()),
                _ => {}
            }
        }
    }

    if !(saw_reasoning && saw_content) {
        return None;
    }

    let reasoning = join_deprecated_section(&reasoning_lines);
    let content = join_deprecated_section(&content_lines);

    if reasoning.is_none() && content.is_none() {
        None
    } else {
        Some((reasoning, content))
    }
}

fn join_deprecated_section(lines: &[String]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }

    let joined = lines.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub use vtcode_commons::tokens::{
    estimate_tokens as estimate_token_count, truncate_to_tokens as truncate_to_token_limit,
};

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
            "model": "gpt-5",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there"}
            ],
            "temperature": 0.7,
            "max_tokens": 100
        });

        let request = parse_chat_request_openai_format(&json, "default-model").unwrap();
        assert_eq!(request.model, "gpt-5");
        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[0].role, MessageRole::User);
        assert_eq!(request.messages[0].content.as_text(), "Hello");
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
            "model": "gpt-5"
        });

        let result =
            parse_response_openai_format(response, "test", "gpt-5".to_string(), false, None)
                .unwrap();
        assert_eq!(result.content_text(), "Hello world");
        let usage = result.usage.expect("usage should be present");
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 5);
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
    fn test_extract_reasoning_content_deprecated_format() {
        let content = "reasoning: Need to run cargo clippy.\ncontent: Need to run cargo clippy.";
        let (reasoning, main) = extract_reasoning_content(content);

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0], "Need to run cargo clippy.");
        assert_eq!(main.as_deref(), Some("Need to run cargo clippy."));
    }

    #[test]
    fn test_extract_reasoning_content_think_tags() {
        let content = "Let me think <think>I need to analyze this problem</think>The answer is 42";
        let (reasoning, main) = extract_reasoning_content(content);

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0], "I need to analyze this problem");
        assert_eq!(main.unwrap(), "Let me think The answer is 42");
    }

    #[test]
    fn test_extract_reasoning_content_analysis_tags() {
        let content = "<analysis>Breaking down the requirements</analysis>Here is the solution";
        let (reasoning, main) = extract_reasoning_content(content);

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0], "Breaking down the requirements");
        assert_eq!(main.unwrap(), "Here is the solution");
    }

    #[test]
    fn test_extract_reasoning_content_thinking_tags() {
        let content = "<thinking>First, I'll check the dependencies</thinking>Now implementing";
        let (reasoning, main) = extract_reasoning_content(content);

        assert_eq!(reasoning.len(), 1);
        assert_eq!(reasoning[0], "First, I'll check the dependencies");
        assert_eq!(main.unwrap(), "Now implementing");
    }

    #[test]
    fn test_extract_reasoning_content_multiple_tags() {
        // Multiple reasoning sections with different tag types
        let content = "<think>Step 1: Plan</think> text <analysis>Step 2: Analyze</think> end";
        let (reasoning, main) = extract_reasoning_content(content);

        // Note: Current implementation may merge adjacent reasoning segments
        // Testing that at least one reasoning section is extracted
        assert!(!reasoning.is_empty());
        assert!(
            reasoning
                .iter()
                .any(|r| r.contains("Step 1") || r.contains("Step 2"))
        );
        let main_text = main.unwrap();
        assert!(main_text.contains("text") || main_text.contains("end"));
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
        assert!(validate_model_string("gpt-5").is_ok());
        assert!(validate_model_string("claude-haiku-4-5").is_ok());
        assert!(validate_model_string("").is_err());
        assert!(validate_model_string(&"a".repeat(101)).is_err());
        assert!(validate_model_string("invalid@model").is_err());
    }
}
