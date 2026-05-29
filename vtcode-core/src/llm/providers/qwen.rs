use async_stream::try_stream;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::{Map, Value};

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, ModelConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
};
use crate::llm::types as llm_types;

use super::{
    common::{
        extract_prompt_cache_settings_default, map_finish_reason_common, override_base_url,
        parse_response_openai_format, resolve_model, serialize_messages_openai_format,
        serialize_tools_openai_format, validate_request_common,
    },
    error_handling::handle_openai_http_error,
    extract_reasoning_trace,
};

const PROVIDER_NAME: &str = "Qwen";
const PROVIDER_KEY: &str = "qwen";

pub struct QwenProvider {
    api_key: String,
    http_client: HttpClient,
    base_url: String,
    model: String,
    prompt_cache_enabled: bool,
    model_behavior: Option<ModelConfig>,
}

impl QwenProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::qwen::DEFAULT_MODEL.to_string(),
            None,
            None,
            TimeoutsConfig::default(),
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None, TimeoutsConfig::default(), None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        _timeouts: TimeoutsConfig,
    ) -> Self {
        Self {
            api_key,
            http_client,
            base_url,
            model,
            prompt_cache_enabled: false,
            model_behavior: None,
        }
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        let api_key_value = api_key
            .filter(|k| !k.trim().is_empty())
            .or_else(|| {
                std::env::var("QWEN_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
            })
            .or_else(|| {
                std::env::var("DASHSCOPE_API_KEY")
                    .ok()
                    .filter(|k| !k.trim().is_empty())
            })
            .unwrap_or_default();

        Self::with_model_internal(
            api_key_value,
            resolve_model(model, models::qwen::DEFAULT_MODEL),
            prompt_cache,
            base_url,
            timeouts.unwrap_or_default(),
            model_behavior,
        )
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
        timeouts: TimeoutsConfig,
        model_behavior: Option<ModelConfig>,
    ) -> Self {
        use crate::llm::http_client::HttpClientFactory;

        let (prompt_cache_enabled, _) =
            extract_prompt_cache_settings_default(prompt_cache, PROVIDER_KEY);

        Self {
            api_key,
            http_client: HttpClientFactory::for_llm(&timeouts),
            base_url: override_base_url(
                urls::QWEN_API_BASE,
                base_url,
                Some(env_vars::QWEN_BASE_URL),
            ),
            model,
            prompt_cache_enabled,
            model_behavior,
        }
    }

    #[must_use]
    #[inline]
    fn is_thinking_enabled(request: &LLMRequest) -> bool {
        request
            .reasoning_effort
            .is_some_and(|e| e != crate::config::types::ReasoningEffortLevel::None)
    }

    fn float_to_json_number(value: f32) -> Result<serde_json::Number, LLMError> {
        serde_json::Number::from_f64(value as f64).ok_or_else(|| LLMError::InvalidRequest {
            message: "invalid numeric parameter value (NaN or infinity)".to_string(),
            metadata: None,
        })
    }

    fn convert_to_qwen_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        let mut payload = Map::with_capacity(12);

        payload.insert("model".to_owned(), Value::String(request.model.clone()));

        let mut messages = self.serialize_messages(request)?;

        if let Some(system_prompt) = &request.system_prompt {
            let trimmed = system_prompt.trim();
            if !trimmed.is_empty() {
                messages.insert(0, serde_json::json!({"role": "system", "content": trimmed}));
            }
        }

        payload.insert("messages".to_owned(), Value::Array(messages));

        if let Some(max_tokens) = request.max_tokens {
            payload.insert(
                "max_tokens".to_owned(),
                Value::Number(serde_json::Number::from(max_tokens as u64)),
            );
        }

        let thinking_enabled = Self::is_thinking_enabled(request);

        if !thinking_enabled {
            if let Some(temperature) = request.temperature {
                payload.insert(
                    "temperature".to_owned(),
                    Value::Number(Self::float_to_json_number(temperature)?),
                );
            }

            if let Some(top_p) = request.top_p {
                payload.insert(
                    "top_p".to_owned(),
                    Value::Number(Self::float_to_json_number(top_p)?),
                );
            }
        }

        if request.stream {
            payload.insert("stream".to_string(), Value::Bool(true));
            payload.insert(
                "stream_options".to_string(),
                serde_json::json!({"include_usage": true}),
            );
        }

        if let Some(tools) = &request.tools
            && let Some(serialized_tools) = serialize_tools_openai_format(tools)
        {
            payload.insert("tools".to_string(), Value::Array(serialized_tools));
        }

        if let Some(choice) = &request.tool_choice {
            payload.insert(
                "tool_choice".to_string(),
                choice.to_provider_format(PROVIDER_KEY),
            );
        }

        if let Some(effort) = request.reasoning_effort {
            let enable_thinking = effort != crate::config::types::ReasoningEffortLevel::None;
            payload.insert("enable_thinking".to_owned(), Value::Bool(enable_thinking));
        }

        if let Some(meta) = &request.metadata {
            if let Some(user_id) = meta.get("user_id").and_then(|v| v.as_str()) {
                payload.insert("user_id".to_owned(), Value::String(user_id.to_owned()));
            }
        }

        Ok(Value::Object(payload))
    }

    async fn send_request(&self, payload: &Value) -> Result<reqwest::Response, LLMError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        self.http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(payload)
            .send()
            .await
            .map_err(|e| LLMError::Network {
                message: error_display::format_llm_error(
                    PROVIDER_NAME,
                    &format!("network error: {}", e),
                ),
                metadata: None,
            })
    }

    fn serialize_messages(&self, request: &LLMRequest) -> Result<Vec<Value>, LLMError> {
        serialize_messages_openai_format(request, PROVIDER_KEY)
    }

    fn parse_response(&self, response_json: Value, model: String) -> Result<LLMResponse, LLMError> {
        let reasoning_extractor = |message: &Value, choice: &Value| {
            message
                .get("reasoning_content")
                .and_then(extract_reasoning_trace)
                .or_else(|| {
                    choice
                        .get("reasoning_content")
                        .and_then(extract_reasoning_trace)
                })
        };

        parse_response_openai_format(
            response_json,
            PROVIDER_NAME,
            model,
            self.prompt_cache_enabled,
            Some(reasoning_extractor),
        )
    }
}

