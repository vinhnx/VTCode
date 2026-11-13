//! A2A HTTP Server using axum
//!
//! Provides HTTP endpoints for the A2A Protocol, enabling VT Code to operate as an A2A agent.
//! The server exposes:
//! - Agent discovery via `/.well-known/agent-card.json`
//! - RPC endpoints at `/a2a` for message sending and task management
//! - Streaming endpoint at `/a2a/stream` for real-time updates via Server-Sent Events

use crate::WebhookNotifier;
use crate::agent_card::AgentCard;
use crate::errors::{A2aError, A2aErrorCode, A2aResult};
use crate::rpc::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse, ListTasksParams, METHOD_MESSAGE_SEND,
    METHOD_MESSAGE_STREAM, METHOD_TASKS_CANCEL, METHOD_TASKS_GET, METHOD_TASKS_LIST, METHOD_TASKS_PUSH_CONFIG_GET,
    METHOD_TASKS_PUSH_CONFIG_SET, MessageSendParams, SendStreamingMessageResponse, StreamingEvent, TaskIdParams,
    TaskQueryParams,
};
use crate::task_manager::TaskManager;
use crate::types::TaskState;
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{
        HeaderMap, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_TYPE, WWW_AUTHENTICATE},
    },
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::post,
};
use serde_json::{Value, json};
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;
use uuid::Uuid;

/// Bearer token used to protect the A2A RPC and streaming endpoints.
#[derive(Clone)]
struct AuthToken(Arc<str>);

impl fmt::Debug for AuthToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthToken(REDACTED)")
    }
}

impl AuthToken {
    fn generated() -> Self {
        Self(Arc::from(Uuid::new_v4().to_string()))
    }

    fn from_value(value: String) -> anyhow::Result<Self> {
        if value.is_empty() || !value.is_ascii() || value.chars().any(char::is_whitespace) {
            anyhow::bail!("A2A authentication token must be non-empty ASCII text without whitespace");
        }

        Ok(Self(Arc::from(value)))
    }

    fn as_str(&self) -> &str {
        &self.0
    }

    fn matches(&self, presented: &str) -> bool {
        let expected = self.0.as_bytes();
        let presented = presented.as_bytes();
        // Branch-free OR-reduction over iterators (no per-byte bounds checks).
        // LLVM can auto-vectorize this shape; the indexed `.get(i)` form cannot
        // prove in-bounds and stays scalar. All bytes are still visited so the
        // comparison remains constant-time with no early exit.
        let common_len = expected.len().min(presented.len());
        let (expected_common, expected_tail) = expected.split_at(common_len);
        let (presented_common, presented_tail) = presented.split_at(common_len);
        let mut difference = expected.len() ^ presented.len();
        difference |= expected_common
            .iter()
            .zip(presented_common.iter())
            .fold(0_usize, |acc, (&a, &b)| acc | usize::from(a ^ b));
        difference |= expected_tail.iter().fold(0_usize, |acc, &b| acc | usize::from(b));
        difference |= presented_tail.iter().fold(0_usize, |acc, &b| acc | usize::from(b));

        difference == 0
    }
}

// ============================================================================
// Server State
// ============================================================================

/// A2A Server State containing shared resources
#[derive(Debug, Clone)]
pub struct A2aServerState {
    /// Task manager for handling task lifecycle
    task_manager: Arc<TaskManager>,
    /// Agent card for discovery
    agent_card: Arc<AgentCard>,
    /// Broadcast channel for streaming events
    event_tx: Arc<tokio::sync::broadcast::Sender<StreamingEvent>>,
    /// Webhook notifier for push notifications
    webhook_notifier: Arc<WebhookNotifier>,
    /// Bearer token required by the RPC and streaming endpoints
    auth_token: AuthToken,
}

impl A2aServerState {
    /// Create a new server state
    pub fn new(task_manager: TaskManager, agent_card: AgentCard) -> Self {
        Self::from_auth_token(task_manager, agent_card, AuthToken::generated())
    }

