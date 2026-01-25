#![allow(clippy::result_large_err)]
use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{AnthropicConfig, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::error_display;
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, Message as ProviderMessage,
};
use crate::llm::providers::openai::responses_api::{
    build_standard_responses_payload, parse_responses_payload,
};
use crate::llm::types as llm_types;
use async_trait::async_trait;
use reqwest::Client as HttpClient;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

use super::common::{extract_prompt_cache_settings, override_base_url, resolve_model};

/// xAI provider that uses the new Responses API
pub struct XAIProvider {
    api_key: Arc<str>,
    http_client: HttpClient,
    base_url: Arc<str>,
    model: Arc<str>,
    prompt_cache_enabled: bool,
    prompt_cache_settings: crate::config::core::OpenAIPromptCacheSettings,
}

impl XAIProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(api_key, models::xai::DEFAULT_MODEL.to_string(), None)
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None)
    }

    pub fn new_with_client(
        api_key: String,
        model: String,
        http_client: reqwest::Client,
        base_url: String,
        timeouts: TimeoutsConfig,
    ) -> Self {
        Self::from_config_with_client(
            Some(api_key),
            Some(model),
            Some(base_url),
            None,
            Some(timeouts),
            None,
            http_client,
        )
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
    ) -> Self {
        let resolved_model = resolve_model(model, models::xai::DEFAULT_MODEL);
        let resolved_base_url =
            override_base_url(urls::XAI_API_BASE, base_url, Some(env_vars::XAI_BASE_URL));

        let (prompt_cache_enabled, prompt_cache_settings) = extract_prompt_cache_settings(
            prompt_cache,
            |providers| &providers.openai, // Use OpenAI settings since xAI follows OpenAI format
            |cfg, provider_settings| cfg.enabled && provider_settings.enabled,
        );

        // Use centralized HTTP client factory for consistent timeout handling
        use crate::llm::http_client::HttpClientFactory;
        let http_client =
            HttpClientFactory::with_timeouts(Duration::from_secs(120), Duration::from_secs(30));

        Self {
            api_key: Arc::from(api_key.unwrap_or_default().as_str()),
            http_client,
            base_url: Arc::from(resolved_base_url.as_str()),
            model: Arc::from(resolved_model.as_str()),
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn from_config_with_client(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        _timeouts: Option<TimeoutsConfig>,
        _anthropic: Option<AnthropicConfig>,
        http_client: reqwest::Client,
    ) -> Self {
        let resolved_model = resolve_model(model, models::xai::DEFAULT_MODEL);
        let resolved_base_url =
            override_base_url(urls::XAI_API_BASE, base_url, Some(env_vars::XAI_BASE_URL));

        let (prompt_cache_enabled, prompt_cache_settings) = if let Some(cfg) = prompt_cache {
            let enabled = cfg.enabled && cfg.providers.openai.enabled;
            (enabled, cfg.providers.openai.clone())
        } else {
            (false, Default::default())
        };

        Self {
            api_key: Arc::from(api_key.unwrap_or_default().as_str()),
            http_client,
            base_url: Arc::from(resolved_base_url.as_str()),
            model: Arc::from(resolved_model.as_str()),
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
    ) -> Self {
        Self::from_config(Some(api_key), Some(model), None, prompt_cache, None, None)
    }

    fn authorize(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.api_key.trim().is_empty() {
            builder
        } else {
            builder.bearer_auth(&self.api_key)
        }
    }
}

#[async_trait]
impl LLMProvider for XAIProvider {
    fn name(&self) -> &str {
        "xai"
    }

    fn supports_reasoning(&self, model: &str) -> bool {
        let requested = if model.trim().is_empty() {
            &self.model
        } else {
            model
        };

        requested == models::xai::GROK_4
            || requested == models::xai::GROK_4_CODE
            || requested == models::xai::GROK_4_CODE_LATEST
            || requested == models::xai::GROK_4_1_FAST
            || requested == models::xai::GROK_CODE_FAST_1
            || requested == models::xai::GROK_4_FAST
            || requested == models::xai::GROK_3
            || requested == models::xai::GROK_3_MINI
    }

    fn supports_reasoning_effort(&self, _model: &str) -> bool {
        false // xAI reasoning is built-in and automatic for reasoning-capable models
    }

    fn supports_parallel_tool_config(&self, _model: &str) -> bool {
        false // xAI follows standard OpenAI tool calling, not yet confirmed for parallel_tool_config payload
    }

    async fn generate(&self, mut request: LLMRequest) -> Result<LLMResponse, LLMError> {
        if !self.prompt_cache_enabled {
            // xAI prompt caching is managed by the platform; no additional parameters required.
        }

        if request.model.trim().is_empty() {
            request.model = self.model.to_string();
        }

        // Use the new Responses API endpoint instead of the deprecated /v1/messages
        let responses_payload = build_standard_responses_payload(&request)?;

        if responses_payload.input.is_empty() {
            let formatted =
                error_display::format_llm_error("xAI", "No messages provided for Responses API");
            return Err(LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }

        let mut xai_request = json!({
            "model": request.model,
            "input": responses_payload.input,
            "stream": request.stream,
        });

        // 'output_types' is part of the xAI Responses API spec
        xai_request["output_types"] = json!(["message", "tool_call"]);

        if let Some(instructions) = responses_payload.instructions {
            if !instructions.trim().is_empty() {
                xai_request["instructions"] = json!(instructions);
            }
        }

        // Add tools if present
        if let Some(tools) = &request.tools {
            if !tools.is_empty() {
                let serialized_tools: Vec<serde_json::Value> = tools
                    .iter()
                    .filter_map(|tool| {
                        if tool.tool_type == "function" {
                            if let Some(func) = &tool.function {
                                Some(json!({
                                    "type": "function",
                                    "name": &func.name,
                                    "description": &func.description,
                                    "parameters": &func.parameters
                                }))
                            } else {
                                None
                            }
                        } else {
                            // For non-function tools, use the basic format
                            Some(json!(tool))
                        }
                    })
                    .collect();

                if !serialized_tools.is_empty() {
                    xai_request["tools"] = serde_json::Value::Array(serialized_tools);

                    // Add tool_choice if specified
                    if let Some(tool_choice) = &request.tool_choice {
                        xai_request["tool_choice"] = tool_choice.to_provider_format("xai");
                    }
                }
            }
        }

        // If configured, include the `prompt_cache_retention` value in the Responses API
        // request for xAI models
        if self.prompt_cache_enabled {
            if let Some(ref retention) = self.prompt_cache_settings.prompt_cache_retention {
                if !retention.trim().is_empty() {
                    xai_request["prompt_cache_retention"] = json!(retention);
                }
            }
        }

        let url = format!("{}/responses", self.base_url);

        let response = self
            .authorize(self.http_client.post(&url))
            .header("Content-Type", "application/json")
            .json(&xai_request)
            .send()
            .await
            .map_err(|e| {
                let formatted =
                    error_display::format_llm_error("xAI", &format!("Network error: {}", e));
                LLMError::Network {
                    message: formatted,
                    metadata: None,
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();

            if status.as_u16() == 429
                || error_text.contains("insufficient_quota")
                || error_text.contains("quota")
                || error_text.contains("rate limit")
            {
                return Err(LLMError::RateLimit { metadata: None });
            }

            let formatted =
                error_display::format_llm_error("xAI", &format!("HTTP {}: {}", status, error_text));
            return Err(LLMError::Provider {
                message: formatted,
                metadata: None,
            });
        }

        let xai_response: serde_json::Value = response.json().await.map_err(|e| {
            let formatted =
                error_display::format_llm_error("xAI", &format!("Failed to parse response: {}", e));
            LLMError::Provider {
                message: formatted,
                metadata: None,
            }
        })?;

        let include_metrics =
            self.prompt_cache_enabled && self.prompt_cache_settings.surface_metrics;
        parse_responses_payload(xai_response, include_metrics)
    }

    fn supported_models(&self) -> Vec<String> {
        models::xai::SUPPORTED_MODELS
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        if request.messages.is_empty() {
            let formatted = error_display::format_llm_error("xAI", "Messages cannot be empty");
            return Err(LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }

        if !request.model.trim().is_empty()
            && !models::xai::SUPPORTED_MODELS
                .iter()
                .any(|m| *m == request.model)
        {
            let formatted = error_display::format_llm_error(
                "xAI",
                &format!("Unsupported model: {}", request.model),
            );
            return Err(LLMError::InvalidRequest {
                message: formatted,
                metadata: None,
            });
        }

        for message in &request.messages {
            if let Err(err) = message.validate_for_provider("openai") {
                let formatted = error_display::format_llm_error("xAI", &err);
                return Err(LLMError::InvalidRequest {
                    message: formatted,
                    metadata: None,
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl LLMClient for XAIProvider {
    async fn generate(&mut self, prompt: &str) -> Result<llm_types::LLMResponse, LLMError> {
        // Parse the prompt into an LLMRequest
        let request = LLMRequest {
            messages: vec![ProviderMessage::user(prompt.to_string())],
            model: self.model.to_string(),
            ..Default::default()
        };

        let response = LLMProvider::generate(self, request).await?;

        Ok(llm_types::LLMResponse {
            content: response.content.unwrap_or_default(),
            model: self.model.to_string(),
            usage: response
                .usage
                .map(|usage| crate::llm::providers::common::convert_usage_to_llm_types(usage)),
            reasoning: response.reasoning,
            reasoning_details: response.reasoning_details,
            request_id: response.request_id,
            organization_id: response.organization_id,
        })
    }

    fn backend_kind(&self) -> llm_types::BackendKind {
        llm_types::BackendKind::XAI
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
