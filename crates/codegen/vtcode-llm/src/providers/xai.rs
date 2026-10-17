use serde_json::{Map, Value};
use vtcode_config::constants::{env_vars, models, urls};

use super::extract_reasoning_trace;
use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, SystemPromptPlacement, impl_openai_compat_provider};

pub struct XaiSpec;

fn xai_reasoning(message: &Value, _choice: &Value) -> Option<String> {
    message.get("reasoning_content").and_then(extract_reasoning_trace)
}

impl OpenAiCompatSpec for XaiSpec {
    const NAME: &'static str = "xAI";
    const KEY: &'static str = "xai";
    const API_KEY_ENV: &'static str = "XAI_API_KEY";
    const DEFAULT_MODEL: &'static str = models::xai::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::XAI_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::XAI_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::xai::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = Some(models::xai::SUPPORTED_MODELS);

    const SYSTEM_PROMPT: SystemPromptPlacement = SystemPromptPlacement::FirstMessage;
    const STREAM_OPTIONS_INCLUDE_USAGE: bool = true;
    const INCLUDE_USER_ID: bool = true;
    const RESPONSE_REASONING_EXTRACTOR: Option<super::openai_compat::ReasoningExtractor> = Some(xai_reasoning);

    fn insert_reasoning(
        _core: &OpenAiCompatCore<Self>,
        request: &crate::provider::LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), crate::provider::LLMError> {
        if let Some(effort) = request.reasoning_effort {
            if !matches!(
                effort,
                vtcode_config::types::ReasoningEffortLevel::None | vtcode_config::types::ReasoningEffortLevel::Unknown
            ) {
                // xAI natively supports `low`/`medium`/`high`/`xhigh` only:
                // no native `minimal` (clamp to `low`) or `max` (clamp to `xhigh`).
                // Older models treat `xhigh` as `high`.
                let value = match effort {
                    vtcode_config::types::ReasoningEffortLevel::Minimal
                    | vtcode_config::types::ReasoningEffortLevel::Low => "low",
                    vtcode_config::types::ReasoningEffortLevel::Max => "xhigh",
                    other => other.as_str(),
                };
                payload.insert("reasoning_effort".to_owned(), serde_json::json!(value));
            }
        }
        Ok(())
    }

    fn response_cache_metrics(core: &OpenAiCompatCore<Self>) -> bool {
        core.prompt_cache_enabled
    }

    fn stream_cache_metrics(_core: &OpenAiCompatCore<Self>) -> bool {
        true
    }

    /// xAI cache affinity: stable conversation identity from VT Code lineage.
    /// Body `prompt_cache_key` covers Responses-style hosts; Chat Completions
    /// sticky routing uses `x-grok-conv-id` via [`session_affinity_header`].
    fn finish_payload(
        _core: &OpenAiCompatCore<Self>,
        request: &crate::provider::LLMRequest,
        payload: &mut Map<String, Value>,
    ) -> Result<(), crate::provider::LLMError> {
        if let Some(lineage) =
            crate::providers::shared::session_lineage_from_prompt_cache_key(request.prompt_cache_key.as_deref())
        {
            payload.insert("prompt_cache_key".to_owned(), Value::String(lineage.clone()));
        }
        Ok(())
    }

    fn session_affinity_header() -> Option<&'static str> {
        Some("x-grok-conv-id")
    }
}

impl XAIProvider {
    /// xAI serves an OpenAI-compatible standalone compaction endpoint
    /// (`POST /v1/responses/compact`), but only for curated Grok models on the
    /// xAI API itself. Anything else stays on the universal local
    /// summarization fallback.
    fn xai_compact_model(&self, model: &str) -> bool {
        let resolved = if model.trim().is_empty() {
            self.core.model.as_str()
        } else {
            model
        };
        models::xai::SUPPORTED_MODELS.contains(&resolved) && self.core.base_url.contains("api.x.ai")
    }

    fn compact_client(&self, model: &str) -> crate::providers::openresponses::OpenResponsesProvider {
        crate::providers::openresponses::OpenResponsesProvider::compact_endpoint_client(
            &self.core.model,
            &self.core.base_url,
            &self.core.api_key,
            model,
        )
    }
}

