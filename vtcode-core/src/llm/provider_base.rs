//! Base traits and utilities for LLM providers
//!
//! This module provides shared functionality to eliminate duplicate code
//! across the 15+ LLM provider implementations.

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::Value;
use std::time::Duration;

use crate::config::TimeoutsConfig;
use crate::llm::{LLMError, LLMStreamEvent, provider::LLMRequest};

/// Default timeout configurations
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
pub const DEFAULT_STREAM_TIMEOUT: Duration = Duration::from_secs(300);

/// Base configuration shared by all providers
#[derive(Debug, Clone)]
pub struct BaseProviderConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub http_client: HttpClient,
    pub prompt_cache_enabled: bool,
    pub request_timeout: Duration,
    pub stream_timeout: Duration,
}

impl BaseProviderConfig {
    /// Create base configuration from common parameters
    pub fn from_options(
        api_key: Option<String>,
        model: Option<String>,
        base_url: Option<String>,
        default_model: &'static str,
        default_url: &'static str,
        env_var: &'static str,
        timeouts: Option<TimeoutsConfig>,
    ) -> Result<Self> {
        let api_key_value = api_key.unwrap_or_default();
        let model_value = model.unwrap_or_else(|| default_model.to_string());
        let base_url_value = Self::resolve_base_url(base_url, default_url, env_var)?;

        let timeout_config = timeouts.unwrap_or_default();
        let http_timeout = timeout_config
            .ceiling_duration(timeout_config.streaming_ceiling_seconds)
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT);
        let http_client = HttpClient::builder()
            .timeout(http_timeout)
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            api_key: api_key_value,
            base_url: base_url_value,
            model: model_value,
            http_client,
            prompt_cache_enabled: false,
            request_timeout: http_timeout,
            stream_timeout: timeout_config
                .ceiling_duration(timeout_config.streaming_ceiling_seconds)
                .unwrap_or(DEFAULT_STREAM_TIMEOUT),
        })
    }

    /// Resolve base URL with environment variable fallback
    fn resolve_base_url(
        base_url: Option<String>,
        default_url: &'static str,
        env_var: &'static str,
    ) -> Result<String> {
        if let Some(url) = base_url {
            Ok(url.trim().to_string())
        } else if let Ok(env_val) = std::env::var(env_var) {
            Ok(env_val.trim().to_string())
        } else {
            Ok(default_url.to_string())
        }
    }

    /// Validate that required API key is present
    pub fn validate_api_key(&self) -> Result<()> {
        if self.api_key.is_empty() {
            anyhow::bail!("API key is required")
        }
        Ok(())
    }
}

/// Trait for providers that support standard OpenAI-compatible APIs
#[async_trait]
pub trait OpenAICompatibleProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn supports_prompt_caching(&self) -> bool;

    /// Parse request from OpenAI format
    fn parse_openai_request(&self, value: &Value, default_model: &str) -> Option<LLMRequest> {
        crate::llm::utils::parse_chat_request_openai_format(value, default_model)
    }

    /// Serialize messages to OpenAI format
    fn serialize_openai_messages(&self, request: &LLMRequest) -> Value {
        crate::llm::utils::serialize_messages_openai_format(request, self.provider_name())
    }

    /// Parse response from OpenAI format
    fn parse_openai_response(
        &self,
        response: Value,
        include_cache: bool,
    ) -> Result<crate::llm::provider::LLMResponse> {
        crate::llm::utils::parse_response_openai_format(
            response,
            self.provider_name(),
            include_cache,
            None,
        )
    }
}

/// Shared error handling utilities
pub struct ErrorHandler {
    _provider_name: &'static str,
}

impl ErrorHandler {
    pub fn new(provider_name: &'static str) -> Self {
        Self {
            _provider_name: provider_name,
        }
    }

