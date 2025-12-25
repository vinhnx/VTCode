#![allow(clippy::result_large_err)]
//! Base trait and common implementations for LLM providers
//!
//! This module provides a unified foundation for all LLM providers to eliminate
//! code duplication across provider implementations.

use crate::llm::provider::{LLMError, LLMRequest, LLMResponse, Message, ToolDefinition};
use async_trait::async_trait;
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::{sleep, timeout};

const DEFAULT_MAX_INFLIGHT_PER_MODEL: usize = 4;
const RATE_LIMIT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

static MODEL_LIMITERS: LazyLock<Mutex<HashMap<String, Arc<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
    #[allow(clippy::result_large_err)]
    pub fn build_http_client(&self) -> Result<HttpClient, LLMError> {
        use crate::llm::http_client::HttpClientFactory;
        Ok(HttpClientFactory::with_timeouts(
            self.timeout,
            Duration::from_secs(30),
        ))
    }
}

/// Common HTTP error handling for all providers
pub fn handle_http_error(status: StatusCode, error_text: &str, _model: &str) -> LLMError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => LLMError::Authentication {
            message: format!("Authentication failed ({}): {}", status, error_text),
            metadata: None,
        },
        StatusCode::TOO_MANY_REQUESTS => LLMError::RateLimit { metadata: None },
        StatusCode::REQUEST_TIMEOUT => LLMError::Network {
            message: format!("Request timeout ({}): {}", status, error_text),
            metadata: None,
        },
        _ if status.is_server_error() => LLMError::Provider {
            message: format!("Server error ({}): {}", status, error_text),
            metadata: None,
        },
        _ => LLMError::Network {
            message: format!("HTTP error ({}): {}", status, error_text),
            metadata: None,
        },
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
        reasoning_effort: Option<String>,
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

        if let Some(val) = tools {
            body["tools"] = serde_json::json!(val);
        }

        if let Some(effort) = reasoning_effort {
            body["reasoning_effort"] = serde_json::json!(effort);
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
    #[allow(clippy::result_large_err)]
    fn build_request(&self, request: &LLMRequest) -> Result<reqwest::Request, LLMError>;

    /// Parse response from the provider
    #[allow(clippy::result_large_err)]
    fn parse_response(&self, response: Value) -> Result<LLMResponse, LLMError>;

    /// Execute LLM request with common error handling and retry logic
    async fn execute_request(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let _permit = acquire_model_permit(&self.config().model).await?;
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
                                    let error = LLMError::Network {
                                        message: format!("Failed to read response: {}", e),
                                        metadata: None,
                                    };
                                    if attempt < max_retries {
                                        last_error = Some(error);
                                        continue;
                                    }
                                    return Err(error);
                                }
                            }
                        }
                        Err(e) => {
                            let error = LLMError::Network {
                                message: format!("Request failed: {}", e),
                                metadata: None,
                            };
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
        Err(last_error.unwrap_or_else(|| LLMError::Network {
            message: "All retries exhausted".to_string(),
            metadata: None,
        }))
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

fn limiter_for_model(model: &str) -> Arc<Semaphore> {
    if let Ok(mut guard) = MODEL_LIMITERS.lock() {
        guard
            .entry(model.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(DEFAULT_MAX_INFLIGHT_PER_MODEL)))
            .clone()
    } else {
        Arc::new(Semaphore::new(DEFAULT_MAX_INFLIGHT_PER_MODEL))
    }
}

async fn acquire_model_permit(model: &str) -> Result<OwnedSemaphorePermit, LLMError> {
    let limiter = limiter_for_model(model);
    match timeout(RATE_LIMIT_ACQUIRE_TIMEOUT, limiter.acquire_owned()).await {
        Ok(Ok(permit)) => Ok(permit),
        Ok(Err(_)) => Err(LLMError::RateLimit { metadata: None }),
        Err(_) => Err(LLMError::RateLimit { metadata: None }),
    }
}