impl_openai_compat_provider!(XAIProvider, XaiSpec, {
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
            || models::xai::REASONING_MODELS.contains(&requested)
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
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or_else(|| {
                vtcode_config::models::model_catalog_entry("xai", requested)
                    .is_some_and(|entry| !entry.reasoning_efforts.is_empty())
            })
    }

    fn supports_responses_compaction(&self, model: &str) -> bool {
        self.xai_compact_model(model)
    }

    fn supports_manual_openai_compaction(&self, model: &str) -> bool {
        self.xai_compact_model(model)
    }

    async fn compact_history(
        &self,
        model: &str,
        history: &[crate::provider::Message],
    ) -> Result<Vec<crate::provider::Message>, crate::provider::LLMError> {
        if !self.xai_compact_model(model) {
            return Err(crate::provider::LLMError::Provider {
                message: "xAI compaction is only supported for curated Grok models on the xAI API".to_string(),
                metadata: None,
            });
        }
        self.compact_client(model).compact_history_request(model, history).await
    }

    async fn compact_history_with_options(
        &self,
        model: &str,
        history: &[crate::provider::Message],
        _options: &crate::provider::ResponsesCompactionOptions,
    ) -> Result<Vec<crate::provider::Message>, crate::provider::LLMError> {
        self.compact_history(model, history).await
    }
});

#[cfg(test)]
mod tests {
    #[test]
    fn xai_advertises_grok_session_affinity_header() {
        use super::XaiSpec;
        use crate::providers::openai_compat::OpenAiCompatSpec;
        assert_eq!(XaiSpec::session_affinity_header(), Some("x-grok-conv-id"));
    }

    #[test]
    fn xai_finish_payload_injects_lineage_prompt_cache_key() {
        let provider = XAIProvider::new("test-key".to_string());

        let mut request = base_request();
        request.prompt_cache_key = Some("vtcode:xai:session-lineage-1".to_string());
        let payload = provider.core.convert_request(&request).unwrap();
        assert_eq!(payload["prompt_cache_key"].as_str(), Some("session-lineage-1"));

        let mut blank = base_request();
        blank.prompt_cache_key = Some("   ".to_string());
        let payload = provider.core.convert_request(&blank).unwrap();
        assert!(payload.get("prompt_cache_key").is_none(), "blank lineage must omit prompt_cache_key");
    }

    #[test]
    fn xai_dispatch_header_name_is_pinned_for_lineage_routing() {
        use super::XaiSpec;
        use crate::providers::openai_compat::OpenAiCompatSpec;
        // Production dispatch attaches this header when request.prompt_cache_key
        // carries session lineage (see openai_compat::dispatch).
        assert_eq!(XaiSpec::session_affinity_header(), Some("x-grok-conv-id"));
    }

    use super::XAIProvider;
    use crate::provider::{LLMRequest, Message, ToolChoice};
    use std::sync::Arc;
    use vtcode_config::constants::models;
    use vtcode_config::types::ReasoningEffortLevel;