    /// Create a new server state with a caller-supplied bearer token.
    pub fn new_with_auth_token(
        task_manager: TaskManager,
        agent_card: AgentCard,
        auth_token: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let auth_token = AuthToken::from_value(auth_token.into())?;
        Ok(Self::from_auth_token(task_manager, agent_card, auth_token))
    }

    fn from_auth_token(task_manager: TaskManager, agent_card: AgentCard, auth_token: AuthToken) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            task_manager: Arc::new(task_manager),
            agent_card: Arc::new(agent_card.with_bearer_auth()),
            event_tx: Arc::new(event_tx),
            webhook_notifier: Arc::new(WebhookNotifier::new()),
            auth_token,
        }
    }

    /// Return the bearer token clients must use for protected endpoints.
    pub fn auth_token(&self) -> &str {
        self.auth_token.as_str()
    }

    /// Create a server state with default settings for VT Code
    fn vtcode_default(base_url: impl Into<String>) -> Self {
        Self::new(TaskManager::new(), AgentCard::vtcode_default(base_url))
    }
}

// ============================================================================
// Router Creation
// ============================================================================

/// Create the A2A HTTP router
pub fn create_router(state: A2aServerState) -> Router {
    let protected_routes = Router::new()
        .route("/a2a", post(handle_rpc))
        .route("/a2a/stream", post(handle_stream))
        .layer(middleware::from_fn_with_state(state.clone(), require_bearer_auth));

    Router::new()
        .route("/.well-known/agent-card.json", axum::routing::get(get_agent_card))
        .merge(protected_routes)
        .with_state(state)
        // Cross-origin access is disabled by default. A deployment that needs
        // browser clients must add an explicit origin allowlist at its edge.
        .layer(
            CorsLayer::new()
                .allow_methods([Method::POST])
                .allow_headers([AUTHORIZATION, CONTENT_TYPE]),
        )
}

async fn require_bearer_auth(State(state): State<A2aServerState>, request: Request, next: Next) -> Response {
    if has_valid_bearer_auth(&state, request.headers()) {
        next.run(request).await
    } else {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(JsonRpcResponse::error(JsonRpcError::new(-32600, "Authentication required"), Value::Null)),
        )
            .into_response();
        drop(
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer")),
        );
        response
    }
}

fn has_valid_bearer_auth(state: &A2aServerState, headers: &HeaderMap) -> bool {
    if headers.get_all(AUTHORIZATION).iter().count() != 1 {
        return false;
    }

    let Some(value) = headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()) else {
        return false;
    };

    let Some((scheme, token)) = value.split_once(' ') else {
        return false;
    };

    scheme.eq_ignore_ascii_case("Bearer")
        && !token.is_empty()
        && !token.chars().any(char::is_whitespace)
        && state.auth_token.matches(token)
}

// ============================================================================
// Handlers
// ============================================================================

/// Get agent card for discovery
async fn get_agent_card(State(state): State<A2aServerState>) -> Json<AgentCard> {
    Json(state.agent_card.as_ref().clone())
}

