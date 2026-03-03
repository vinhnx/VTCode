//! Response parsing for OpenRouter API
//!
//! Converts OpenRouter API JSON responses into internal LLMResponse format.

use crate::llm::provider::{LLMError, LLMResponse};
use crate::llm::providers::common::parse_response_openai_format;
use serde_json::Value;

pub fn parse_response(response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
    parse_response_openai_format::<fn(&Value, &Value) -> Option<String>>(
        response_json,
        "OpenRouter",
        model,
        false,
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

        let parsed = parse_response(response, "minimax/minimax-m2.5".to_string())
            .expect("response should parse");
        assert_eq!(parsed.reasoning.as_deref(), Some("trace"));
        assert!(parsed.reasoning_details.is_some());
    }
}
