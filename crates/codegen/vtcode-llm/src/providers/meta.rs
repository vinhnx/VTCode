//! Official Meta AI OpenAI-compatible provider.

use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::types::ReasoningEffortLevel;

use super::common::validate_supported_models;
use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, impl_openai_compat_provider};
use crate::provider::{LLMError, LLMRequest, ToolChoice};

/// Wire-dialect description for the official Meta AI API.
pub struct MetaSpec;

fn reasoning_effort_value(effort: ReasoningEffortLevel) -> Option<&'static str> {
    match effort {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
        ReasoningEffortLevel::Minimal => Some("minimal"),
        ReasoningEffortLevel::Low => Some("low"),
        ReasoningEffortLevel::Medium => Some("medium"),
        ReasoningEffortLevel::High => Some("high"),
        ReasoningEffortLevel::XHigh => Some("xhigh"),
        ReasoningEffortLevel::Max => Some("max"),
    }
}

impl OpenAiCompatSpec for MetaSpec {
    const NAME: &'static str = "Meta AI";
    const KEY: &'static str = "meta";
    const API_KEY_ENV: &'static str = "META_API_KEY";
    const DEFAULT_MODEL: &'static str = models::meta::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::META_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::META_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::meta::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = Some(models::meta::SUPPORTED_MODELS);
    const MAX_TOKENS_KEY: &'static str = "max_completion_tokens";
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = false;
    const STREAM_REASONING_FIELDS: &'static [&'static str] = &[];
    const VALIDATE_ON_GENERATE: bool = true;

    fn resolve_api_key(api_key: Option<String>) -> String {
        api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var(Self::API_KEY_ENV).ok().filter(|key| !key.trim().is_empty()))
            // Meta's documentation calls the official variable MODEL_API_KEY.
            .or_else(|| std::env::var("MODEL_API_KEY").ok().filter(|key| !key.trim().is_empty()))
            .unwrap_or_default()
    }

    fn insert_tool_choice(_core: &OpenAiCompatCore<Self>, request: &LLMRequest, payload: &mut Map<String, Value>) {
        if matches!(request.tool_choice, Some(ToolChoice::None)) {
            payload.remove("tools");
            return;
        }
        // Meta Chat Completions defaults to `auto`; only emit the documented
        // supported choice when callers explicitly select it.
        if request.tools.as_ref().is_some_and(|tools| !tools.is_empty())
            && matches!(request.tool_choice, Some(ToolChoice::Auto))
        {
            payload.insert("tool_choice".to_owned(), Value::String("auto".to_owned()));
        }
    }

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if let Some(effort) = request.reasoning_effort
            && let Some(value) = reasoning_effort_value(effort)
        {
            payload.insert("reasoning_effort".to_owned(), Value::String(value.to_owned()));
        }
        Ok(())
    }

    fn finish_payload(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if let Some(output_format) = &request.output_format {
            payload.insert("response_format".to_owned(), output_format.clone());
        }
        if let Some(parallel_tool_calls) = request.parallel_tool_calls
            && payload.contains_key("tools")
        {
            payload.insert("parallel_tool_calls".to_owned(), Value::Bool(parallel_tool_calls));
        }
        Ok(())
    }

    fn validate(_core: &OpenAiCompatCore<Self>, request: &LLMRequest) -> Result<(), LLMError> {
        validate_supported_models(request, Self::NAME, Self::KEY, Self::LISTED_MODELS)?;

        if request.tools.as_ref().is_some_and(|tools| !tools.is_empty())
            && request
                .tool_choice
                .as_ref()
                .is_some_and(|choice| !matches!(choice, ToolChoice::Auto | ToolChoice::None))
        {
            return Err(LLMError::InvalidRequest {
                message: "Meta AI Chat Completions supports only `tool_choice: auto` when tools are present".to_owned(),
                metadata: None,
            });
        }

        Ok(())
    }
}

impl_openai_compat_provider!(MetaProvider, MetaSpec, {
    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        true
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        true
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        1_048_576
    }
});

#[cfg(test)]
mod tests {
    use super::{MetaProvider, MetaSpec};
    use crate::BackendKind;
    use crate::provider::{LLMProvider, LLMRequest, Message, ToolChoice, ToolDefinition};
    use crate::providers::openai_compat::OpenAiCompatSpec;
    use std::sync::Arc;
    use vtcode_config::constants::{models, urls};
    use vtcode_config::types::ReasoningEffortLevel;

