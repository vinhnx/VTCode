//! ACP Client V2 with full protocol compliance
//!
//! This module implements the ACP client with:
//! - JSON-RPC 2.0 transport
//! - Full session lifecycle (initialize, session/new, session/prompt)
//! - SSE streaming for session updates
//! - Capability negotiation
//!
//! Reference: https://agentclientprotocol.com/llms.txt

use crate::capabilities::{
    AgentCapabilities, AuthenticateParams, AuthenticateResult, ClientCapabilities, ClientInfo,
    InitializeParams, InitializeResult, SUPPORTED_VERSIONS,
};
use crate::error::{AcpError, AcpResult};
use crate::jsonrpc::{JSONRPC_VERSION, JsonRpcId, JsonRpcRequest, JsonRpcResponse};
use crate::session::{
    AcpSession, SessionCancelParams, SessionLoadParams, SessionLoadResult, SessionNewParams,
    SessionNewResult, SessionPromptParams, SessionPromptResult, SessionState,
    SessionUpdateNotification,
};

use reqwest::{Client as HttpClient, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, trace, warn};

/// ACP Client V2 with full protocol compliance
///
/// This client implements the complete ACP session lifecycle:
/// 1. `initialize()` - Negotiate protocol version and capabilities
/// 2. `authenticate()` - Optional authentication (if required by agent)
/// 3. `session_new()` - Create a new conversation session
/// 4. `session_prompt()` - Send prompts and receive responses
/// 5. `session_cancel()` - Cancel ongoing operations
///
/// Streaming updates are delivered via SSE and can be subscribed to
/// using `subscribe_updates()`.
pub struct AcpClientV2 {
    /// HTTP client for JSON-RPC requests
    http_client: HttpClient,

    /// Base URL of the ACP agent
    base_url: String,

    /// Local client identifier
    #[allow(dead_code)]
    client_id: String,

    /// Client capabilities
    capabilities: ClientCapabilities,

    /// Request timeout
    #[allow(dead_code)]
    timeout: Duration,

    /// Request ID counter for correlation
    request_counter: AtomicU64,

    /// Negotiated protocol version (set after initialize)
    protocol_version: RwLock<Option<String>>,

    /// Agent capabilities (set after initialize)
    agent_capabilities: RwLock<Option<AgentCapabilities>>,

    /// Active sessions
    sessions: RwLock<HashMap<String, AcpSession>>,

    /// Authentication token (if authenticated)
    auth_token: RwLock<Option<String>>,
}

/// Builder for AcpClientV2
pub struct AcpClientV2Builder {
    base_url: String,
    client_id: String,
    capabilities: ClientCapabilities,
    timeout: Duration,
}

