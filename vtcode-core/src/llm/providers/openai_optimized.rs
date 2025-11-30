//! Optimized OpenAI provider using shared utilities
//!
//! This demonstrates how the new provider_base module reduces duplicate code
//! and excessive allocations in LLM provider implementations.

use crate::config::TimeoutsConfig;
use crate::config::constants::{env_vars, models, urls};
use crate::config::core::{OpenAIPromptCacheSettings, PromptCachingConfig};
use crate::llm::client::LLMClient;
use crate::llm::provider_base::{
    AuthHandler, AuthType, BaseProviderConfig, ErrorHandler, ModelResolver,
    RequestProcessor, StreamProcessor, OpenAICompatibleProvider,
};
use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent,
    Message, MessageRole, ToolCall, ToolChoice, ToolDefinition,
};
use crate::llm::utils as llm_utils;
use crate::llm::types as llm_types;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct OpenAIProvider {
    config: BaseProviderConfig,
    auth_handler: AuthHandler,
    error_handler: ErrorHandler,
    request_processor: RequestProcessor,
    stream_processor: StreamProcessor,
    model_resolver: ModelResolver,
    prompt_cache_enabled: bool,
    prompt_cache_settings: OpenAIPromptCacheSettings,
}

impl OpenAIProvider {
    pub fn new(api_key: String) -> Self {
        Self::with_model_internal(
            api_key,
            models::openai::DEFAULT_MODEL.to_string(),
            None,
            None,
        )
    }

    pub fn with_model(api_key: String, model: String) -> Self {
        Self::with_model_internal(api_key, model, None, None)
    }

    pub fn from_config(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        prompt_cache: Option<PromptCachingConfig>,
        timeouts: Option<TimeoutsConfig>,
    ) -> Self {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = model.unwrap_or_else(|| models::openai::DEFAULT_MODEL.to_string());

        Self::with_model_internal(api_key_value, model_value, prompt_cache, base_url)
    }

    fn with_model_internal(
        api_key: String,
        model: String,
        prompt_cache: Option<PromptCachingConfig>,
        base_url: Option<String>,
    ) -> Self {
        let (prompt_cache_enabled, prompt_cache_settings) = Self::extract_prompt_cache_settings(
            prompt_cache,
        );

        let config = BaseProviderConfig::from_options(
            Some(api_key.clone()),
            Some(model.clone()),
            base_url,
            models::openai::DEFAULT_MODEL,
            urls::OPENAI_API_BASE,
            env_vars::OPENAI_BASE_URL,
            None, // timeouts handled separately for now
        ).unwrap_or_else(|_| {
            // Fallback to manual config creation
            BaseProviderConfig {
                api_key: api_key.clone(),
                base_url: base_url.unwrap_or_else(|| urls::OPENAI_API_BASE.to_string()),
                model: model.clone(),
                http_client: reqwest::Client::new(),
                prompt_cache_enabled: false,
                request_timeout: std::time::Duration::from_secs(120),
                stream_timeout: std::time::Duration::from_secs(300),
            }
        });

        let auth_handler = AuthHandler::new(AuthType::BearerToken, api_key);
        let error_handler = ErrorHandler::new("OpenAI");
        let request_processor = RequestProcessor::new("OpenAI");
        let stream_processor = StreamProcessor::new("OpenAI", false);
        let model_resolver = ModelResolver::new(
            "OpenAI",
            models::openai::DEFAULT_MODEL,
            &models::openai::SUPPORTED_MODELS,
        );

        Self {
            config,
            auth_handler,
            error_handler,
            request_processor,
            stream_processor,
            model_resolver,
            prompt_cache_enabled,
            prompt_cache_settings,
        }
    }

    fn extract_prompt_cache_settings(
        prompt_cache: Option<PromptCachingConfig>,
    ) -> (bool, OpenAIPromptCacheSettings) {
        match prompt_cache {
            Some(cache_config) => {
                let enabled = cache_config.enabled && 
                    cache_config.openai.as_ref().map(|cfg| cfg.enabled).unwrap_or(false);
                let settings = cache_config.openai.unwrap_or_default();
                (enabled, settings)
            }
            None => (false, OpenAIPromptCacheSettings::default()),
        }
    }

