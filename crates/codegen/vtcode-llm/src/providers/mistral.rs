use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};

use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, impl_openai_compat_provider};
use crate::provider::{LLMError, LLMRequest};

pub struct MistralSpec;

impl OpenAiCompatSpec for MistralSpec {
    const NAME: &'static str = "Mistral";
    const KEY: &'static str = "mistral";
    const API_KEY_ENV: &'static str = "MISTRAL_API_KEY";
    const DEFAULT_MODEL: &'static str = models::mistral::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::MISTRAL_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::MISTRAL_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::mistral::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> =
        Some(models::mistral::SUPPORTED_MODELS);

    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = true;
    const INCLUDE_USER_ID: bool = true;
    const DELTA_ORDER: super::shared::OpenAiDeltaOrder =
        super::shared::OpenAiDeltaOrder::ContentFirst;

    fn response_cache_metrics(core: &OpenAiCompatCore<Self>) -> bool {
        core.prompt_cache_enabled
    }

    fn stream_cache_metrics(_core: &OpenAiCompatCore<Self>) -> bool {
        true
    }

    fn insert_tool_choice(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) {
        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_owned(),
                choice.to_provider_format(Self::KEY),
            );
        } else if request.tools.as_ref().is_some_and(|t| !t.is_empty()) {
            // Mistral's default "auto" tool_choice sometimes causes the model
            // to emit tool call arguments as plain text content. Setting it
            // explicitly when tools are present helps the model use
            // structured tool_calls.
            payload.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
        }
    }

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if let Some(effort) = request.reasoning_effort
            && effort != vtcode_config::types::ReasoningEffortLevel::None
        {
            payload.insert(
                "reasoning_effort".to_owned(),
                Value::String("high".to_owned()),
            );
        }
        Ok(())
    }

    fn finish_payload(
        _core: &OpenAiCompatCore<Self>,
        _request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if payload.contains_key("tools") {
            payload.insert("parallel_tool_calls".to_owned(), Value::Bool(false));
        }
        Ok(())
    }
}

impl_openai_compat_provider!(MistralProvider, MistralSpec, {
    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.core.model
        } else {
            model
        };

        self.core
            .model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(false)
            || requested == models::mistral::MISTRAL_LARGE_3
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.core
            .model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        256_000
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Message, ToolChoice, ToolDefinition};
    use std::sync::Arc;
    use vtcode_config::types::ReasoningEffortLevel;

    fn provider() -> MistralProvider {
        MistralProvider::from_config(
            Some("test-key".to_string()),
            Some("mistral-large-latest".to_string()),
            Some("https://example.test/v1".to_string()),
            None,
            None,
            None,
            None,
        )
    }

    fn base_request() -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::new("system guidance".to_string())),
            model: "mistral-large-latest".to_string(),
            max_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.25),
            stream: true,
            metadata: Some(serde_json::json!({"user_id": "user-42"})),
            ..Default::default()
        }
    }

    fn sample_tools() -> Arc<Vec<ToolDefinition>> {
        Arc::new(vec![ToolDefinition::function(
            "lookup".to_string(),
            "Look things up".to_string(),
            serde_json::json!({"type": "object", "properties": {}}),
        )])
    }

    #[test]
    fn golden_payload_basic_shape() {
        let payload = provider().core.convert_request(&base_request()).unwrap();

        assert_eq!(payload["model"], "mistral-large-latest");
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system guidance");
        assert_eq!(payload["max_tokens"], 512);
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["top_p"], 0.25);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["user_id"], "user-42");
        assert!(payload.get("tools").is_none());
        assert!(payload.get("tool_choice").is_none());
        assert!(payload.get("parallel_tool_calls").is_none());
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn golden_payload_tools_disable_parallel_calls_and_default_to_auto() {
        let mut request = base_request();
        request.tools = Some(sample_tools());
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(payload["tools"].as_array().unwrap().len(), 1);
        assert_eq!(payload["parallel_tool_calls"], false);
        // Implicit tool_choice defaults to auto when tools are present.
        assert_eq!(payload["tool_choice"], "auto");

        let mut request = base_request();
        request.tools = Some(sample_tools());
        request.tool_choice = Some(ToolChoice::Any);
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(
            payload["tool_choice"],
            ToolChoice::Any.to_provider_format("mistral")
        );
    }

    #[test]
    fn golden_payload_reasoning_effort_pinned_to_high() {
        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::Low);
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(payload["reasoning_effort"], "high");
        // Sampling parameters are not suppressed.
        assert_eq!(payload["temperature"], 0.5);

        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::None);
        let payload = provider().core.convert_request(&request).unwrap();
        assert!(payload.get("reasoning_effort").is_none());
    }
}