impl AcpClientV2Builder {
    /// Create a new builder
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client_id: format!("vtcode-{}", uuid::Uuid::new_v4()),
            capabilities: ClientCapabilities::default(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Set client identifier
    pub fn with_client_id(mut self, id: impl Into<String>) -> Self {
        self.client_id = id.into();
        self
    }

    /// Set client capabilities
    pub fn with_capabilities(mut self, caps: ClientCapabilities) -> Self {
        self.capabilities = caps;
        self
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build the client
    pub fn build(self) -> AcpResult<AcpClientV2> {
        let http_client = HttpClient::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| AcpError::ConfigError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(AcpClientV2 {
            http_client,
            base_url: self.base_url,
            client_id: self.client_id,
            capabilities: self.capabilities,
            timeout: self.timeout,
            request_counter: AtomicU64::new(1),
            protocol_version: RwLock::new(None),
            agent_capabilities: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
            auth_token: RwLock::new(None),
        })
    }
}

impl AcpClientV2 {
    /// Create a new client with default settings
    pub fn new(base_url: impl Into<String>) -> AcpResult<Self> {
        AcpClientV2Builder::new(base_url).build()
    }

    /// Get the next request ID
    fn next_request_id(&self) -> JsonRpcId {
        let id = self.request_counter.fetch_add(1, Ordering::SeqCst);
        JsonRpcId::Number(id as i64)
    }

    /// Check if client has been initialized
    pub async fn is_initialized(&self) -> bool {
        self.protocol_version.read().await.is_some()
    }

    /// Get negotiated protocol version
    pub async fn protocol_version(&self) -> Option<String> {
        self.protocol_version.read().await.clone()
    }

    /// Get agent capabilities (after initialization)
    pub async fn agent_capabilities(&self) -> Option<AgentCapabilities> {
        self.agent_capabilities.read().await.clone()
    }

    // ========================================================================
    // JSON-RPC Transport Layer
    // ========================================================================

    /// Send a JSON-RPC request and parse the response
    async fn call<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: Option<P>,
    ) -> AcpResult<R> {
        let id = self.next_request_id();
        let params_value = params
            .map(|p| serde_json::to_value(p))
            .transpose()
            .map_err(|e| AcpError::SerializationError(e.to_string()))?;

        let request = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.to_string(),
            params: params_value,
            id: Some(id.clone()),
        };

        debug!(method = method, id = %id, "Sending JSON-RPC request");

        let url = format!("{}/rpc", self.base_url.trim_end_matches('/'));

        let mut req_builder = self.http_client.post(&url).json(&request);

        // Add auth token if present
        if let Some(token) = self.auth_token.read().await.as_ref() {
            req_builder = req_builder.bearer_auth(token);
        }

        let response = req_builder
            .send()
            .await
            .map_err(|e| AcpError::NetworkError(format!("Request failed: {}", e)))?;

        let status = response.status();

        match status {
            StatusCode::OK => {
                let body = response
                    .text()
                    .await
                    .map_err(|e| AcpError::NetworkError(e.to_string()))?;

                trace!(body_len = body.len(), "Received JSON-RPC response");

                let rpc_response: JsonRpcResponse = serde_json::from_str(&body).map_err(|e| {
                    AcpError::SerializationError(format!("Invalid response: {}", e))
                })?;

                if let Some(error) = rpc_response.error {
                    return Err(AcpError::RemoteError {
                        agent_id: self.base_url.clone(),
                        message: error.message,
                        code: Some(error.code),
                    });
                }

                let result = rpc_response.result.unwrap_or(Value::Null);
                serde_json::from_value(result)
                    .map_err(|e| AcpError::SerializationError(format!("Result parse error: {}", e)))
            }

            StatusCode::UNAUTHORIZED => Err(AcpError::RemoteError {
                agent_id: self.base_url.clone(),
                message: "Authentication required".to_string(),
                code: Some(401),
            }),

            StatusCode::REQUEST_TIMEOUT => Err(AcpError::Timeout("Request timed out".to_string())),

            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(AcpError::RemoteError {
                    agent_id: self.base_url.clone(),
                    message: format!("HTTP {}: {}", status.as_u16(), body),
                    code: Some(status.as_u16() as i32),
                })
            }
        }
    }

    /// Send a notification (no response expected)
    async fn notify<P: Serialize>(&self, method: &str, params: Option<P>) -> AcpResult<()> {
        let params_value = params
            .map(|p| serde_json::to_value(p))
            .transpose()
            .map_err(|e| AcpError::SerializationError(e.to_string()))?;

        let request = JsonRpcRequest::notification(method, params_value);

        debug!(method = method, "Sending JSON-RPC notification");

        let url = format!("{}/rpc", self.base_url.trim_end_matches('/'));

        let mut req_builder = self.http_client.post(&url).json(&request);

        if let Some(token) = self.auth_token.read().await.as_ref() {
            req_builder = req_builder.bearer_auth(token);
        }

        // Fire and forget
        let _ = req_builder.send().await;

        Ok(())
    }

    // ========================================================================
    // ACP Protocol Methods
    // ========================================================================

    /// Initialize the connection with the agent
    ///
    /// This must be called before any other method. It negotiates:
    /// - Protocol version
    /// - Client and agent capabilities
    /// - Authentication requirements
    pub async fn initialize(&self) -> AcpResult<InitializeResult> {
        let params = InitializeParams {
            protocol_versions: SUPPORTED_VERSIONS.iter().map(|s| s.to_string()).collect(),
            capabilities: self.capabilities.clone(),
            client_info: ClientInfo::default(),
        };

        let result: InitializeResult = self.call("initialize", Some(params)).await?;

        // Validate negotiated protocol version is one we support
        if !SUPPORTED_VERSIONS.contains(&result.protocol_version.as_str()) {
            return Err(AcpError::InvalidRequest(format!(
                "Agent negotiated unsupported protocol version: {}",
                result.protocol_version
            )));
        }

        // Store negotiated state
        *self.protocol_version.write().await = Some(result.protocol_version.clone());
        *self.agent_capabilities.write().await = Some(result.capabilities.clone());

        debug!(
            protocol = %result.protocol_version,
            agent = %result.agent_info.name,
            "ACP connection initialized"
        );

        Ok(result)
    }

    /// Authenticate with the agent (if required)
    pub async fn authenticate(&self, params: AuthenticateParams) -> AcpResult<AuthenticateResult> {
        let result: AuthenticateResult = self.call("authenticate", Some(params)).await?;

        if result.authenticated {
            if let Some(token) = &result.session_token {
                *self.auth_token.write().await = Some(token.clone());
            }
            debug!("Authentication successful");
        } else {
            warn!("Authentication failed");
        }

        Ok(result)
    }

    /// Create a new session
    pub async fn session_new(&self, params: SessionNewParams) -> AcpResult<SessionNewResult> {
        if !self.is_initialized().await {
            return Err(AcpError::InvalidRequest(
                "Client not initialized. Call initialize() first.".to_string(),
            ));
        }

        let result: SessionNewResult = self.call("session/new", Some(params)).await?;

        // Track session locally
        let session = AcpSession::new(&result.session_id);
        self.sessions
            .write()
            .await
            .insert(result.session_id.clone(), session);

        debug!(session_id = %result.session_id, "Session created");

        Ok(result)
    }

    /// Load an existing session
    pub async fn session_load(&self, session_id: &str) -> AcpResult<SessionLoadResult> {
        if !self.is_initialized().await {
            return Err(AcpError::InvalidRequest(
                "Client not initialized. Call initialize() first.".to_string(),
            ));
        }

        let params = SessionLoadParams {
            session_id: session_id.to_string(),
        };

        let result: SessionLoadResult = self.call("session/load", Some(params)).await?;

        // Track session locally
        self.sessions
            .write()
            .await
            .insert(session_id.to_string(), result.session.clone());

        debug!(
            session_id = session_id,
            turns = result.history.len(),
            "Session loaded"
        );

        Ok(result)
    }

    /// Send a prompt to the session
    ///
    /// Returns the turn result. For streaming responses, use `subscribe_updates()`
    /// before calling this method.
    pub async fn session_prompt(
        &self,
        params: SessionPromptParams,
    ) -> AcpResult<SessionPromptResult> {
        self.session_prompt_with_timeout(params, None).await
    }

    /// Send a prompt with a custom timeout
    pub async fn session_prompt_with_timeout(
        &self,
        params: SessionPromptParams,
        timeout: Option<Duration>,
    ) -> AcpResult<SessionPromptResult> {
        if !self.is_initialized().await {
            return Err(AcpError::InvalidRequest(
                "Client not initialized. Call initialize() first.".to_string(),
            ));
        }

        let session_id = params.session_id.clone();

        // Update session state
        if let Some(session) = self.sessions.write().await.get_mut(&session_id) {
            session.set_state(SessionState::Active);
            session.increment_turn();
        }

        // Use custom timeout if provided
        let result: SessionPromptResult = if let Some(custom_timeout) = timeout {
            tokio::time::timeout(
                custom_timeout,
                self.call::<_, SessionPromptResult>("session/prompt", Some(params)),
            )
            .await
            .map_err(|_| AcpError::Timeout("Prompt request timed out".to_string()))??
        } else {
            self.call("session/prompt", Some(params)).await?
        };

        // Update session state based on result
        if let Some(session) = self.sessions.write().await.get_mut(&session_id) {
            match result.status {
                crate::session::TurnStatus::Completed => {
                    session.set_state(SessionState::AwaitingInput);
                }
                crate::session::TurnStatus::Cancelled => {
                    session.set_state(SessionState::Cancelled);
                }
                crate::session::TurnStatus::Failed => {
                    session.set_state(SessionState::Failed);
                }
                crate::session::TurnStatus::AwaitingInput => {
                    session.set_state(SessionState::AwaitingInput);
                }
            }
        }

        debug!(
            session_id = %session_id,
            turn_id = %result.turn_id,
            status = ?result.status,
            "Prompt completed"
        );

        Ok(result)
    }

    /// Cancel an ongoing operation
    pub async fn session_cancel(&self, session_id: &str, turn_id: Option<&str>) -> AcpResult<()> {
        let params = SessionCancelParams {
            session_id: session_id.to_string(),
            turn_id: turn_id.map(String::from),
        };

        self.notify("session/cancel", Some(params)).await?;

        // Update local session state
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.set_state(SessionState::Cancelled);
        }

        debug!(session_id = session_id, "Session cancelled");

        Ok(())
    }

    /// Get a session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<AcpSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// List all active sessions
    pub async fn list_sessions(&self) -> Vec<AcpSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    // ========================================================================
    // SSE Streaming
    // ========================================================================

    /// Subscribe to session updates via Server-Sent Events
    ///
    /// Returns a receiver channel that will receive update notifications.
    /// The connection will remain open until the receiver is dropped.
    pub async fn subscribe_updates(
        &self,
        session_id: &str,
    ) -> AcpResult<mpsc::Receiver<SessionUpdateNotification>> {
        let (tx, rx) = mpsc::channel(100);

        let url = format!(
            "{}/sse/session/{}",
            self.base_url.trim_end_matches('/'),
            session_id
        );

        let _http_client = self.http_client.clone();
        let auth_token = self.auth_token.read().await.clone();

        // Spawn SSE listener task
        tokio::spawn(async move {
            if let Err(e) = Self::sse_listener(url, auth_token, tx).await {
                warn!("SSE listener error: {}", e);
            }
        });

        Ok(rx)
    }

    /// Internal SSE listener implementation
    async fn sse_listener(
        url: String,
        auth_token: Option<String>,
        tx: mpsc::Sender<SessionUpdateNotification>,
    ) -> AcpResult<()> {
        let client = HttpClient::new();

        let mut req = client.get(&url);
        if let Some(token) = auth_token {
            req = req.bearer_auth(token);
        }

        let response = req
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| AcpError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AcpError::NetworkError(format!(
                "SSE connection failed: {}",
                response.status()
            )));
        }

        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;

        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| AcpError::NetworkError(e.to_string()))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete events
            while let Some(event_end) = buffer.find("\n\n") {
                let event = buffer[..event_end].to_string();
                buffer = buffer[event_end + 2..].to_string();

                // Parse SSE event fields
                let mut event_type = None;
                let mut data_lines = Vec::new();

                for line in event.lines() {
                    if let Some(data) = line.strip_prefix("data:") {
                        data_lines.push(data.trim());
                    } else if let Some(evt) = line.strip_prefix("event:") {
                        event_type = Some(evt.trim());
                    }
                    // Ignore: id:, retry:, and comment lines
                }

                // Process session/update events
                if (event_type.is_none() || event_type == Some("session/update"))
                    && !data_lines.is_empty()
                {
                    let data = data_lines.join("\n");
                    if let Ok(notification) =
                        serde_json::from_str::<SessionUpdateNotification>(&data)
                        && tx.send(notification).await.is_err()
                    {
                        // Receiver dropped, exit
                        return Ok(());
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_builder() {
        let client = AcpClientV2Builder::new("http://localhost:8080")
            .with_client_id("test-client")
            .with_timeout(Duration::from_secs(60))
            .build()
            .unwrap();

        assert_eq!(client.base_url, "http://localhost:8080");
        assert_eq!(client.client_id, "test-client");
        assert_eq!(client.timeout, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn test_client_not_initialized() {
        let client = AcpClientV2::new("http://localhost:8080").unwrap();
        assert!(!client.is_initialized().await);
    }

    #[test]
    fn test_request_id_generation() {
        let client = AcpClientV2::new("http://localhost:8080").unwrap();

        let id1 = client.next_request_id();
        let id2 = client.next_request_id();

        assert_ne!(id1, id2);
    }
}