    fn base_request() -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::from("system guidance")),
            model: models::xai::DEFAULT_MODEL.to_string(),
            max_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.25),
            stream: true,
            tool_choice: Some(ToolChoice::Auto),
            ..Default::default()
        }
    }

    #[test]
    fn golden_payload_basic_shape() {
        let provider = XAIProvider::new("test-key".to_string());
        let payload = provider.core.convert_request(&base_request()).unwrap();

        assert_eq!(payload["model"], models::xai::DEFAULT_MODEL);
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "system guidance");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(payload["max_tokens"], 512);
        assert_eq!(payload["temperature"], 0.5);
        assert_eq!(payload["top_p"], 0.25);
        assert_eq!(payload["stream"], true);
        assert_eq!(payload["stream_options"]["include_usage"], true);
        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn golden_payload_with_reasoning_effort() {
        let provider = XAIProvider::new("test-key".to_string());

        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::High);
        let payload = provider.core.convert_request(&request).unwrap();
        assert_eq!(payload["reasoning_effort"], "high");
    }

    #[test]
    fn max_effort_clamps_to_xhigh() {
        let provider = XAIProvider::new("test-key".to_string());

        let mut request = base_request();
        request.reasoning_effort = Some(ReasoningEffortLevel::Max);
        let payload = provider.core.convert_request(&request).unwrap();
        // No native `max`; closest supported level is `xhigh`.
        assert_eq!(payload["reasoning_effort"], "xhigh");

        // No native `minimal` either; closest supported level is `low`.
        request.reasoning_effort = Some(ReasoningEffortLevel::Minimal);
        let payload = provider.core.convert_request(&request).unwrap();
        assert_eq!(payload["reasoning_effort"], "low");
    }

    #[test]
    fn compat_payload_forwards_ultrafast_service_tier() {
        let provider = XAIProvider::new("test-key".to_string());

        let mut request = base_request();
        request.service_tier = Some("ultrafast".to_string());
        let payload = provider.core.convert_request(&request).unwrap();
        assert_eq!(payload["service_tier"], "ultrafast");

        let mut blank = base_request();
        blank.service_tier = Some("   ".to_string());
        let payload = provider.core.convert_request(&blank).unwrap();
        assert!(payload.get("service_tier").is_none(), "blank tier must be omitted");
    }

    #[test]
    fn compaction_support_is_curated_grok_models_on_xai_api_only() {
        use crate::provider::LLMProvider;

        // Default xAI endpoint: curated Grok models compact natively.
        let provider = XAIProvider::new("test-key".to_string());
        assert!(provider.supports_responses_compaction(models::xai::DEFAULT_MODEL));
        assert!(provider.supports_manual_openai_compaction(models::xai::DEFAULT_MODEL));
        for model in models::xai::SUPPORTED_MODELS {
            assert!(provider.supports_manual_openai_compaction(model), "compact support for {model}");
        }
        assert!(!provider.supports_native_inline_compaction(models::xai::DEFAULT_MODEL));
    }

    #[tokio::test]
    async fn compact_history_posts_to_xai_compact_endpoint() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/responses/compact"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "cmp_01HZ9P0V8M2YQK3F7C4G6N5R2A",
                "object": "response.compaction",
                "created_at": 1748895600,
                "model": "grok-4.6",
                "output": [
                    {
                        "id": "msg_000",
                        "type": "message",
                        "status": "completed",
                        "role": "user",
                        "content": [{ "type": "input_text", "text": "Summarize the auth work." }]
                    },
                    {
                        "id": "cmp_001",
                        "type": "compaction",
                        "encrypted_content": "opaque-blob"
                    }
                ]
            })))
            .mount(&server)
            .await;

        // The host gate only passes on the xAI API, so the transport is
        // exercised through the compact client directly against the mock.
        let provider = XAIProvider::new_with_client(
            "test-key".to_string(),
            models::xai::DEFAULT_MODEL.to_string(),
            reqwest::Client::builder().no_proxy().build().expect("test client should build"),
            format!("{}/v1", server.uri()),
            vtcode_config::TimeoutsConfig::default(),
        );
        let history = vec![Message::user("Summarize the auth work.".to_string())];
        let compacted = provider
            .compact_client(models::xai::DEFAULT_MODEL)
            .compact_history_request(models::xai::DEFAULT_MODEL, &history)
            .await
            .expect("xAI compaction should succeed");
        assert!(!compacted.is_empty());
        assert!(
            compacted
                .iter()
                .any(|message| message.content.as_text().contains("Summarize the auth work.")),
            "retained xAI input must survive compaction"
        );
    }

    #[tokio::test]
    async fn compact_history_rejects_unlisted_models() {
        use crate::provider::LLMProvider;

        let provider = XAIProvider::new("test-key".to_string());
        assert!(!provider.supports_manual_openai_compaction("gpt-5"));
        let history = vec![Message::user("hello".to_string())];
        provider
            .compact_history("gpt-5", &history)
            .await
            .expect_err("unlisted models must stay on local compaction");
    }
}
