use reqwest::RequestBuilder;
use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::models::{MiMoAuthMethod, detect_mimo_auth_method};

use super::common::override_base_url;
use super::extract_reasoning_trace;
use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, impl_openai_compat_provider};
use crate::provider::{LLMError, LLMRequest};

pub struct MimoSpec;

fn mimo_reasoning(message: &Value, choice: &Value) -> Option<String> {
    message
        .get("reasoning_content")
        .and_then(extract_reasoning_trace)
        .or_else(|| choice.get("reasoning_content").and_then(extract_reasoning_trace))
}

/// Auth method is fully derivable from the key prefix and base URL, so it is
/// recomputed where needed instead of being stored on the provider.
fn auth_method(core: &OpenAiCompatCore<MimoSpec>) -> MiMoAuthMethod {
    detect_mimo_auth_method(&core.api_key, Some(&core.base_url))
}

impl OpenAiCompatSpec for MimoSpec {
    const NAME: &'static str = "Xiaomi MiMo";
    const KEY: &'static str = "mimo";
    const API_KEY_ENV: &'static str = "MIMO_API_KEY";
    const DEFAULT_MODEL: &'static str = models::mimo::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::MIMO_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::MIMO_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::mimo::PAYG_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = None;

    const MAX_TOKENS_KEY: &'static str = "max_completion_tokens";
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = true;
    const INCLUDE_USER_ID: bool = true;
    const RESPONSE_REASONING_EXTRACTOR: Option<super::openai_compat::ReasoningExtractor> = Some(mimo_reasoning);

    fn resolve_base_url(api_key: &str, base_url: Option<String>) -> String {
        let auth = detect_mimo_auth_method(api_key, base_url.as_deref());
        let env_var = match auth {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => env_vars::MIMO_BASE_URL,
            MiMoAuthMethod::TokenPlan => env_vars::MIMO_TOKEN_PLAN_BASE_URL,
        };
        override_base_url(auth.api_base(), base_url, Some(env_var))
    }

    fn response_cache_metrics(core: &OpenAiCompatCore<Self>) -> bool {
        core.prompt_cache_enabled
    }

    fn stream_cache_metrics(_core: &OpenAiCompatCore<Self>) -> bool {
        true
    }

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), LLMError> {
        if let Some(effort) = request.reasoning_effort {
            let kind = if effort == vtcode_config::types::ReasoningEffortLevel::None {
                "disabled"
            } else {
                "enabled"
            };
            payload.insert("thinking".to_owned(), serde_json::json!({"type": kind}));
        }
        Ok(())
    }

    fn apply_auth(core: &OpenAiCompatCore<Self>, builder: RequestBuilder) -> RequestBuilder {
        match auth_method(core) {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => builder.header("api-key", &core.api_key),
            MiMoAuthMethod::TokenPlan => builder.bearer_auth(&core.api_key),
        }
    }

    fn api_key_env(core: &OpenAiCompatCore<Self>) -> &'static str {
        auth_method(core).env_key()
    }

    fn listed_models(core: &OpenAiCompatCore<Self>) -> &'static [&'static str] {
        match auth_method(core) {
            MiMoAuthMethod::PayAsYouGo | MiMoAuthMethod::Unknown => models::mimo::PAYG_MODELS,
            MiMoAuthMethod::TokenPlan => models::mimo::TOKEN_PLAN_MODELS,
        }
    }

    fn validate(core: &OpenAiCompatCore<Self>, request: &LLMRequest) -> Result<(), LLMError> {
        super::common::validate_supported_models(request, Self::NAME, Self::KEY, Self::listed_models(core))
    }
}

impl_openai_compat_provider!(MiMoProvider, MimoSpec, {
    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, model: &str) -> bool {
        model == models::mimo::MIMO_V2_5
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
            || requested == models::mimo::MIMO_V2_5_PRO
            || requested == models::mimo::MIMO_V2_5
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.core
            .model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let requested = if model.trim().is_empty() {
            &self.core.model
        } else {
            model
        };
        match requested {
            models::mimo::MIMO_V2_5_PRO | models::mimo::MIMO_V2_5 => 1_048_576,
            _ => 128_000,
        }
    }
});

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{LLMProvider, Message, ToolChoice};
    use std::sync::Arc;
    use vtcode_config::types::ReasoningEffortLevel;

    fn provider() -> MiMoProvider {
        MiMoProvider::from_config(
            Some("sk-test-key".to_string()),
            Some("mimo-v2.5".to_string()),
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
            model: "mimo-v2.5".to_string(),
            max_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.25),
            stream: true,
            tool_choice: Some(ToolChoice::Auto),
            metadata: Some(serde_json::json!({"user_id": "user-42"})),
            ..Default::default()
        }
    }

    #[test]
    fn golden_payload_basic_shape() {
        let payload = provider().core.convert_request(&base_request()).unwrap();

        assert_eq!(payload["model"], "mimo-v2.5");
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system guidance");
        assert_eq!(payload["max_completion_tokens"], 512);
        assert!(payload.get("max_tokens").is_none());
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["top_p"], 0.25);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["tool_choice"], "auto");
        assert_eq!(payload["user_id"], "user-42");
        assert!(payload.get("thinking").is_none());
    }

    #[test]
    fn golden_payload_thinking_object_and_sampling_suppression() {
        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::High);
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(payload["thinking"]["type"], "enabled");
        assert!(payload.get("temperature").is_none());
        assert!(payload.get("top_p").is_none());

        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::None);
        let payload = provider().core.convert_request(&request).unwrap();
        assert_eq!(payload["thinking"]["type"], "disabled");
        assert_eq!(payload["temperature"], 0.5);
    }

    #[test]
    fn auth_method_drives_supported_models() {
        let payg = MiMoProvider::from_config(Some("sk-test-key".to_string()), None, None, None, None, None, None);
        assert_eq!(
            payg.supported_models(),
            models::mimo::PAYG_MODELS.iter().map(|m| m.to_string()).collect::<Vec<_>>()
        );

        let token_plan = MiMoProvider::from_config(Some("tp-test-key".to_string()), None, None, None, None, None, None);
        assert_eq!(
            token_plan.supported_models(),
            models::mimo::TOKEN_PLAN_MODELS
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
        );
    }
}
