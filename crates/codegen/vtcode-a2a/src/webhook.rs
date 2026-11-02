//! Webhook delivery for A2A push notifications
//!
//! Handles HTTP POST delivery of streaming events to configured webhook URLs
//! with retry logic, authentication, and error handling.

use super::rpc::{SendStreamingMessageResponse, StreamingEvent, TaskPushNotificationConfig};
use reqwest::Client;
use std::time::Duration;
use tracing::{debug, warn};
use url::{Host, Url};

/// Parse and validate a webhook destination before it is stored or requested.
///
/// HTTPS is required for remote destinations. Plain HTTP is limited to the
/// exact localhost name or an IP loopback address, never a hostname suffix.
pub(crate) fn parse_webhook_url(raw_url: &str) -> Result<Url, String> {
    let url = Url::parse(raw_url).map_err(|error| format!("Webhook URL must be a valid absolute URL: {error}"))?;

    if url.username() != "" || url.password().is_some() {
        return Err("Webhook URL must not contain credentials".to_string());
    }
    if url.fragment().is_some() {
        return Err("Webhook URL must not contain a fragment".to_string());
    }

    let is_loopback = matches!(
        url.host(),
        Some(Host::Domain(host)) if host.eq_ignore_ascii_case("localhost")
    ) || matches!(url.host(), Some(Host::Ipv4(address)) if address.is_loopback())
        || matches!(url.host(), Some(Host::Ipv6(address)) if address.is_loopback());

    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback => Ok(url),
        "http" => Err("Webhook URL must use HTTPS unless it targets localhost".to_string()),
        _ => Err("Webhook URL must use HTTPS or HTTP localhost".to_string()),
    }
}

/// Webhook notifier for delivering A2A events
#[derive(Debug, Clone)]
pub struct WebhookNotifier {
    client: Option<Client>,
    max_retries: u32,
    retry_delay_ms: u64,
}

