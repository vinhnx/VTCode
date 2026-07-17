use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::types::ReasoningEffortLevel;

use super::extract_reasoning_trace;
use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, impl_openai_compat_provider};
use crate::provider::{LLMError, LLMRequest};

const LEGACY_API_KEY_ENV: &str = "STEP_API_KEY";

pub struct StepFunSpec;

fn stepfun_reasoning(message: &Value, choice: &Value) -> Option<String> {
    message
        .get("reasoning")
        .and_then(extract_reasoning_trace)
        .or_else(|| choice.get("reasoning").and_then(extract_reasoning_trace))
}

fn reasoning_effort_value(effort: ReasoningEffortLevel) -> Option<&'static str> {
    match effort {
        ReasoningEffortLevel::None | ReasoningEffortLevel::Unknown => None,
        ReasoningEffortLevel::Minimal | ReasoningEffortLevel::Low => Some("low"),
        ReasoningEffortLevel::Medium => Some("medium"),
        ReasoningEffortLevel::High | ReasoningEffortLevel::XHigh | ReasoningEffortLevel::Max => {
            Some("high")
        }
    }
}

impl OpenAiCompatSpec for StepFunSpec {
    const NAME: &'static str = "StepFun";
    const KEY: &'static str = "stepfun";
    const API_KEY_ENV: &'static str = "STEPFUN_API_KEY";
    const DEFAULT_MODEL: &'static str = models::stepfun::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::STEPFUN_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::STEPFUN_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::stepfun::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> =
        Some(models::stepfun::SUPPORTED_MODELS);

    const STREAM_REASONING_FIELDS: &'static [&'static str] = &["reasoning"];
    const RESPONSE_REASONING_EXTRACTOR: Option<super::openai_compat::ReasoningExtractor> =
        Some(stepfun_reasoning);

    fn resolve_api_key(api_key: Option<String>) -> String {
        api_key
            .filter(|key| !key.trim().is_empty())
            .or_else(|| std::env::var(Self::API_KEY_ENV).ok())
            .or_else(|| std::env::var(LEGACY_API_KEY_ENV).ok())
            .unwrap_or_default()
    }

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if let Some(effort) = request.reasoning_effort
            && let Some(mapped) = reasoning_effort_value(effort)
        {
            payload.insert(
                "reasoning_effort".to_owned(),
                Value::String(mapped.to_string()),
            );
        }
        Ok(())
    }
}

impl_openai_compat_provider!(StepFunProvider, StepFunSpec, {
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
            .and_then(|behavior| behavior.model_supports_reasoning)
            .unwrap_or(false)
            || models::stepfun::REASONING_MODELS.contains(&requested)
    }

    fn supports_reasoning_effort(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.core.model
        } else {
            model
        };

        self.core
            .model_behavior
            .as_ref()
            .and_then(|behavior| behavior.model_supports_reasoning_effort)
            .unwrap_or(false)
            || models::stepfun::REASONING_MODELS.contains(&requested)
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        262_144
    }
});

#[cfg(test)]
mod tests {
    use super::StepFunProvider;
    use crate::provider::{LLMRequest, Message};
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    #[test]
    fn payload_maps_reasoning_effort() {
        let provider = StepFunProvider::new("test-key".to_string());
        let payload = provider
            .core
            .convert_request(&LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                reasoning_effort: Some(ReasoningEffortLevel::XHigh),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert_eq!(
            payload
                .get("reasoning_effort")
                .and_then(|value| value.as_str()),
            Some("high")
        );
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());
    }

    #[test]
    fn golden_payload_basic_shape() {
        use crate::provider::ToolChoice;
        use std::sync::Arc;

        let provider = StepFunProvider::new("test-key".to_string());
        let payload = provider
            .core
            .convert_request(&LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                system_prompt: Some(Arc::new("system guidance".to_string())),
                max_tokens: Some(512),
                temperature: Some(0.5),
                top_p: Some(0.25),
                stream: true,
                tool_choice: Some(ToolChoice::Auto),
                metadata: Some(serde_json::json!({"user_id": "user-42"})),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert_eq!(payload["model"], models::stepfun::STEP_3_7_FLASH);
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system guidance");
        assert_eq!(payload["max_tokens"], 512);
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["top_p"], 0.25);
        assert_eq!(payload["stream"], true);
        // StepFun does not send stream_options or user_id.
        assert!(payload.get("stream_options").is_none());
        assert!(payload.get("user_id").is_none());
        assert_eq!(payload["tool_choice"], "auto");
        assert!(payload.get("reasoning_effort").is_none());
    }

    #[test]
    fn golden_payload_unknown_effort_suppresses_sampling_without_effort_field() {
        let provider = StepFunProvider::new("test-key".to_string());
        let payload = provider
            .core
            .convert_request(&LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                temperature: Some(0.5),
                reasoning_effort: Some(ReasoningEffortLevel::Unknown),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert!(payload.get("reasoning_effort").is_none());
        assert!(payload.get("temperature").is_none());

        let payload = provider
            .core
            .convert_request(&LLMRequest {
                model: models::stepfun::STEP_3_7_FLASH.to_string(),
                messages: vec![Message::user("hello".to_string())].into(),
                temperature: Some(0.5),
                reasoning_effort: Some(ReasoningEffortLevel::Low),
                ..Default::default()
            })
            .expect("payload should be valid");

        assert_eq!(payload["reasoning_effort"], "low");
    }
}