    fn supports_tools(&self, model: &str) -> bool {
        // Simplified tool support check
        !matches!(model, 
            models::openai::GPT_5_NANO | models::openai::GPT_5_1_NANO
        )
    }

    fn supports_temperature_parameter(&self, model: &str) -> bool {
        // GPT-5 variants and GPT-5 Codex models don't support temperature parameter
        !matches!(model, 
            models::openai::GPT_5_CODEX | models::openai::GPT_5 | 
            models::openai::GPT_5_MINI | models::openai::GPT_5_NANO
        )
    }

    fn convert_to_openai_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        // Use shared utility for basic OpenAI format
        let mut openai_request = llm_utils::serialize_messages_openai_format(request, "OpenAI");

        // Add OpenAI-specific parameters
        if let Some(max_tokens) = request.max_tokens {
            openai_request["max_completion_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            if self.supports_temperature_parameter(&request.model) {
                openai_request["temperature"] = json!(temperature);
            }
        }

        // Add tools if supported
        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools {
                if let Some(serialized) = Self::serialize_tools(tools) {
                    openai_request["tools"] = serialized;

                    // Disable parallel tool calls for custom tools
                    let has_custom_tool = tools.iter().any(|tool| tool.tool_type == "custom");
                    if has_custom_tool {
                        openai_request["parallel_tool_calls"] = json!(false);
                    }
                }
            }

            if let Some(tool_choice) = &request.tool_choice {
                openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
            }