    fn provider() -> MetaProvider {
        MetaProvider::from_config(
            Some("test-key".to_owned()),
            Some(models::meta::DEFAULT_MODEL.to_owned()),
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn request() -> LLMRequest {
        LLMRequest {
            messages: Arc::new(vec![Message::user("hello".to_owned())]),
            model: models::meta::DEFAULT_MODEL.to_owned(),
            max_tokens: Some(512),
            temperature: Some(0.4),
            top_p: Some(0.8),
            stream: true,
            ..Default::default()
        }
    }

    #[test]
    fn payload_omits_callable_tools_for_tool_choice_none() {
        let provider = provider();
        for stream in [false, true] {
            let mut request = LLMRequest {
                model: models::meta::DEFAULT_MODEL.to_owned(),
                messages: vec![Message::user("Synthesize the saved evidence".to_owned())].into(),
                tools: Some(Arc::new(vec![ToolDefinition::function(
                    "lookup".to_owned(),
                    "Look up a value".to_owned(),
                    serde_json::json!({"type": "object"}),
                )])),
                tool_choice: Some(ToolChoice::None),
                stream,
                ..Default::default()
            };
            provider
                .validate_request(&request)
                .expect("none is supported by omitting tools");
            let payload = provider.core.convert_request(&request).expect("payload");
            assert!(payload.get("tools").is_none());
            assert!(payload.get("tool_choice").is_none());
            assert!(payload.get("tool_stream").is_none());
            request.tool_choice = Some(ToolChoice::Auto);
            let enabled = provider.core.convert_request(&request).expect("enabled payload");
            assert_eq!(enabled["tools"][0]["function"]["name"], "lookup");
            assert_eq!(enabled["tool_choice"], "auto");
        }
    }

    #[test]
    fn meta_uses_official_endpoint_and_backend_kind() {
        let provider = provider();
        assert_eq!(provider.core.base_url, urls::META_API_BASE);
        assert_eq!(provider.core.api_key, "test-key");
        assert_eq!(provider.backend_kind(), BackendKind::Meta);
        assert_eq!(MetaSpec::API_KEY_ENV, "META_API_KEY");
    }

    #[test]
    fn supported_models_include_all_official_meta_ids() {
        let expected = models::meta::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(MetaProvider::new("test-key".to_owned()).supported_models(), expected);
    }

    #[test]
    fn payload_uses_meta_completion_fields() {
        let payload = provider().core.convert_request(&request()).expect("payload should be valid");

        assert_eq!(payload["model"], models::meta::DEFAULT_MODEL);
        assert_eq!(payload["max_completion_tokens"], 512);
        assert!((payload["temperature"].as_f64().expect("temperature should be numeric") - 0.4).abs() < 1e-6);
        assert!((payload["top_p"].as_f64().expect("top_p should be numeric") - 0.8).abs() < 1e-6);
        assert_eq!(payload["stream"], true);
        assert!(payload.get("stream_options").is_none());
    }

    #[test]
    fn reasoning_effort_maps_to_meta_values() {
        for (effort, expected) in [
            (ReasoningEffortLevel::Minimal, "minimal"),
            (ReasoningEffortLevel::Low, "low"),
            (ReasoningEffortLevel::Medium, "medium"),
            (ReasoningEffortLevel::High, "high"),
            (ReasoningEffortLevel::XHigh, "xhigh"),
            (ReasoningEffortLevel::Max, "max"),
        ] {
            let mut request = request();
            request.reasoning_effort = Some(effort);
            let payload = provider().core.convert_request(&request).expect("payload should be valid");
            assert_eq!(payload["reasoning_effort"], expected);
        }

        let mut request = request();
        request.reasoning_effort = Some(ReasoningEffortLevel::None);
        let payload = provider().core.convert_request(&request).expect("payload should be valid");
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn structured_output_and_parallel_tools_are_forwarded() {
        let mut request = request();
        request.output_format = Some(serde_json::json!({
            "type": "json_schema",
            "json_schema": {"name": "answer", "schema": {"type": "object"}}
        }));
        request.parallel_tool_calls = Some(true);
        request.tools = Some(Arc::new(vec![ToolDefinition::function(
            "lookup".to_owned(),
            "Look up a value".to_owned(),
            serde_json::json!({"type": "object"}),
        )]));
        request.tool_choice = Some(ToolChoice::Auto);

        let payload = provider().core.convert_request(&request).expect("payload should be valid");
        assert_eq!(payload["response_format"]["type"], "json_schema");
        assert_eq!(payload["parallel_tool_calls"], true);
        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn unsupported_tool_choice_is_rejected_when_tools_are_present() {
        let mut request = request();
        request.tools = Some(Arc::new(vec![ToolDefinition::function(
            "lookup".to_owned(),
            "Look up a value".to_owned(),
            serde_json::json!({"type": "object"}),
        )]));
        request.tool_choice = Some(ToolChoice::Any);

        let error = provider().validate_request(&request).expect_err("choice should be rejected");
        assert!(error.to_string().contains("tool_choice"));
    }
}
