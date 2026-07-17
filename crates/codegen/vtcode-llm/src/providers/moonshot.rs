use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};

use super::openai_compat::{
    OpenAiCompatCore, OpenAiCompatSpec, SystemPromptPlacement, impl_openai_compat_provider,
};
use crate::provider::{LLMError, LLMRequest};

pub struct MoonshotSpec;

fn is_thinking_model(model: &str) -> bool {
    model.contains("kimi-k3") || model.contains("k2-thinking") || model.contains("kimi-k2-thinking")
}

impl OpenAiCompatSpec for MoonshotSpec {
    const NAME: &'static str = "Moonshot";
    const KEY: &'static str = "moonshot";
    const API_KEY_ENV: &'static str = "MOONSHOT_API_KEY";
    const DEFAULT_MODEL: &'static str = models::moonshot::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::MOONSHOT_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::MOONSHOT_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::moonshot::SUPPORTED_MODELS;
    // Moonshot publishes new official aliases and preview slugs faster than VT Code's
    // curated picker list is refreshed, so let the upstream API be the source of truth
    // for model identifiers and keep local validation focused on request shape.
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = None;

    const SYSTEM_PROMPT: SystemPromptPlacement = SystemPromptPlacement::Omitted;
    const INCLUDE_TOP_P: bool = false;
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;

    fn normalize_model(model: String) -> String {
        model.trim().to_string()
    }

    fn float_number(value: f32) -> Result<serde_json::Number, LLMError> {
        serde_json::Number::from_f64(f64::from(value)).ok_or_else(|| LLMError::InvalidRequest {
            message: "Invalid temperature value".to_string(),
            metadata: None,
        })
    }

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        // Add reasoning_effort for Kimi K2 Thinking model
        if let Some(effort) = request.reasoning_effort
            && is_thinking_model(&request.model)
        {
            payload
                .insert("reasoning_effort".to_string(), Value::String(effort.as_str().to_string()));
        }
        Ok(())
    }
}

impl_openai_compat_provider!(MoonshotProvider, MoonshotSpec, {
    fn supports_reasoning(&self, model: &str) -> bool {
        is_thinking_model(model)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        is_thinking_model(model)
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, ToolChoice};
    use std::sync::Arc;
    use vtcode_config::types::ReasoningEffortLevel;

    fn provider() -> MoonshotProvider {
        MoonshotProvider::from_config(
            Some("test-key".to_string()),
            Some("kimi-k2.7".to_string()),
            Some("https://example.test/v1".to_string()),
            None,
            None,
            None,
            None,
        )
    }

    fn base_request(model: &str) -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::new("system guidance".to_string())),
            model: model.to_string(),
            max_tokens: Some(512),
            temperature: Some(0.5),
            stream: true,
            tool_choice: Some(ToolChoice::Auto),
            ..Default::default()
        }
    }

    #[test]
    fn golden_payload_basic_shape() {
        let payload = provider().core.convert_request(&base_request("kimi-k2.7")).unwrap();

        assert_eq!(payload["model"], "kimi-k2.7");
        // Moonshot does not inject the system prompt into messages.
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert_eq!(payload["max_tokens"], 512);
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["stream"], true);
        assert!(payload.get("stream_options").is_none());
        assert!(payload.get("top_p").is_none());
        assert!(payload.get("reasoning_effort").is_none());
        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn golden_payload_reasoning_effort_for_thinking_models() {
        let mut request = base_request("kimi-k2-thinking");
        request.reasoning_effort = Some(ReasoningEffortLevel::Low);
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(payload["reasoning_effort"], "low");
        // Sampling parameters are not suppressed for reasoning requests.
        assert_eq!(payload["temperature"], 0.5);

        let mut request = base_request("kimi-k2.7");
        request.reasoning_effort = Some(ReasoningEffortLevel::Low);
        let payload = provider().core.convert_request(&request).unwrap();
        assert!(payload.get("reasoning_effort").is_none());
    }
}