#[async_trait]
impl LLMProvider for QwenProvider {
    fn name(&self) -> &str {
        PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        true
    }

    fn supports_vision(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning)
            .unwrap_or(false)
            || requested == models::qwen::QWEN3_7_MAX
            || requested == models::qwen::QWEN3_6_FLASH
            || requested == models::qwen::QWEN3_6_PLUS
            || requested == models::qwen::DEEPSEEK_V4_FLASH
            || requested == models::qwen::DEEPSEEK_V4_PRO
            || requested == models::qwen::GLM_5_1
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        self.model_behavior
            .as_ref()
            .and_then(|b| b.model_supports_reasoning_effort)
            .unwrap_or(false)
    }

    fn effective_context_size(&self, model: &str) -> usize {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };
        match requested {
            models::qwen::QWEN3_6_FLASH
            | models::qwen::DEEPSEEK_V4_FLASH
            | models::qwen::DEEPSEEK_V4_PRO => 1_048_576,
            models::qwen::QWEN3_7_MAX | models::qwen::QWEN3_6_PLUS | models::qwen::GLM_5_1 => {
                131_072
            }
            _ => 131_072,
        }
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut request = request;
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }
        let model = request.model.clone();

        let payload = self.convert_to_qwen_format(&request)?;
        let response = self.send_request(&payload).await?;
        let response = handle_openai_http_error(response, PROVIDER_NAME, "QWEN_API_KEY").await?;

        let response_json: Value = response.json().await.map_err(|e| LLMError::Provider {
            message: error_display::format_llm_error(
                PROVIDER_NAME,
                &format!("failed to parse response: {}", e),
            ),
            metadata: None,
        })?;

        self.parse_response(response_json, model)
    }

    async fn stream(&self, mut request: LLMRequest) -> Result<LLMStream, LLMError> {
        if request.model.trim().is_empty() {
            request.model = self.model.clone();
        }

        self.validate_request(&request)?;
        request.stream = true;
        let model = request.model.clone();

        let payload = self.convert_to_qwen_format(&request)?;
        let response = self.send_request(&payload).await?;
        let response = handle_openai_http_error(response, PROVIDER_NAME, "QWEN_API_KEY").await?;

        let bytes_stream = response.bytes_stream();
        let (event_tx, event_rx) =
            tokio::sync::mpsc::unbounded_channel::<Result<LLMStreamEvent, LLMError>>();
        let tx = event_tx.clone();

        let model_clone = model.clone();
        tokio::spawn(async move {
            let mut aggregator =
                crate::llm::providers::shared::StreamAggregator::new(model_clone.clone());

            let result = crate::llm::providers::shared::process_openai_stream(
                bytes_stream,
                PROVIDER_NAME,
                model_clone,
                |value| {
                    if let Some(choices) = value.get("choices").and_then(|c| c.as_array())
                        && let Some(choice) = choices.first()
                    {
                        if let Some(delta) = choice.get("delta") {
                            if let Some(reasoning) =
                                delta.get("reasoning_content").and_then(|r| r.as_str())
                            {
                                if let Some(delta) = aggregator.handle_reasoning(reasoning) {
                                    let _ = tx.send(Ok(LLMStreamEvent::Reasoning { delta }));
                                }
                            }

                            if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                for event in aggregator.handle_content(content) {
                                    let _ = tx.send(Ok(event));
                                }
                            }

                            if let Some(tool_calls) =
                                delta.get("tool_calls").and_then(|tc| tc.as_array())
                            {
                                aggregator.handle_tool_calls(tool_calls);
                            }
                        }

                        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                            aggregator.set_finish_reason(map_finish_reason_common(reason));
                        }
                    }

                    if let Some(_usage_value) = value.get("usage") {
                        if let Some(usage) =
                            crate::llm::providers::common::parse_usage_openai_format(&value, true)
                        {
                            aggregator.set_usage(usage);
                        }
                    }
                    Ok(())
                },
            )
            .await;

            match result {
                Ok(_) => {
                    let response = aggregator.finalize();
                    let _ = tx.send(Ok(LLMStreamEvent::Completed {
                        response: Box::new(response),
                    }));
                }
                Err(err) => {
                    let _ = tx.send(Err(err));
                }
            }
        });

        let stream = try_stream! {
            let mut receiver = event_rx;
            while let Some(event) = receiver.recv().await {
                yield event?;
            }
        };

        Ok(Box::pin(stream))
    }

    fn supported_models(&self) -> Vec<String> {
        models::qwen::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        let supported_models = models::qwen::SUPPORTED_MODELS
            .iter()
            .map(|model| model.to_string())
            .collect::<Vec<_>>();

        validate_request_common(
            request,
            PROVIDER_NAME,
            PROVIDER_KEY,
            Some(&supported_models),
        )
    }
}

#[async_trait]
impl LLMClient for QwenProvider {
    async fn generate(&mut self, prompt: &str) -> Result<LLMResponse, LLMError> {
        let request = super::common::make_default_request(prompt, &self.model);
        Ok(LLMProvider::generate(self, request).await?)
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::Qwen
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
