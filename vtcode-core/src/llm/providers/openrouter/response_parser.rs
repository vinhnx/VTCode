//! Response parsing for OpenRouter API
//!
//! Converts OpenRouter API JSON responses into internal LLMResponse format.

use crate::llm::provider::{LLMError, LLMResponse};
use crate::llm::providers::common::parse_response_openai_format;
use serde_json::Value;

pub fn parse_response(
    response_json: Value,
    model: String,
    include_cache_metrics: bool,
) -> Result<LLMResponse, LLMError> {
    parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
        response_json,
        "OpenRouter",
        model,
        include_cache_metrics,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::parse_response;
    use serde_json::json;

    #[test]
    fn parse_response_preserves_native_reasoning_details() {
        let response = json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "answer",
                    "reasoning_details": [{
                        "type": "reasoning.text",
                        "id": "r1",
                        "text": "trace"
                    }]
                }
            }],
            "usage": {
                "prompt_tokens": 1,
                "completion_tokens": 1,
                "total_tokens": 2
            }
        });

        let parsed = parse_response(response, "minimax/minimax-m2.5".to_string(), false)
            .expect("response should parse");
        assert_eq!(parsed.reasoning.as_deref(), Some("trace"));
        assert!(parsed.reasoning_details.is_some());
    }

    #[test]
    fn parse_response_includes_cache_metrics_when_enabled() {
        let response = json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "answer"
                }
            }],
            "usage": {
                "prompt_tokens": 200,
                "completion_tokens": 20,
                "total_tokens": 220,
                "prompt_cache_hit_tokens": 120,
                "prompt_cache_miss_tokens": 40
            }
        });

        let parsed = parse_response(response, "openai/gpt-5".to_string(), true)
            .expect("response should parse");
        let usage = parsed.usage.expect("usage should exist");
        assert_eq!(usage.cached_prompt_tokens, Some(120));
        assert_eq!(usage.cache_creation_tokens, Some(40));
    }
}
