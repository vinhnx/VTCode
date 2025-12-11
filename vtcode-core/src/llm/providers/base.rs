//! Base trait and common implementations for LLM providers
//!
//! This module provides a unified foundation for all LLM providers to eliminate
//! code duplication across provider implementations.

use crate::llm::provider::{LLMError, LLMRequest, LLMResponse, Message, ToolDefinition};
use async_trait::async_trait;
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;

/// Base configuration shared by all providers
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    pub max_retries: u32,
}

impl ProviderConfig {
    /// Create provider config with sensible defaults
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            api_key,
            base_url,
            model,
            timeout: Duration::from_secs(120),
            max_retries: 3,
        }
    }

    /// Build HTTP client with provider-specific configuration
    pub fn build_http_client(&self) -> Result<HttpClient, LLMError> {
        HttpClient::builder()
            .timeout(self.timeout)
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| LLMError::Network(format!("Failed to build HTTP client: {}", e)))
    }
}

/// Common HTTP error handling for all providers
pub fn handle_http_error(status: StatusCode, error_text: &str, _model: &str) -> LLMError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => LLMError::Authentication(format!(
            "Authentication failed ({}): {}",
            status, error_text
        )),
        StatusCode::TOO_MANY_REQUESTS => LLMError::RateLimit,
        StatusCode::REQUEST_TIMEOUT => {
            LLMError::Network(format!("Request timeout ({}): {}", status, error_text))
        }
        _ if status.is_server_error() => {
            LLMError::Provider(format!("Server error ({}): {}", status, error_text))
        }
        _ => LLMError::Network(format!("HTTP error ({}): {}", status, error_text)),
    }
}

/// Check if error indicates model not found (common across providers)
pub fn is_model_not_found(status: StatusCode, error_text: &str) -> bool {
    status == StatusCode::NOT_FOUND
        || error_text.contains("model_not_found")
        || (error_text.to_ascii_lowercase().contains("does not exist")
            && error_text.to_ascii_lowercase().contains("model"))
}

/// Common request building utilities
pub mod request_builder {
    use super::*;

    /// Build standard headers for API requests
    pub fn build_headers(
        api_key: &str,
        provider_headers: Option<Vec<(&str, &str)>>,
    ) -> reqwest::header::HeaderMap {
        use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // Default authorization header (can be overridden by providers)
        if let Ok(auth_value) = HeaderValue::from_str(&format!("Bearer {}", api_key)) {
            headers.insert(AUTHORIZATION, auth_value);
        }

        // Add provider-specific headers
        if let Some(custom_headers) = provider_headers {
            for (key, value) in custom_headers {
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(key.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    headers.insert(name, val);
                }
            }
        }

        headers
    }

    /// Convert tools to OpenAI-compatible format (used by many providers)
    pub fn serialize_tools_openai(tools: &[ToolDefinition]) -> Option<Vec<Value>> {
        if tools.is_empty() {
            return None;
        }
        Some(tools.iter().map(|tool| serde_json::json!(tool)).collect())
    }

    /// Build standard request body structure
    pub fn build_request_body(
        messages: &[Message],
        model: &str,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
        tools: Option<Vec<Value>>,
        stream: bool,
    ) -> Value {
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages.iter().map(|msg| serde_json::json!({
                "role": msg.role.to_string().to_lowercase(),
                "content": msg.content,
            })).collect::<Vec<_>>(),
        });

        if let Some(max_tokens_val) = max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens_val);
        }

        if let Some(temp) = temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if let Some(tools_val) = tools {
            body["tools"] = serde_json::json!(tools_val);
        }

        if stream {
            body["stream"] = serde_json::json!(true);
        }

        body
    }
}

/// Base provider trait with common functionality
#[async_trait]
pub trait BaseProvider: Send + Sync {
    /// Get provider configuration
    fn config(&self) -> &ProviderConfig;

    /// Build HTTP request for the provider
    fn build_request(&self, request: &LLMRequest) -> Result<reqwest::Request, LLMError>;

    /// Parse response from the provider
    fn parse_response(&self, response: Value) -> Result<LLMResponse, LLMError>;

    /// Execute LLM request with common error handling and retry logic
    async fn execute_request(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let client = self.config().build_http_client()?;
        let max_retries = self.config().max_retries;

        let mut last_error = None;

        for attempt in 0..=max_retries {
            match self.build_request(&request) {
                Ok(http_request) => {
                    match client.execute(http_request).await {
                        Ok(response) => {
                            let status = response.status();

                            match response.text().await {
                                Ok(text) => {
                                    // Try to parse as JSON first
                                    match serde_json::from_str::<Value>(&text) {
                                        Ok(json_value) => {
                                            // Check for provider-specific error format
                                            if let Some(error_obj) = json_value.get("error") {
                                                let error_text = error_obj.to_string();
                                                if attempt < max_retries
                                                    && should_retry_status(status)
                                                {
                                                    sleep(backoff_duration(attempt)).await;
                                                    last_error = Some(handle_http_error(
                                                        status,
                                                        &error_text,
                                                        &self.config().model,
                                                    ));
                                                    continue;
                                                }
                                                return Err(handle_http_error(
                                                    status,
                                                    &error_text,
                                                    &self.config().model,
                                                ));
                                            }

                                            // Success - parse response
                                            return self.parse_response(json_value);
                                        }
                                        Err(_) => {
                                            // Not JSON - treat as error text
                                            if attempt < max_retries && should_retry_status(status)
                                            {
                                                sleep(backoff_duration(attempt)).await;
                                                last_error = Some(handle_http_error(
                                                    status,
                                                    &text,
                                                    &self.config().model,
                                                ));
                                                continue;
                                            }
                                            return Err(handle_http_error(
                                                status,
                                                &text,
                                                &self.config().model,
                                            ));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error = LLMError::Network(format!(
                                        "Failed to read response: {}",
                                        e
                                    ));
                                    if attempt < max_retries {
                                        last_error = Some(error);
                                        continue;
                                    }
                                    return Err(error);
                                }
                            }
                        }
                        Err(e) => {
                            let error = LLMError::Network(format!("Request failed: {}", e));
                            if attempt < max_retries {
                                sleep(backoff_duration(attempt)).await;
                                last_error = Some(error);
                                continue;
                            }
                            return Err(error);
                        }
                    }
                }
                Err(e) => {
                    if attempt < max_retries {
                        last_error = Some(e);
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        // All retries exhausted
        Err(last_error.unwrap_or_else(|| LLMError::Network("All retries exhausted".to_string())))
    }
}

/// Determine if a status code should trigger a retry
fn should_retry_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::REQUEST_TIMEOUT
            | StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

/// Exponential backoff with an upper bound to reduce provider hammering
fn backoff_duration(attempt: u32) -> Duration {
    let capped_attempt = attempt.min(5);
    const BASE_MS: u64 = 200;
    let backoff_ms = BASE_MS.saturating_mul(2_u64.saturating_pow(capped_attempt));
    Duration::from_millis(backoff_ms.min(5_000))
}
