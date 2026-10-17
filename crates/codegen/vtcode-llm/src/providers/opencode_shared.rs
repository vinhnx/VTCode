use vtcode_config::constants::{env_vars, models, urls};
use vtcode_config::models::model_catalog_entry;

use crate::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent};

use super::common::{map_finish_reason_common, validate_request_common};
use super::openai_compat::{OpenAiCompatCore, OpenAiCompatSpec, SystemPromptPlacement};

pub(super) fn validate_normalized_request(
    request: &LLMRequest,
    normalized_model: &str,
    provider_name: &str,
    provider_key: &str,
    supported_models: &[&str],
) -> Result<(), LLMError> {
    let supported_models = supported_models.iter().map(|model| model.to_string()).collect::<Vec<_>>();

    // Direct validation accepts an empty model; generate/stream supply the
    // default separately. Clone only when a nonempty model needs normalization.
    if request.model.trim().is_empty() || normalized_model == request.model {
        validate_request_common(request, provider_name, provider_key, Some(&supported_models))
    } else {
        let mut normalized = request.clone();
        normalized.model = normalized_model.to_string();
        validate_request_common(&normalized, provider_name, provider_key, Some(&supported_models))
    }
}

/// Wire-dialect spec for the OpenCode Go OpenAI-compatible protocol.
pub(crate) struct OpenCodeGoInnerSpec;

impl OpenAiCompatSpec for OpenCodeGoInnerSpec {
    const NAME: &'static str = "OpenCode Go";
    const KEY: &'static str = "opencode-go";
    const API_KEY_ENV: &'static str = "OPENCODE_GO_API_KEY";
    const DEFAULT_MODEL: &'static str = models::opencode_go::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::OPENCODE_GO_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::OPENCODE_GO_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::opencode_go::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = Some(models::opencode_go::SUPPORTED_MODELS);

    const SYSTEM_PROMPT: SystemPromptPlacement = SystemPromptPlacement::Omitted;
    const INCLUDE_TOP_P: bool = false;
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
}

/// Wire-dialect spec for the OpenCode Zen OpenAI-compatible protocol.
pub(crate) struct OpenCodeZenInnerSpec;

impl OpenAiCompatSpec for OpenCodeZenInnerSpec {
    const NAME: &'static str = "OpenCode Zen";
    const KEY: &'static str = "opencode-zen";
    const API_KEY_ENV: &'static str = "OPENCODE_ZEN_API_KEY";
    const DEFAULT_MODEL: &'static str = models::opencode_zen::DEFAULT_MODEL;
    const DEFAULT_BASE_URL: &'static str = urls::OPENCODE_ZEN_API_BASE;
    const BASE_URL_ENV: Option<&'static str> = Some(env_vars::OPENCODE_ZEN_BASE_URL);
    const LISTED_MODELS: &'static [&'static str] = models::opencode_zen::SUPPORTED_MODELS;
    const VALIDATION_ALLOWLIST: Option<&'static [&'static str]> = Some(models::opencode_zen::SUPPORTED_MODELS);

    const SYSTEM_PROMPT: SystemPromptPlacement = SystemPromptPlacement::Omitted;
    const INCLUDE_TOP_P: bool = false;
    const SUPPRESS_SAMPLING_WHEN_REASONING: bool = false;
}

/// Shared OpenAI-compatible provider shell used by the OpenCode Go and
/// OpenCode Zen protocol dispatchers for the models that speak plain
/// chat-completions.
pub(crate) struct OpenCodeCompatibleProvider<S: OpenAiCompatSpec> {
    core: OpenAiCompatCore<S>,
}

impl<S: OpenAiCompatSpec> OpenCodeCompatibleProvider<S> {
    pub(crate) fn new(api_key: String, http_client: reqwest::Client, base_url: String, model: String) -> Self {
        Self {
            core: OpenAiCompatCore::from_parts(api_key, model, http_client, base_url),
        }
    }

    fn requested_model<'a>(&'a self, model: &'a str) -> &'a str {
        if model.trim().is_empty() {
            &self.core.model
        } else {
            model
        }
    }
}