    /// Handle HTTP errors consistently across providers
    pub fn handle_http_error(&self, status: reqwest::StatusCode, error_text: &str) -> LLMError {
        use reqwest::StatusCode;

        let error_message = match status {
            StatusCode::UNAUTHORIZED => format!("Authentication failed: Invalid API key"),
            StatusCode::TOO_MANY_REQUESTS => format!("Rate limit exceeded"),
            StatusCode::BAD_REQUEST => format!("Bad request: {}", error_text.trim()),
            s if s.as_u16() == 402 => format!("Insufficient balance"),
            _ => format!("HTTP {}: {}", status, error_text.trim()),
        };

        let formatted_error =
            crate::llm::error_display::format_llm_error(self._provider_name, &error_message);

        // Handle different error types based on status code
        if status == StatusCode::UNAUTHORIZED {
            LLMError::ApiError(formatted_error)
        } else if status == StatusCode::TOO_MANY_REQUESTS {
            LLMError::RateLimit
        } else if status.as_u16() == 402 {
            LLMError::ApiError(formatted_error)
        } else {
            LLMError::ApiError(formatted_error)
        }
    }

    /// Handle request validation errors
    pub fn validate_request(&self, request: &LLMRequest) -> Result<()> {
        if request.messages.is_empty() {
            anyhow::bail!("Request must contain at least one message")
        }

        if request.model.is_empty() {
            anyhow::bail!("Request must specify a model")
        }

        // Check if model is supported (this would need to be customized per provider)
        if !self.is_model_supported(&request.model) {
            anyhow::bail!("Unsupported model: {}", request.model)
        }

        Ok(())
    }

    /// Check if model is supported (default implementation, override as needed)
    fn is_model_supported(&self, model: &str) -> bool {
        // Default implementation assumes all models are supported
        // Individual providers should override this with their specific model lists
        !model.is_empty()
    }
}

/// Shared streaming utilities
pub struct StreamProcessor {
    provider_name: &'static str,
    supports_reasoning: bool,
}

impl StreamProcessor {
    pub fn new(provider_name: &'static str, supports_reasoning: bool) -> Self {
        Self {
            provider_name,
            supports_reasoning,
        }
    }

    /// Process SSE stream chunk consistently
    pub fn process_stream_chunk(&self, chunk: &str) -> Vec<LLMStreamEvent> {
        let mut events = Vec::new();

        for line in chunk.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    // Stream completion indicated by DONE marker
                    continue;
                }

                match serde_json::from_str::<Value>(data) {
                    Ok(json) => {
                        if let Some(event) = self.parse_stream_event(json) {
                            events.push(event);
                        }
                    }
                    Err(_) => {
                        // Skip invalid JSON
                        continue;
                    }
                }
            }
        }

        events
    }

    /// Parse individual stream event (override for provider-specific logic)
    fn parse_stream_event(&self, json: Value) -> Option<LLMStreamEvent> {
        // Default implementation for OpenAI-compatible providers
        crate::llm::utils::parse_stream_event_openai_format(json, self.provider_name)
    }

    /// Extract reasoning content if supported
    pub fn extract_reasoning(&self, content: &str) -> (Vec<String>, Option<String>) {
        if !self.supports_reasoning {
            return (Vec::new(), None);
        }

        // Default implementation - providers can override
        crate::llm::utils::extract_reasoning_content(content)
    }
}

/// Unified authentication header handling
pub struct AuthHandler {
    auth_type: AuthType,
    api_key: String,
}

