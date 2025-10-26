pub mod config;
pub mod retry;

pub use config::ClientConfig;
pub use retry::RetryConfig;

use crate::gemini::models::{GenerateContentRequest, GenerateContentResponse};
use crate::gemini::streaming::{
    StreamingError, StreamingMetrics, StreamingProcessor, StreamingResponse,
};
use anyhow::{Context, Result};
use reqwest::Client as ReqwestClient;
use std::time::Instant;

#[derive(Clone)]
pub struct Client {
    api_key: String,
    model: String,
    http: ReqwestClient,
    config: ClientConfig,
    retry_config: RetryConfig,
    metrics: StreamingMetrics,
}

impl Client {
    pub fn new(api_key: String, model: String) -> Self {
        Self::with_config(api_key, model, ClientConfig::default())
    }

    /// Create a client with custom configuration
    pub fn with_config(api_key: String, model: String, config: ClientConfig) -> Self {
        let http_client = ReqwestClient::builder()
            .pool_max_idle_per_host(config.pool_max_idle_per_host)
            .pool_idle_timeout(config.pool_idle_timeout)
            .tcp_keepalive(config.tcp_keepalive)
            .timeout(config.request_timeout)
            .connect_timeout(config.connect_timeout)
            .user_agent(&config.user_agent)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            api_key,
            model,
            http: http_client,
            config,
            retry_config: RetryConfig::default(),
            metrics: StreamingMetrics::default(),
        }
    }

    /// Get current client configuration
    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    /// Set retry configuration
    pub fn with_retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Get current retry configuration
    pub fn retry_config(&self) -> &RetryConfig {
        &self.retry_config
    }

    /// Get streaming metrics
    pub fn metrics(&self) -> &StreamingMetrics {
        &self.metrics
    }

    /// Reset streaming metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = StreamingMetrics::default();
    }

    /// Classify error to determine if it's retryable
    fn classify_error(&self, error: &anyhow::Error) -> StreamingError {
        let error_str = error.to_string().to_lowercase();

        if error_str.contains("timeout")
            || error_str.contains("connection")
            || error_str.contains("network")
        {
            StreamingError::NetworkError {
                message: error.to_string(),
                is_retryable: true,
            }
        } else if error_str.contains("rate limit") || error_str.contains("429") {
            StreamingError::ApiError {
                status_code: 429,
                message: "Rate limit exceeded".to_string(),
                is_retryable: true,
            }
        } else {
            StreamingError::NetworkError {
                message: error.to_string(),
                is_retryable: false,
            }
        }
    }

    /// Generate content with the Gemini API
    pub async fn generate(
        &mut self,
        request: &GenerateContentRequest,
    ) -> Result<GenerateContentResponse> {
        let start_time = Instant::now();

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            self.model
        );

        let response = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(request)
            .send()
            .await
            .context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("API error {}: {}", status, error_text));
        }

        let response_data: GenerateContentResponse =
            response.json().await.context("Failed to parse response")?;

        self.metrics.total_requests += 1;
        self.metrics.total_response_time += start_time.elapsed();

        Ok(response_data)
    }

    /// Generate content with the Gemini API using streaming
    pub async fn generate_stream<F>(
        &mut self,
        request: &GenerateContentRequest,
        on_chunk: F,
    ) -> Result<StreamingResponse, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let start_time = Instant::now();

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent",
            self.model
            .header("x-api-key", &self.api_key)
        );

        let response = self
            .http
            .post(&url)
            .json(request)
            .send()
            .await
            .map_err(|e| {
                let error = anyhow::Error::new(e);
                self.classify_error(&error)
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();

            let is_retryable = match status.as_u16() {
                429 | 500 | 502 | 503 | 504 => true,
                _ => false,
            };

            return Err(StreamingError::ApiError {
                status_code: status.as_u16(),
                message: error_text,
                is_retryable,
            });
        }

        // Process the streaming response
        let mut processor = StreamingProcessor::new();
        let result = processor.process_stream(response, on_chunk).await;

        self.metrics.total_requests += 1;
        self.metrics.total_response_time += start_time.elapsed();

        result
    }
}