impl Default for WebhookNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl WebhookNotifier {
    fn build_http_client() -> Option<Client> {
        match Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => Some(client),
            Err(error) => {
                warn!(error = %error, "Failed to configure webhook HTTP client; webhook delivery disabled");
                None
            }
        }
    }

    /// Create a new webhook notifier with default settings
    pub(crate) fn new() -> Self {
        Self {
            client: Self::build_http_client(),
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }

    /// Create a webhook notifier with custom settings
    fn with_settings(max_retries: u32, retry_delay_ms: u64) -> Self {
        Self {
            client: Self::build_http_client(),
            max_retries,
            retry_delay_ms,
        }
    }

    /// Deliver a streaming event to a webhook URL
    pub(crate) async fn send_event(
        &self,
        config: &TaskPushNotificationConfig,
        event: StreamingEvent,
    ) -> Result<(), WebhookError> {
        let response = SendStreamingMessageResponse { event };
        let json = serde_json::to_string(&response).map_err(|e| WebhookError::Serialization(e.to_string()))?;
        let url = parse_webhook_url(&config.url).map_err(WebhookError::InvalidUrl)?;

        self.send_with_retry(&url, &json, config.authentication.as_deref()).await
    }

    /// Retry delay before `attempt` (1-based) with unbounded exponential growth
    /// guarded against overflow.
    fn retry_delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1);
        Duration::from_millis(self.retry_delay_ms.saturating_mul(2_u64.saturating_pow(exponent)))
    }

    /// Send webhook with retry logic
    async fn send_with_retry(&self, url: &Url, json: &str, auth: Option<&str>) -> Result<(), WebhookError> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let delay = self.retry_delay(attempt); // Exponential backoff
                debug!("Retrying webhook delivery after {:?} (attempt {})", delay, attempt);
                tokio::time::sleep(delay).await;
            }

            match self.send_request(url, json, auth).await {
                Ok(()) => {
                    debug!("Webhook delivered successfully");
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
    async fn send_request(&self, url: &Url, json: &str, auth: Option<&str>) -> Result<(), WebhookError> {
        let Some(client) = self.client.as_ref() else {
            return Err(WebhookError::ClientUnavailable);
        };

        let mut request = client
            .post(url.clone())
            .header("Content-Type", "application/json")
            .header("User-Agent", "VT Code-A2A/1.0");

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
#[derive(Debug, Clone, thiserror::Error)]
pub enum WebhookError {
    /// Network error
    #[error("Network error: {0}")]
    Network(String),
    /// HTTP error status code
    #[error("HTTP error: {0}")]
    HttpError(u16),
    /// JSON serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),
    /// Webhook URL failed validation
    #[error("Invalid webhook URL: {0}")]
    InvalidUrl(String),
    /// HTTP client could not be configured safely
    #[error("Webhook HTTP client is unavailable")]
    ClientUnavailable,
    /// Unknown error
    #[error("Unknown error")]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{TaskState, TaskStatus};

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

    #[test]
    fn test_webhook_retry_delay_doubles_and_saturates() {
        let notifier = WebhookNotifier::with_settings(3, 1_000);
        assert_eq!(notifier.retry_delay(1), Duration::from_millis(1_000));
        assert_eq!(notifier.retry_delay(2), Duration::from_millis(2_000));
        assert_eq!(notifier.retry_delay(3), Duration::from_millis(4_000));
        assert_eq!(
            notifier.retry_delay(u32::MAX),
            Duration::from_millis(u64::MAX),
            "an overflowing exponent must saturate instead of panicking"
        );
    }

    #[test]
    fn test_parse_webhook_url_requires_safe_scheme_and_host() {
        for url in [
            "https://example.com/webhook",
            "http://localhost/webhook",
            "http://127.0.0.1:8080/webhook",
            "http://[::1]:8080/webhook",
        ] {
            assert!(parse_webhook_url(url).is_ok(), "URL should be accepted: {url}");
        }

        for url in [
            "http://localhost.evil.example/webhook",
            "http://localhost@evil.example/webhook",
            "http://example.com/webhook",
            "ftp://example.com/webhook",
            "https://user:password@example.com/webhook",
            "https://example.com/webhook#fragment",
        ] {
            assert!(parse_webhook_url(url).is_err(), "URL should be rejected: {url}");
        }
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

        if let Err(WebhookError::Serialization(_)) = result {
            panic!("Unexpected serialization error");
        }
    }

    #[tokio::test]
    async fn test_send_event_rejects_invalid_url_before_network_access() {
        let notifier = WebhookNotifier::new();
        let config = TaskPushNotificationConfig {
            task_id: "task-1".to_string(),
            url: "http://localhost.evil.example/webhook".to_string(),
            authentication: None,
        };
        let event = StreamingEvent::Unknown;

        let result = notifier.send_event(&config, event).await;
        assert!(matches!(result, Err(WebhookError::InvalidUrl(_))));
    }

    #[tokio::test]
    async fn test_webhook_client_does_not_follow_redirects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test listener");
        let address = listener.local_addr().expect("read test listener address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept initial webhook");
            let mut request = [0; 1024];
            let _ignored = socket.read(&mut request).await.expect("read initial webhook");
            let redirect = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://{address}/redirect-target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(redirect.as_bytes()).await.expect("send redirect");

            match tokio::time::timeout(Duration::from_millis(250), listener.accept()).await {
                Ok(Ok((mut redirected_socket, _))) => {
                    let mut request = [0; 1024];
                    let _ignored = redirected_socket.read(&mut request).await.expect("read redirected webhook");
                    let response = b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    redirected_socket.write_all(response).await.expect("send redirected response");
                    true
                }
                Ok(Err(_)) | Err(_) => false,
            }
        });

        let notifier = WebhookNotifier::with_settings(0, 0);
        let config = TaskPushNotificationConfig {
            task_id: "task-1".to_string(),
            url: format!("http://{address}/hook"),
            authentication: None,
        };
        let result = notifier.send_event(&config, StreamingEvent::Unknown).await;
        assert!(matches!(result, Err(WebhookError::HttpError(302))));
        assert!(!server.await.expect("join test server"));
    }
}