#[derive(Debug, Clone, Copy)]
pub enum AuthType {
    BearerToken,
    ApiKeyHeader(&'static str),
    QueryParam(&'static str),
}

impl AuthHandler {
    pub fn new(auth_type: AuthType, api_key: String) -> Self {
        Self { auth_type, api_key }
    }

    /// Apply authentication to request builder
    pub fn apply_auth(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.auth_type {
            AuthType::BearerToken => builder.bearer_auth(&self.api_key),
            AuthType::ApiKeyHeader(header_name) => builder.header(header_name, &self.api_key),
            AuthType::QueryParam(param_name) => builder.query(&[(param_name, &self.api_key)]),
        }
    }
}

/// Shared request/response processing utilities
pub struct RequestProcessor {
    provider_name: &'static str,
}

impl RequestProcessor {
    pub fn new(provider_name: &'static str) -> Self {
        Self { provider_name }
    }

    /// Build HTTP request with consistent error handling
    pub async fn build_request(
        &self,
        client: &HttpClient,
        method: reqwest::Method,
        url: String,
        auth: Option<&AuthHandler>,
        body: Option<Value>,
    ) -> Result<reqwest::RequestBuilder> {
        let mut builder = client.request(method, &url);

        if let Some(auth_handler) = auth {
            builder = auth_handler.apply_auth(builder);
        }

        builder = builder
            .header("Content-Type", "application/json")
            .header("User-Agent", "VTCode/1.0");

        if let Some(body_value) = body {
            builder = builder.json(&body_value);
        }

        Ok(builder)
    }

    /// Handle response with consistent error processing
    pub async fn handle_response(&self, response: reqwest::Response) -> Result<Value> {
        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let error_handler = ErrorHandler::new(self.provider_name);
            return Err(error_handler.handle_http_error(status, &error_text).into());
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response body")?;

        serde_json::from_str(&response_text).context("Failed to parse JSON response")
    }

    /// Handle streaming response
    pub async fn handle_stream_response(
        &self,
        response: reqwest::Response,
    ) -> Result<impl futures::Stream<Item = Result<String>>> {
        let status = response.status();

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            let error_handler = ErrorHandler::new(self.provider_name);
            return Err(error_handler.handle_http_error(status, &error_text).into());
        }

        Ok(response.bytes_stream().map(|result| {
            result
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                .map_err(|e| anyhow::anyhow!("Stream error: {}", e))
        }))
    }
}

/// Common model resolution utilities
pub struct ModelResolver {
    #[allow(dead_code)]
    provider_name: &'static str,
    default_model: &'static str,
    supported_models: &'static [&'static str],
}

impl ModelResolver {
    pub fn new(
        provider_name: &'static str,
        default_model: &'static str,
        supported_models: &'static [&'static str],
    ) -> Self {
        Self {
            provider_name,
            default_model,
            supported_models,
        }
    }

    /// Resolve model with fallback to default
    pub fn resolve_model(&self, model: Option<String>) -> String {
        model.unwrap_or_else(|| self.default_model.to_string())
    }

    /// Validate model is supported
    pub fn validate_model(&self, model: &str) -> Result<()> {
        if self.supported_models.is_empty() {
            // If no specific supported models listed, accept any non-empty model
            if model.is_empty() {
                anyhow::bail!("Model cannot be empty")
            }
            return Ok(());
        }

        if !self.supported_models.contains(&model) {
            anyhow::bail!(
                "Unsupported model: {}. Supported models: {:?}",
                model,
                self.supported_models
            )
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_provider_config() {
        let config = BaseProviderConfig::from_options(
            Some("test_key".to_string()),
            Some("test_model".to_string()),
            None,
            "default_model",
            "https://api.example.com",
            "TEST_API_KEY",
            None,
        )
        .unwrap();

        assert_eq!(config.api_key, "test_key");
        assert_eq!(config.model, "test_model");
        assert_eq!(config.base_url, "https://api.example.com");
    }

    #[test]
    fn test_error_handler() {
        let handler = ErrorHandler::new("test_provider");

        let error = handler.handle_http_error(reqwest::StatusCode::UNAUTHORIZED, "Invalid API key");

        match error {
            LLMError::Authentication(_) => {
                // Expected
            }
            _ => panic!("Expected authentication error"),
        }
    }

    #[test]
    fn test_model_resolver() {
        let resolver = ModelResolver::new("test_provider", "default-model", &["model1", "model2"]);

        assert_eq!(resolver.resolve_model(None), "default-model");
        assert_eq!(resolver.resolve_model(Some("custom".to_string())), "custom");

        assert!(resolver.validate_model("model1").is_ok());
        assert!(resolver.validate_model("unsupported").is_err());
    }
}