            if let Some(parallel) = request.parallel_tool_calls {
                if !openai_request.get("parallel_tool_calls").is_some() {
                    openai_request["parallel_tool_calls"] = json!(parallel);
                }
            }
        }

        Ok(openai_request)
    }

    fn serialize_tools(tools: &[ToolDefinition]) -> Option<Value> {
        if tools.is_empty() {
            return None;
        }

        let serialized_tools: Vec<Value> = tools
            .iter()
            .filter_map(|tool| {
                Some(match tool.tool_type.as_str() {
                    "function" => {
                        let func = tool.function.as_ref()?;
                        json!({
                            "type": "function",
                            "name": &func.name,
                            "description": &func.description,
                            "parameters": &func.parameters,
                            "function": {
                                "name": &func.name,
                                "description": &func.description,
                                "parameters": &func.parameters,
                            }
                        })
                    }
                    "apply_patch" | "shell" | "custom" | "grammar" => {
                        // For GPT-5.1 native tool types, use direct serialization
                        json!(tool)
                    }
                    _ => {
                        // Fallback for unknown tool types
                        json!(tool)
                    }
                })
            })
            .collect();

        Some(Value::Array(serialized_tools))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        // Use shared utility for basic parsing, then add OpenAI-specific logic
        let mut request = llm_utils::parse_chat_request_openai_format(value, &self.config.model)?;

        // Add OpenAI-specific parsing here if needed
        
        Some(request)
    }

    async fn make_request(&self, request: &LLMRequest, use_responses_api: bool) -> Result<LLMResponse, LLMError> {
        // Validate request
        self.error_handler.validate_request(request)?;

        // Build request body
        let body = if use_responses_api {
            self.convert_to_openai_responses_format(request)?
        } else {
            self.convert_to_openai_format(request)?
        };

        // Build URL
        let url = if use_responses_api {
            format!("{}/responses", self.config.base_url)
        } else {
            format!("{}/chat/completions", self.config.base_url)
        };

        // Make HTTP request using shared processor
        let response = self.request_processor
            .build_request(
                &self.config.http_client,
                reqwest::Method::POST,
                url,
                Some(&self.auth_handler),
                Some(body),
            )?
            .send()
            .await
            .map_err(|e| LLMError::Network(e.to_string()))?;

        // Handle response
        if request.stream {
            // Handle streaming separately
            return Err(LLMError::Provider("Streaming not implemented in this demo".to_string()));
        }

        let response_json = self.request_processor.handle_response(response).await?;
        
        // Parse response using shared utility
        llm_utils::parse_response_openai_format(
            response_json,
            "OpenAI",
            self.prompt_cache_enabled,
            None,
        )
    }

    fn convert_to_openai_responses_format(&self, request: &LLMRequest) -> Result<Value, LLMError> {
        // Simplified responses API format
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|msg| {
                json!({
                    "role": msg.role.as_openai_str(),
                    "content": msg.content.as_text()
                })
            })
            .collect();

        if messages.is_empty() {
            return Err(LLMError::InvalidRequest(
                self.error_handler.handle_http_error(
                    reqwest::StatusCode::BAD_REQUEST,
                    "No messages provided for Responses API"
                ).to_string()
            ));
        }

        let mut openai_request = json!({
            "model": request.model,
            "input": messages,
            "stream": request.stream
        });

        if let Some(max_tokens) = request.max_tokens {
            openai_request["max_output_tokens"] = json!(max_tokens);
        }

        if let Some(temperature) = request.temperature {
            if self.supports_temperature_parameter(&request.model) {
                openai_request["temperature"] = json!(temperature);
            }
        }

        // Add tools
        if self.supports_tools(&request.model) {
            if let Some(tools) = &request.tools {
                if let Some(serialized) = Self::serialize_tools(tools) {
                    openai_request["tools"] = serialized;
                }
            }

            if let Some(tool_choice) = &request.tool_choice {
                openai_request["tool_choice"] = tool_choice.to_provider_format("openai");
            }
        }

        Ok(openai_request)
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    async fn chat(&self, request: &LLMRequest) -> Result<LLMResponse, LLMError> {
        // Determine if we should use responses API
        let use_responses_api = matches!(request.model.as_str(), 
            models::openai::GPT_5 | models::openai::GPT_5_CODEX
        );

        self.make_request(request, use_responses_api).await
    }

    async fn chat_stream(&self, request: &LLMRequest) -> Result<LLMStream, LLMError> {
        // Validate request
        self.error_handler.validate_request(request)?;

        // Build request body
        let use_responses_api = matches!(request.model.as_str(), 
            models::openai::GPT_5 | models::openai::GPT_5_CODEX
        );

        let body = if use_responses_api {
            self.convert_to_openai_responses_format(request)?
        } else {
            self.convert_to_openai_format(request)?
        };

        // Build URL
        let url = if use_responses_api {
            format!("{}/responses", self.config.base_url)
        } else {
            format!("{}/chat/completions", self.config.base_url)
        };

        // Make streaming request
        let response = self.request_processor
            .build_request(
                &self.config.http_client,
                reqwest::Method::POST,
                url,
                Some(&self.auth_handler),
                Some(body),
            )?
            .send()
            .await
            .map_err(|e| LLMError::Network(e.to_string()))?;

        // Handle streaming response
        let stream = self.request_processor.handle_stream_response(response).await?;
        
        Ok(Box::pin(try_stream! {
            futures::pin_mut!(stream);
            
            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result.map_err(|e| LLMError::Network(e.to_string()))?;
                
                // Process stream chunk using shared processor
                let events = self.stream_processor.process_stream_chunk(&chunk);
                
                for event in events {
                    yield event;
                }
            }
        }))
    }

    fn parse_chat_request(&self, value: &Value) -> Option<LLMRequest> {
        self.parse_chat_request(value)
    }

    fn get_model(&self) -> &str {
        &self.config.model
    }
}

// Implement OpenAICompatibleProvider trait for additional compatibility
impl OpenAICompatibleProvider for OpenAIProvider {
    fn provider_name(&self) -> &'static str {
        "OpenAI"
    }

    fn supports_prompt_caching(&self) -> bool {
        self.prompt_cache_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_creation() {
        let provider = OpenAIProvider::new("test_key".to_string());
        assert_eq!(provider.config.api_key, "test_key");
        assert_eq!(provider.config.model, models::openai::DEFAULT_MODEL);
    }

    #[test]
    fn test_tool_support() {
        let provider = OpenAIProvider::new("test_key".to_string());
        
        assert!(provider.supports_tools(models::openai::GPT_5));
        assert!(provider.supports_tools(models::openai::GPT_5_MINI));
        assert!(!provider.supports_tools(models::openai::GPT_5_NANO));
    }

    #[test]
    fn test_temperature_support() {
        let provider = OpenAIProvider::new("test_key".to_string());
        
        assert!(!provider.supports_temperature_parameter(models::openai::GPT_5_CODEX));
        assert!(!provider.supports_temperature_parameter(models::openai::GPT_5));
        assert!(provider.supports_temperature_parameter(models::openai::GPT_5_MINI));
    }
}