/// Handle JSON-RPC requests
async fn handle_rpc(
    State(state): State<A2aServerState>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, A2aErrorResponse> {
    // Validate request
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(A2aErrorResponse::invalid_request("Invalid JSON-RPC version", request.id));
    }

    // Dispatch to method handler
    let result = match request.method.as_str() {
        METHOD_MESSAGE_SEND => handle_message_send(&state, request.params, request.id.clone()).await,
        METHOD_MESSAGE_STREAM => handle_message_stream(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_GET => handle_tasks_get(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_LIST => handle_tasks_list(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_CANCEL => handle_tasks_cancel(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_PUSH_CONFIG_SET => handle_push_config_set(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_PUSH_CONFIG_GET => handle_push_config_get(&state, request.params, request.id.clone()).await,
        _ => {
            return Err(A2aErrorResponse::method_not_found(&request.method, request.id));
        }
    };

    match result {
        Ok(result_value) => Ok(Json(JsonRpcResponse::success(result_value, request.id))),
        Err(err) => Err(A2aErrorResponse::from_error(err, request.id)),
    }
}

/// Spawn a best-effort webhook delivery task.
///
/// Documented detached: the delivery is bounded (10s client timeout, max 3
/// retries with backoff in `WebhookNotifier`), self-terminating, and its
/// outcome is observable via `tracing::warn` on failure. Detachment is safe
/// because webhook delivery must never block the SSE stream or agent turn;
/// see "Task Extent, Error Propagation, and Cancel-Safety" in
/// `docs/guides/async-architecture.md`.
fn spawn_webhook_delivery(
    notifier: Arc<WebhookNotifier>,
    task_manager: Arc<TaskManager>,
    task_id: String,
    event: StreamingEvent,
) {
    drop(tokio::spawn(async move {
        let Some(cfg) = task_manager.get_webhook_config(&task_id).await else {
            return;
        };
        if let Err(error) = notifier.send_event(&cfg, event).await {
            tracing::warn!(task_id = %task_id, error = %error, "A2A webhook delivery failed");
        }
    }));
}

/// Handle Server-Sent Events streaming
async fn handle_stream(State(state): State<A2aServerState>, Json(request): Json<JsonRpcRequest>) -> impl IntoResponse {
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(A2aErrorResponse::invalid_request("Invalid JSON-RPC version", request.id.clone()));
    }

    if request.method != METHOD_MESSAGE_STREAM {
        return Err(A2aErrorResponse::method_not_found(&request.method, request.id.clone()));
    }

    // Parse params
    let params: MessageSendParams = serde_json::from_value(request.params.unwrap_or_default())
        .map_err(|_e| A2aErrorResponse::invalid_request("Invalid message/stream params", request.id.clone()))?;

    // Create or get task
    let task_id = if let Some(task_id) = params.task_id.clone() {
        task_id
    } else {
        let task = state.task_manager.create_task(params.context_id.clone()).await;
        task.id.clone()
    };

    // Add initial message
    drop(
        state
            .task_manager
            .add_message(&task_id, params.message.clone())
            .await
            .map_err(|e| A2aErrorResponse::from_error(e, request.id.clone()))?,
    );

    // Subscribe to broadcast channel
    let mut rx = state.event_tx.subscribe();
    let task_id_clone = task_id.clone();
    let context_id = params.context_id.clone();
    let notifier = state.webhook_notifier.clone();
    let task_manager = state.task_manager.clone();

    // Create stream from broadcast receiver using async_stream
    let stream = async_stream::stream! {
        while let Ok(event) = rx.recv().await {
            // Filter events for this task/context
            let matches = match &event {
                StreamingEvent::Message { context_id: ctx, .. } => {
                    context_id.as_ref() == ctx.as_ref()
                }
                StreamingEvent::TaskStatus { task_id: tid, .. } => tid == &task_id_clone,
                StreamingEvent::TaskArtifact { task_id: tid, .. } => tid == &task_id_clone,
                _ => false,
            };

            if matches {
                spawn_webhook_delivery(notifier.clone(), task_manager.clone(), task_id_clone.clone(), event.clone());

                let is_final = event.is_final();
                let json = serde_json::to_string(&SendStreamingMessageResponse { event })
                    .unwrap_or_default();
                yield Ok::<_, !>(Event::default().data(json));

                if is_final {
                    break;
                }
            }
        }
    };

    // Start background task to process and emit events.
    // Documented detached: bounded demo pipeline (~600ms of sleeps plus
    // channel sends), terminated by completion, with outcomes fanned out over
    // the broadcast channel; webhook legs use `spawn_webhook_delivery`.
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    drop(tokio::spawn(async move {
        // Simulate agent processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Update task to working
        drop(
            state_clone
                .task_manager
                .update_status(&task_id_clone, TaskState::Working, None)
                .await,
        );

        // Send status update event
        let status_event = StreamingEvent::TaskStatus {
            task_id: task_id_clone.clone(),
            context_id: params.context_id.clone(),
            status: crate::types::TaskStatus::new(TaskState::Working),
            kind: "status-update".to_string(),
            r#final: false,
        };
        drop(state_clone.event_tx.send(status_event.clone()));

        spawn_webhook_delivery(
            state_clone.webhook_notifier.clone(),
            state_clone.task_manager.clone(),
            task_id_clone.clone(),
            status_event,
        );

        // Simulate generating a response message
        tokio::time::sleep(Duration::from_millis(200)).await;
        let response_msg = crate::types::Message::agent_text("Processing your request...");
        let message_event = StreamingEvent::Message {
            message: response_msg,
            context_id: params.context_id.clone(),
            kind: "streaming-response".to_string(),
            r#final: false,
        };
        drop(state_clone.event_tx.send(message_event.clone()));

        spawn_webhook_delivery(
            state_clone.webhook_notifier.clone(),
            state_clone.task_manager.clone(),
            task_id_clone.clone(),
            message_event,
        );

        // Complete the task
        tokio::time::sleep(Duration::from_millis(300)).await;
        drop(
            state_clone
                .task_manager
                .update_status(&task_id_clone, TaskState::Completed, None)
                .await,
        );

        // Send final status event
        let final_status_event = StreamingEvent::TaskStatus {
            task_id: task_id_clone,
            context_id: params.context_id,
            status: crate::types::TaskStatus::new(TaskState::Completed),
            kind: "status-update".to_string(),
            r#final: true,
        };
        drop(state_clone.event_tx.send(final_status_event.clone()));

        spawn_webhook_delivery(
            state_clone.webhook_notifier.clone(),
            state_clone.task_manager.clone(),
            final_status_event.task_id().unwrap_or_default().to_string(),
            final_status_event,
        );
    }));

    Ok(Sse::new(Box::pin(stream)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}

// ============================================================================
// RPC Method Handlers
// ============================================================================

/// Handle message/send RPC method
async fn handle_message_send(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let params: MessageSendParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid message/send params"))?;

    // Create or get task
    let task_id = if let Some(task_id) = params.task_id {
        task_id
    } else {
        let task = state.task_manager.create_task(params.context_id).await;
        task.id.clone()
    };

    // Add message to history
    drop(state.task_manager.add_message(&task_id, params.message).await?);

    // Update status to working
    let task = state.task_manager.update_status(&task_id, TaskState::Working, None).await?;

    // Return task as response
    Ok(serde_json::to_value(task)?)
}

/// Handle tasks/pushNotificationConfig/set RPC method
async fn handle_push_config_set(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let config: crate::rpc::TaskPushNotificationConfig = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid pushNotificationConfig/set params"))?;

    state.task_manager.set_webhook_config(config).await?;

    Ok(json!({ "success": true }))
}

/// Handle tasks/pushNotificationConfig/get RPC method
async fn handle_push_config_get(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let params: TaskIdParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid pushNotificationConfig/get params"))?;

    let config = state.task_manager.get_webhook_config(&params.id).await;

    Ok(serde_json::to_value(config)?)
}

/// Handle message/stream RPC method
fn handle_message_stream<'a>(
    state: &'a A2aServerState,
    params: Option<Value>,
    id: Value,
) -> impl Future<Output = A2aResult<Value>> + 'a {
    // Same as message_send for now, but would support streaming
    handle_message_send(state, params, id)
}

/// Handle tasks/get RPC method
async fn handle_tasks_get(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let params: TaskQueryParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid tasks/get params"))?;

    let task = state
        .task_manager
        .get_task_or_error_with_history(&params.id, params.history_length.unwrap_or_default() as usize)
        .await?;

    Ok(serde_json::to_value(task)?)
}

/// Handle tasks/list RPC method
async fn handle_tasks_list(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let params = match params {
        Some(params) => serde_json::from_value(params)
            .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid tasks/list params"))?,
        None => ListTasksParams::default(),
    };

    let result = state.task_manager.list_tasks(params).await;

    Ok(serde_json::to_value(result)?)
}

/// Handle tasks/cancel RPC method
async fn handle_tasks_cancel(state: &A2aServerState, params: Option<Value>, _id: Value) -> A2aResult<Value> {
    let params: TaskIdParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_e| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid tasks/cancel params"))?;

    let task = state.task_manager.cancel_task(&params.id).await?;

    Ok(serde_json::to_value(task)?)
}

// ============================================================================
// Error Response Handler
// ============================================================================

/// A2A error response for Axum
pub struct A2aErrorResponse {
    response: Box<JsonRpcResponse>,
    status_code: StatusCode,
}

impl A2aErrorResponse {
    /// Create a new error response
    fn new(error: JsonRpcError, id: Value, status_code: StatusCode) -> Self {
        Self {
            response: Box::new(JsonRpcResponse::error(error, id)),
            status_code,
        }
    }

    /// Create an invalid request error response
    fn invalid_request(message: &str, id: Value) -> Self {
        Self::new(JsonRpcError::invalid_request(message), id, StatusCode::BAD_REQUEST)
    }

    /// Create a method not found error response
    fn method_not_found(method: &str, id: Value) -> Self {
        Self::new(JsonRpcError::method_not_found(method), id, StatusCode::NOT_FOUND)
    }

    /// Create an error response from an A2aError
    fn from_error(error: A2aError, id: Value) -> Self {
        let code: i32 = error.code().into();
        let message = error.to_string();
        let status_code = match &error {
            A2aError::RpcError { code, .. } => match code {
                A2aErrorCode::InvalidRequest | A2aErrorCode::InvalidParams | A2aErrorCode::JsonParseError => {
                    StatusCode::BAD_REQUEST
                }
                A2aErrorCode::MethodNotFound => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            },
            A2aError::TaskNotFound(_) => StatusCode::NOT_FOUND,
            A2aError::TaskNotCancelable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            A2aError::InvalidStateTransition { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        Self::new(JsonRpcError::new(code, message), id, status_code)
    }
}

impl IntoResponse for A2aErrorResponse {
    fn into_response(self) -> Response {
        (self.status_code, Json(*self.response)).into_response()
    }
}

// ============================================================================
// Server Startup
// ============================================================================

/// Run the A2A server
pub async fn run(state: A2aServerState, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("A2A server listening on {}", addr);
    axum::serve(listener, create_router(state))
        .with_graceful_shutdown(crate::shutdown_signal_logged("A2A"))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_state_creation() {
        let state = A2aServerState::vtcode_default("http://localhost:8080");
        assert_eq!(state.agent_card.name, "vtcode-agent");
    }

    #[test]
    fn test_error_response_task_not_found() {
        use serde_json::json;
        let err_response = A2aErrorResponse::from_error(A2aError::TaskNotFound("test-id".to_string()), json!(1));
        assert_eq!(err_response.status_code, StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_response_task_not_cancelable() {
        use serde_json::json;
        let err = A2aError::TaskNotCancelable("Cannot cancel completed task".to_string());
        let err_response = A2aErrorResponse::from_error(err, json!(1));
        assert_eq!(err_response.status_code, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[test]
    fn test_error_response_invalid_request() {
        use serde_json::json;
        let err_response = A2aErrorResponse::invalid_request("Invalid JSON", json!(1));
        assert_eq!(err_response.status_code, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_server_state_with_broadcast() {
        let state = A2aServerState::vtcode_default("http://localhost:8080");

        // Verify broadcast channel works
        let mut rx = state.event_tx.subscribe();

        // Send a test event
        let test_event = StreamingEvent::Message {
            message: super::super::types::Message::agent_text("Test"),
            context_id: Some("test".to_string()),
            kind: "streaming-response".to_string(),
            r#final: false,
        };

        let _ignored = state.event_tx.send(test_event.clone()).expect("send event");

        // Receive the event
        let received = rx.recv().await.expect("receive event");
        assert!(!received.is_final());
    }

    #[test]
    fn test_server_state_debug_redacts_auth_token() {
        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");

        let debug = format!("{state:?}");
        assert!(!debug.contains("test-token"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn test_server_state_rejects_invalid_auth_token() {
        let result = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            " ",
        );

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_agent_card_is_public_and_advertises_authentication() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/.well-known/agent-card.json")
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("receive response");

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let card: Value = serde_json::from_slice(&body).expect("parse card");
        assert_eq!(card["securitySchemes"]["bearerAuth"]["scheme"], "bearer");
        assert_eq!(card["security"][0]["bearerAuth"], json!([]));
    }

    #[tokio::test]
    async fn test_rpc_requires_bearer_auth_before_json_parsing() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let token = state.auth_token().to_string();
        let app = create_router(state);

        let unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .body(Body::from("not-json"))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(unauthorized.headers()[WWW_AUTHENTICATE], "Bearer");

        let wrong_token = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, "Bearer wrong-token")
                    .body(Body::from("{}"))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(wrong_token.status(), StatusCode::UNAUTHORIZED);

        let stream_unauthorized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a/stream")
                    .header("content-type", "application/json")
                    .body(Body::from("not-json"))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(stream_unauthorized.status(), StatusCode::UNAUTHORIZED);

        let authenticated = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(r#"{"jsonrpc":"2.0","method":"unknown","id":1}"#))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(authenticated.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_duplicate_authorization_headers_are_rejected() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let app = create_router(state);
        let mut request = Request::builder()
            .method("POST")
            .uri("/a2a")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .expect("build request");
        let _ = request
            .headers_mut()
            .append(AUTHORIZATION, HeaderValue::from_static("Bearer test-token"));
        let _ = request
            .headers_mut()
            .append(AUTHORIZATION, HeaderValue::from_static("Bearer test-token"));

        let response = app.oneshot(request).await.expect("receive response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_rpc_does_not_allow_cross_origin_cors_access() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let app = create_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("origin", "https://attacker.example")
                    .body(Body::from("{}"))
                    .expect("build request"),
            )
            .await
            .expect("receive response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn test_task_history_limits_are_applied_by_rpc_handlers() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let task_manager = TaskManager::new();
        let task = task_manager.create_task(None).await;
        drop(
            task_manager
                .add_message(&task.id, crate::types::Message::user_text("private message"))
                .await
                .expect("add message"),
        );
        let state = A2aServerState::new_with_auth_token(
            task_manager,
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let app = create_router(state);

        let request_body = |params| {
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "method": METHOD_TASKS_GET,
                "params": params,
                "id": 1,
            }))
            .expect("serialize request")
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, "Bearer test-token")
                    .body(Body::from(request_body(json!({"id": task.id}))))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let body: Value = serde_json::from_slice(&body).expect("parse response");
        assert!(body["result"].get("history").is_none());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, "Bearer test-token")
                    .body(Body::from(request_body(json!({"id": task.id, "historyLength": 1}))))
                    .expect("build request"),
            )
            .await
            .expect("receive response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let body: Value = serde_json::from_slice(&body).expect("parse response");
        assert_eq!(body["result"]["history"].as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn test_malformed_task_list_params_are_rejected() {
        use axum::{body::Body, http::Request};
        use tower::ServiceExt;

        let state = A2aServerState::new_with_auth_token(
            TaskManager::new(),
            AgentCard::vtcode_default("http://localhost:8080"),
            "test-token",
        )
        .expect("valid auth token");
        let app = create_router(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/a2a")
                    .header("content-type", "application/json")
                    .header(AUTHORIZATION, "Bearer test-token")
                    .body(Body::from(r#"{"jsonrpc":"2.0","method":"tasks/list","params":[],"id":1}"#))
                    .expect("build request"),
            )
            .await
            .expect("receive response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