#[async_trait::async_trait]
impl<S: OpenAiCompatSpec> LLMProvider for OpenCodeCompatibleProvider<S> {
    fn name(&self) -> &str {
        S::KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.reasoning)
            .unwrap_or(false)
    }

    fn supports_tools(&self, model: &str) -> bool {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.tool_call)
            .unwrap_or(true)
    }

    fn supports_structured_output(&self, model: &str) -> bool {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.structured_output)
            .unwrap_or(false)
    }

    fn supports_context_caching(&self, model: &str) -> bool {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.caching)
            .unwrap_or(false)
    }

    fn supports_vision(&self, model: &str) -> bool {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.vision)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        model_catalog_entry(S::KEY, self.requested_model(model))
            .map(|entry| entry.context_window)
            .filter(|value| *value > 0)
            .unwrap_or(128_000)
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.core.prepare(&mut request);
        self.core.generate_prepared(request).await
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.core.prepare(&mut request);
        self.validate_request(&request)?;
        request.stream = true;

        let model = request.model.clone();
        let response = self.core.dispatch(&request).await?;

        let bytes_stream = response.bytes_stream();
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx = event_tx.clone();

        let model_clone = model.clone();
        let provider_name = S::NAME;
        // Timeout for the entire streaming task (5 minutes).
        // Prevents indefinite hangs when upstream server stops responding.
        let stream_timeout = std::time::Duration::from_secs(300);
        let handle = tokio::spawn(async move {
            let mut aggregator = crate::providers::shared::StreamAggregator::new(model_clone.clone());

            let result = tokio::time::timeout(
                stream_timeout,
                crate::providers::shared::process_openai_stream(bytes_stream, provider_name, model_clone, |value| {
                    if let Some(choices) = value.get("choices").and_then(|candidate| candidate.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta")
                            && let Some(content) = delta.get("content").and_then(|candidate| candidate.as_str())
                        {
                            for event in aggregator.handle_content(content) {
                                let _ = tx.send(Ok(event));
                            }
                        }

                        if let Some(reason) = choice.get("finish_reason").and_then(|candidate| candidate.as_str()) {
                            aggregator.set_finish_reason(map_finish_reason_common(reason));
                        }
                    }

                    if value.get("usage").is_some()
                        && let Some(usage) = crate::providers::common::parse_usage_openai_format(&value, false)
                    {
                        aggregator.set_usage(usage);
                    }
                    Ok(())
                }),
            )
            .await;

            match result {
                Ok(Ok(_)) => {
                    let response = aggregator.finalize();
                    let _ = tx.send(Ok(LLMStreamEvent::Completed { response: Box::new(response) }));
                }
                Ok(Err(error)) => {
                    let _ = tx.send(Err(error));
                }
                Err(_elapsed) => {
                    let _ = tx.send(Err(LLMError::Provider {
                        message: format!("{provider_name}: streaming timed out after 5 minutes"),
                        metadata: None,
                    }));
                }
            }
        });

        let stream = async_stream::try_stream! {
            let mut receiver = event_rx;
            let _abort_guard = super::common::TaskAbortGuard(handle);
            while let Some(event) = receiver.recv().await {
                yield event?;
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        self.core.supported_models()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        self.core.validate(request)
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenCodeCompatibleProvider, OpenCodeGoInnerSpec};
    use crate::provider::{LLMError, LLMProvider, LLMRequest, Message, ToolChoice};
    use crate::providers::{OpenCodeGoProvider, OpenCodeZenProvider};
    use std::sync::Arc;
    use vtcode_config::types::ReasoningEffortLevel;

    struct DispatcherCase {
        provider: Box<dyn LLMProvider>,
        name: &'static str,
        prefix: &'static str,
        model: &'static str,
    }

    fn dispatchers() -> [DispatcherCase; 2] {
        let client = reqwest::Client::new();
        [
            DispatcherCase {
                provider: Box::new(OpenCodeGoProvider::new_with_client(
                    "test-key".to_string(),
                    "glm-5.3".to_string(),
                    client.clone(),
                    "https://example.test/v1".to_string(),
                    Default::default(),
                )),
                name: "OpenCode Go",
                prefix: "opencode-go/",
                model: "glm-5.3",
            },
            DispatcherCase {
                provider: Box::new(OpenCodeZenProvider::new_with_client(
                    "test-key".to_string(),
                    "gpt-5.6-sol".to_string(),
                    client,
                    "https://example.test/v1".to_string(),
                    Default::default(),
                )),
                name: "OpenCode Zen",
                prefix: "opencode-zen/",
                model: "gpt-5.6-sol",
            },
        ]
    }

    #[test]
    fn dispatcher_validation_accepts_supported_prefixed_and_blank_models_without_mutation() {
        for DispatcherCase { provider, prefix, model, .. } in dispatchers() {
            for requested_model in [
                model.to_string(),
                format!("  {prefix}{model}  "),
                String::new(),
                "  ".to_string(),
            ] {
                let request = LLMRequest {
                    messages: vec![Message::user("hello".to_string())].into(),
                    model: requested_model.clone(),
                    ..Default::default()
                };
                assert!(provider.validate_request(&request).is_ok(), "{}: {requested_model:?}", provider.name());
                assert_eq!(request.model, requested_model);
            }
        }

        let provider = &dispatchers()[1].provider;
        let request = LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            model: "opencode/gpt-5.6-sol".to_string(),
            ..Default::default()
        };
        assert!(provider.validate_request(&request).is_ok());
    }

    #[test]
    fn dispatcher_validation_rejects_unknown_models_with_provider_context() {
        for DispatcherCase { provider, name, prefix, .. } in dispatchers() {
            let request = LLMRequest {
                messages: vec![Message::user("hello".to_string())].into(),
                model: format!("{prefix}unknown-model"),
                ..Default::default()
            };
            let LLMError::InvalidRequest { message, .. } = provider.validate_request(&request).unwrap_err() else {
                panic!("expected an invalid request");
            };
            assert!(message.contains(name), "{message}");
            assert!(message.contains("Unsupported model: unknown-model"), "{message}");
            assert!(!message.contains(prefix), "{message}");
            assert_eq!(request.model, format!("{prefix}unknown-model"));
        }
    }

    #[test]
    fn dispatcher_validation_keeps_provider_model_allowlists_separate() {
        for DispatcherCase { provider, prefix, model, .. } in dispatchers() {
            let other_model = if model == "glm-5.3" { "gpt-5.6-sol" } else { "glm-5.3" };
            let request = LLMRequest {
                messages: vec![Message::user("hello".to_string())].into(),
                model: format!("{prefix}{other_model}"),
                ..Default::default()
            };
            let LLMError::InvalidRequest { message, .. } = provider.validate_request(&request).unwrap_err() else {
                panic!("expected an invalid request");
            };
            assert!(message.contains(&format!("Unsupported model: {other_model}")), "{message}");
        }
    }

    #[test]
    fn dispatcher_validation_keeps_message_checks_after_model_normalization() {
        for DispatcherCase { provider, name, prefix, model } in dispatchers() {
            let request = LLMRequest {
                model: format!("{prefix}{model}"),
                ..Default::default()
            };
            let error = provider.validate_request(&request).unwrap_err().to_string();
            assert!(error.contains(name), "{error}");
            assert!(error.contains("Messages cannot be empty"), "{error}");

            let mut invalid_message = Message::user("hello".to_string());
            invalid_message.tool_calls = Some(Vec::new());
            let request = LLMRequest { messages: vec![invalid_message].into(), ..request };
            let error = provider.validate_request(&request).unwrap_err().to_string();
            assert!(error.contains(name), "{error}");
            assert!(error.contains("cannot make tool calls"), "{error}");
        }
    }

    fn base_request() -> LLMRequest {
        LLMRequest {
            messages: vec![Message::user("hello".to_string())].into(),
            system_prompt: Some(Arc::from("system guidance")),
            model: "some-model".to_string(),
            max_tokens: Some(512),
            temperature: Some(0.5),
            top_p: Some(0.25),
            stream: true,
            tool_choice: Some(ToolChoice::Auto),
            reasoning_effort: Some(ReasoningEffortLevel::High),
            metadata: Some(serde_json::json!({"user_id": "user-42"})),
            ..Default::default()
        }
    }

    #[test]
    fn golden_payload_basic_shape() {
        let provider = OpenCodeCompatibleProvider::<OpenCodeGoInnerSpec>::new(
            "test-key".to_string(),
            reqwest::Client::new(),
            "https://example.test/v1".to_string(),
            "some-model".to_string(),
        );

        let payload = provider.core.convert_request(&base_request()).unwrap();

        assert_eq!(payload["model"], "some-model");
        let messages = payload["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        // The system prompt is dropped entirely by the old hand-rolled path.
        assert!(payload.get("system").is_none());

        assert_eq!(payload["max_tokens"], 512);
        // Sampling suppression is not implemented in the old path.
        assert_eq!(payload["temperature"], 0.5);
        assert!(payload.get("top_p").is_none());

        assert_eq!(payload["stream"], true);
        assert!(payload.get("stream_options").is_none());
        assert!(payload.get("user_id").is_none());
        assert!(payload.get("thinking").is_none());
        assert!(payload.get("reasoning").is_none());
        assert!(payload.get("reasoning_effort").is_none());

        assert_eq!(payload["tool_choice"], "auto");
    }

    #[test]
    fn tool_catalog_serialization_is_stable_across_cached_calls() {
        use crate::provider::ToolDefinition;

        let provider = OpenCodeCompatibleProvider::<OpenCodeGoInnerSpec>::new(
            "test-key".to_string(),
            reqwest::Client::new(),
            "https://example.test/v1".to_string(),
            "some-model".to_string(),
        );

        // Stable catalog shared across requests (mirrors the session-stable
        // `prompt_bundle.request_tools` Arc).
        let tools = Arc::new(vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read a file".to_string(),
            serde_json::json!({"type": "object", "properties": {}}),
        )]);

        let mut req_a = base_request();
        req_a.tools = Some(Arc::clone(&tools));
        let mut req_b = base_request();
        req_b.tools = Some(Arc::clone(&tools));

        let payload_a = provider.core.convert_request(&req_a).unwrap();
        let payload_b = provider.core.convert_request(&req_b).unwrap();

        // The cached serialize path must produce a byte-identical `tools` array.
        assert_eq!(payload_a.get("tools"), payload_b.get("tools"));
        assert!(payload_a.get("tools").is_some());
    }
}
