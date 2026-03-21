//! HTTP-based ACP client for agent communication

use crate::discovery::AgentRegistry;
use crate::error::{AcpError, AcpResult};
use crate::messages::AcpMessage;
use reqwest::{Client as HttpClient, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, trace};

/// ACP Client for communicating with remote agents
pub struct AcpClient {
    /// HTTP client for requests
    http_client: HttpClient,

    /// Local agent identifier
    local_agent_id: String,

    /// Agent discovery registry
    registry: AgentRegistry,

    /// Request timeout
    #[allow(dead_code)]
    timeout: Duration,
}

/// Builder for ACP client
pub struct AcpClientBuilder {
    local_agent_id: String,
    timeout: Duration,
}

impl AcpClientBuilder {
    /// Create a new builder
    pub fn new(local_agent_id: String) -> Self {
        Self {
            local_agent_id,
            timeout: Duration::from_secs(30),
        }
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the client
    pub fn build(self) -> AcpResult<AcpClient> {
        let http_client = HttpClient::builder().timeout(self.timeout).build()?;

        Ok(AcpClient {
            http_client,
            local_agent_id: self.local_agent_id,
            registry: AgentRegistry::new(),
            timeout: self.timeout,
        })
    }
}

impl AcpClient {
    /// Create a new ACP client with default settings
    pub fn new(local_agent_id: String) -> AcpResult<Self> {
        AcpClientBuilder::new(local_agent_id).build()
    }

    /// Get the agent registry
    pub fn registry(&self) -> &AgentRegistry {
        &self.registry
    }

    /// Send a request to a remote agent synchronously
    pub async fn call_sync(
        &self,
        remote_agent_id: &str,
        action: String,
        args: Value,
    ) -> AcpResult<Value> {
        debug!(
            remote_agent = remote_agent_id,
            action = %action,
            "Sending synchronous request to remote agent"
        );

        let agent_info = self
            .registry
            .find(remote_agent_id)
            .await
            .map_err(|_| AcpError::AgentNotFound(remote_agent_id.to_string()))?;

        let message = AcpMessage::request(
            self.local_agent_id.clone(),
            remote_agent_id.to_string(),
            action,
            args,
        );

        let response = self.send_request(&agent_info.base_url, &message).await?;

        trace!(
            remote_agent = remote_agent_id,
            "Response received from remote agent"
        );

        Ok(response)
    }

    /// Send a request to a remote agent asynchronously
    pub async fn call_async(
        &self,
        remote_agent_id: &str,
        action: String,
        args: Value,
    ) -> AcpResult<String> {
        debug!(
            remote_agent = remote_agent_id,
            action = %action,
            "Sending asynchronous request to remote agent"
        );

        let agent_info = self
            .registry
            .find(remote_agent_id)
            .await
            .map_err(|_| AcpError::AgentNotFound(remote_agent_id.to_string()))?;

        let mut message = AcpMessage::request(
            self.local_agent_id.clone(),
            remote_agent_id.to_string(),
            action,
            args,
        );

        // Set async flag in request
        if let crate::messages::MessageContent::Request(ref mut req) = message.content {
            req.sync = false;
        }

        // Async calls may not wait for response
        let _ = self.send_request(&agent_info.base_url, &message).await;

        trace!(
            remote_agent = remote_agent_id,
            message_id = %message.id,
            "Asynchronous request sent"
        );

        Ok(message.id)
    }

    /// Send raw ACP message and get response
    async fn send_request(&self, base_url: &str, message: &AcpMessage) -> AcpResult<Value> {
        let url = format!("{}/messages", base_url.trim_end_matches('/'));

        trace!(url = %url, message_id = %message.id, "Sending ACP message");

        let response = self.http_client.post(&url).json(message).send().await?;

        let status = response.status();

        match status {
            StatusCode::OK | StatusCode::ACCEPTED => {
                let body = response.text().await?;
                trace!(
                    status = %status,
                    body_len = body.len(),
                    "Received ACP response"
                );

                if body.is_empty() {
                    return Ok(Value::Null);
                }

                serde_json::from_str(&body).map_err(|e| {
                    AcpError::SerializationError(format!(
                        "Failed to parse response: {}: {}",
                        e, body
                    ))
                })
            }

            StatusCode::REQUEST_TIMEOUT => Err(AcpError::Timeout(
                "Request to remote agent timed out".to_string(),
            )),

            StatusCode::NOT_FOUND => Err(AcpError::AgentNotFound(
                "Remote agent endpoint not found".to_string(),
            )),

            status => {
                let body = response.text().await.unwrap_or_default();
                Err(AcpError::RemoteError {
                    agent_id: base_url.to_string(),
                    message: format!("HTTP {}: {}", status.as_u16(), body),
                    code: Some(status.as_u16() as i32),
                })
            }
        }
    }

    /// Discover agent metadata from base URL (offline discovery)
    pub async fn discover_agent(&self, base_url: &str) -> AcpResult<crate::discovery::AgentInfo> {
        let url = format!("{}/metadata", base_url.trim_end_matches('/'));

        trace!(url = %url, "Discovering agent metadata");

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| AcpError::NetworkError(format!("Discovery failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AcpError::NetworkError(format!(
                "Discovery failed with status {}",
                response.status()
            )));
        }

        let agent_info = response.json().await?;

        trace!("Agent metadata discovered successfully");

        Ok(agent_info)
    }

    /// Check if a remote agent is reachable
    pub async fn ping(&self, remote_agent_id: &str) -> AcpResult<bool> {
        let agent_info = self
            .registry
            .find(remote_agent_id)
            .await
            .map_err(|_| AcpError::AgentNotFound(remote_agent_id.to_string()))?;

        let url = format!("{}/health", agent_info.base_url.trim_end_matches('/'));

        match self.http_client.get(&url).send().await {
            Ok(response) => {
                let is_healthy = response.status().is_success();
                if is_healthy {
                    self.registry
                        .update_status(remote_agent_id, true)
                        .await
                        .ok();
                }
                Ok(is_healthy)
            }
            Err(_) => {
                self.registry
                    .update_status(remote_agent_id, false)
                    .await
                    .ok();
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = AcpClient::new("test-agent".to_string()).unwrap();
        assert_eq!(client.local_agent_id, "test-agent");
    }

    #[tokio::test]
    async fn test_client_builder() {
        let client = AcpClientBuilder::new("test-agent".to_string())
            .with_timeout(Duration::from_secs(60))
            .build()
            .unwrap();

        assert_eq!(client.local_agent_id, "test-agent");
        assert_eq!(client.timeout, Duration::from_secs(60));
    }
}
