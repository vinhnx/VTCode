//! Webhook delivery for A2A push notifications
//!
//! Handles HTTP POST delivery of streaming events to configured webhook URLs
//! with retry logic, authentication, and error handling.

use super::rpc::{SendStreamingMessageResponse, StreamingEvent, TaskPushNotificationConfig};
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, error, warn};

/// Webhook notifier for delivering A2A events
#[derive(Debug, Clone)]
pub struct WebhookNotifier {
    client: Client,
    max_retries: u32,
    retry_delay_ms: u64,
}

impl Default for WebhookNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookNotifier {
    /// Create a new webhook notifier with default settings
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }

    /// Create a webhook notifier with custom settings
    pub fn with_settings(max_retries: u32, retry_delay_ms: u64) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            max_retries,
            retry_delay_ms,
        }
    }

    /// Deliver a streaming event to a webhook URL
    pub async fn send_event(
        &self,
        config: &TaskPushNotificationConfig,
        event: StreamingEvent,
    ) -> Result<(), WebhookError> {
        let response = SendStreamingMessageResponse { event };
        let json = serde_json::to_string(&response)
            .map_err(|e| WebhookError::Serialization(e.to_string()))?;

        self.send_with_retry(&config.url, &json, config.authentication.as_deref())
            .await
    }

    /// Send webhook with retry logic
    async fn send_with_retry(
        &self,
        url: &str,
        json: &str,
        auth: Option<&str>,
    ) -> Result<(), WebhookError> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let delay = self.retry_delay_ms * 2u64.pow(attempt - 1); // Exponential backoff
                debug!("Retrying webhook delivery after {}ms (attempt {})", delay, attempt);
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }

            match self.send_request(url, json, auth).await {
                Ok(()) => {
                    debug!("Webhook delivered successfully to {}", url);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Webhook delivery attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or(WebhookError::Unknown))
    }

    /// Send a single HTTP request
    async fn send_request(
        &self,
        url: &str,
        json: &str,
        auth: Option<&str>,
    ) -> Result<(), WebhookError> {
        let mut request = self
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", "VTCode-A2A/1.0");

        if let Some(auth_header) = auth {
            request = request.header("Authorization", auth_header);
        }

        let response = request
            .body(json.to_string())
            .send()
            .await
            .map_err(|e| WebhookError::Network(e.to_string()))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(WebhookError::HttpError(response.status().as_u16()))
        }
    }
}

/// Webhook delivery errors
#[derive(Debug, Clone)]
pub enum WebhookError {
    /// Network error
    Network(String),
    /// HTTP error status code
    HttpError(u16),
    /// JSON serialization error
    Serialization(String),
    /// Unknown error
    Unknown,
}

impl std::fmt::Display for WebhookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebhookError::Network(msg) => write!(f, "Network error: {}", msg),
            WebhookError::HttpError(code) => write!(f, "HTTP error: {}", code),
            WebhookError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            WebhookError::Unknown => write!(f, "Unknown error"),
        }
    }
}

impl std::error::Error for WebhookError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::a2a::types::{Message, TaskState, TaskStatus};

    #[test]
    fn test_webhook_notifier_creation() {
        let notifier = WebhookNotifier::new();
        assert_eq!(notifier.max_retries, 3);
        assert_eq!(notifier.retry_delay_ms, 1000);
    }

    #[test]
    fn test_webhook_notifier_with_settings() {
        let notifier = WebhookNotifier::with_settings(5, 2000);
        assert_eq!(notifier.max_retries, 5);
        assert_eq!(notifier.retry_delay_ms, 2000);
    }

    #[tokio::test]
    async fn test_webhook_error_display() {
        let err = WebhookError::Network("Connection refused".to_string());
        assert!(err.to_string().contains("Network error"));

        let err = WebhookError::HttpError(404);
        assert!(err.to_string().contains("404"));
    }

    #[tokio::test]
    async fn test_send_event_serialization() {
        let notifier = WebhookNotifier::new();
        let config = TaskPushNotificationConfig {
            task_id: "task-1".to_string(),
            url: "https://example.com/webhook".to_string(),
            authentication: None,
        };

        let event = StreamingEvent::TaskStatus {
            task_id: "task-1".to_string(),
            context_id: None,
            status: TaskStatus::new(TaskState::Completed),
            kind: "status-update".to_string(),
            r#final: true,
        };

        // This will fail with network error since the URL doesn't exist,
        // but we're testing that serialization works
        let result = notifier.send_event(&config, event).await;
        assert!(result.is_err());

        if let Err(e) = result {
            // Should be network error, not serialization error
            match e {
                WebhookError::Serialization(_) => panic!("Unexpected serialization error"),
                _ => {} // Expected network or HTTP error
            }
        }
    }
}
