//! A2A HTTP Server using axum
//!
//! Provides HTTP endpoints for the A2A Protocol, enabling VT Code to operate as an A2A agent.
//! The server exposes:
//! - Agent discovery via `/.well-known/agent-card.json`
//! - RPC endpoints at `/a2a` for message sending and task management
//! - Streaming endpoint at `/a2a/stream` for real-time updates via Server-Sent Events

#![cfg(feature = "a2a-server")]

use crate::a2a::WebhookNotifier;
use crate::a2a::agent_card::AgentCard;
use crate::a2a::errors::{A2aError, A2aErrorCode, A2aResult};
use crate::a2a::rpc::{
    JSONRPC_VERSION, JsonRpcError, JsonRpcRequest, JsonRpcResponse, ListTasksParams,
    METHOD_MESSAGE_SEND, METHOD_MESSAGE_STREAM, METHOD_TASKS_CANCEL, METHOD_TASKS_GET,
    METHOD_TASKS_LIST, METHOD_TASKS_PUSH_CONFIG_GET, METHOD_TASKS_PUSH_CONFIG_SET,
    MessageSendParams, SendStreamingMessageResponse, StreamingEvent, TaskIdParams, TaskQueryParams,
};
use crate::a2a::task_manager::TaskManager;
use crate::a2a::types::TaskState;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::post,
};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

// ============================================================================
// Server State
// ============================================================================

/// A2A Server State containing shared resources
#[derive(Debug, Clone)]
pub struct A2aServerState {
    /// Task manager for handling task lifecycle
    pub task_manager: Arc<TaskManager>,
    /// Agent card for discovery
    pub agent_card: Arc<AgentCard>,
    /// Broadcast channel for streaming events
    pub event_tx: Arc<tokio::sync::broadcast::Sender<StreamingEvent>>,
    /// Webhook notifier for push notifications
    pub webhook_notifier: Arc<WebhookNotifier>,
}

impl A2aServerState {
    /// Create a new server state
    pub fn new(task_manager: TaskManager, agent_card: AgentCard) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            task_manager: Arc::new(task_manager),
            agent_card: Arc::new(agent_card),
            event_tx: Arc::new(event_tx),
            webhook_notifier: Arc::new(WebhookNotifier::new()),
        }
    }

    /// Create a server state with default settings for VT Code
    pub fn vtcode_default(base_url: impl Into<String>) -> Self {
        Self::new(TaskManager::new(), AgentCard::vtcode_default(base_url))
    }
}

// ============================================================================
// Router Creation
// ============================================================================

/// Create the A2A HTTP router
pub fn create_router(state: A2aServerState) -> Router {
    Router::new()
        .route(
            "/.well-known/agent-card.json",
            axum::routing::get(get_agent_card),
        )
        .route("/a2a", post(handle_rpc))
        .route("/a2a/stream", post(handle_stream))
        .with_state(state)
        .layer(CorsLayer::permissive())
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
        return Err(A2aErrorResponse::invalid_request(
            "Invalid JSON-RPC version",
            request.id,
        ));
    }

    // Dispatch to method handler
    let result = match request.method.as_str() {
        METHOD_MESSAGE_SEND => {
            handle_message_send(&state, request.params, request.id.clone()).await
        }
        METHOD_MESSAGE_STREAM => {
            handle_message_stream(&state, request.params, request.id.clone()).await
        }
        METHOD_TASKS_GET => handle_tasks_get(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_LIST => handle_tasks_list(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_CANCEL => {
            handle_tasks_cancel(&state, request.params, request.id.clone()).await
        }
        METHOD_TASKS_PUSH_CONFIG_SET => {
            handle_push_config_set(&state, request.params, request.id.clone()).await
        }
        METHOD_TASKS_PUSH_CONFIG_GET => {
            handle_push_config_get(&state, request.params, request.id.clone()).await
        }
        _ => {
            return Err(A2aErrorResponse::method_not_found(
                &request.method,
                request.id,
            ));
        }
    };

    match result {
        Ok(result_value) => Ok(Json(JsonRpcResponse::success(result_value, request.id))),
        Err(err) => Err(A2aErrorResponse::from_error(err, request.id)),
    }
}

/// Handle Server-Sent Events streaming
async fn handle_stream(
    State(state): State<A2aServerState>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(A2aErrorResponse::invalid_request(
            "Invalid JSON-RPC version",
            request.id.clone(),
        ));
    }

    if request.method != METHOD_MESSAGE_STREAM {
        return Err(A2aErrorResponse::method_not_found(
            &request.method,
            request.id.clone(),
        ));
    }

    // Parse params
    let params: MessageSendParams = serde_json::from_value(request.params.unwrap_or_default())
        .map_err(|_| {
            A2aErrorResponse::invalid_request("Invalid message/stream params", request.id.clone())
        })?;

    // Create or get task
    let task_id = if let Some(task_id) = params.task_id.clone() {
        task_id
    } else {
        let task = state
            .task_manager
            .create_task(params.context_id.clone())
            .await;
        task.id.clone()
    };

    // Add initial message
    state
        .task_manager
        .add_message(&task_id, params.message.clone())
        .await
        .map_err(|e| A2aErrorResponse::from_error(e, request.id.clone()))?;

    // Subscribe to broadcast channel
    let mut rx = state.event_tx.subscribe();
    let task_id_clone = task_id.clone();
    let context_id = params.context_id.clone();
    let notifier = state.webhook_notifier.clone();
    let task_manager = state.task_manager.clone();

    // Create stream from broadcast receiver using async_stream
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    // Filter events for this task/context
                    let matches = match &event {
                        StreamingEvent::Message { context_id: ctx, .. } => {
                            context_id.as_ref() == ctx.as_ref()
                        }
                        StreamingEvent::TaskStatus { task_id: tid, .. } => tid == &task_id_clone,
                        StreamingEvent::TaskArtifact { task_id: tid, .. } => tid == &task_id_clone,
                    };

                    if matches {
                        // Fire webhook asynchronously (best-effort)
                        let notifier = notifier.clone();
                        let task_manager = task_manager.clone();
                        let task_id_for_hook = task_id_clone.clone();
                        let event_for_hook = event.clone();
                        tokio::spawn(async move {
                            if let Some(cfg) = task_manager.get_webhook_config(&task_id_for_hook).await {
                                let _ = notifier.send_event(&cfg, event_for_hook).await;
                            }
                        });

                        let is_final = event.is_final();
                        let json = serde_json::to_string(&SendStreamingMessageResponse { event })
                            .unwrap_or_default();
                        yield Ok::<_, Infallible>(Event::default().data(json));

                        if is_final {
                            break;
                        }
                    }
                }
                Err(_) => break,
            }
        }
    };

    // Start background task to process and emit events
    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    tokio::spawn(async move {
        // Simulate agent processing
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Update task to working
        let _ = state_clone
            .task_manager
            .update_status(&task_id_clone, TaskState::Working, None)
            .await;

        // Send status update event
        let status_event = StreamingEvent::TaskStatus {
            task_id: task_id_clone.clone(),
            context_id: params.context_id.clone(),
            status: crate::a2a::types::TaskStatus::new(TaskState::Working),
            kind: "status-update".to_string(),
            r#final: false,
        };
        let _ = state_clone.event_tx.send(status_event.clone());

        // Fire webhook if configured
        let notifier = state_clone.webhook_notifier.clone();
        let task_manager = state_clone.task_manager.clone();
        let task_id_for_hook = task_id_clone.clone();
        tokio::spawn(async move {
            if let Some(cfg) = task_manager.get_webhook_config(&task_id_for_hook).await {
                let _ = notifier.send_event(&cfg, status_event).await;
            }
        });

        // Simulate generating a response message
        tokio::time::sleep(Duration::from_millis(200)).await;
        let response_msg = crate::a2a::types::Message::agent_text("Processing your request...");
        let message_event = StreamingEvent::Message {
            message: response_msg,
            context_id: params.context_id.clone(),
            kind: "streaming-response".to_string(),
            r#final: false,
        };
        let _ = state_clone.event_tx.send(message_event.clone());

        // Fire webhook if configured
        let notifier = state_clone.webhook_notifier.clone();
        let task_manager = state_clone.task_manager.clone();
        let task_id_for_hook = task_id_clone.clone();
        tokio::spawn(async move {
            if let Some(cfg) = task_manager.get_webhook_config(&task_id_for_hook).await {
                let _ = notifier.send_event(&cfg, message_event).await;
            }
        });

        // Complete the task
        tokio::time::sleep(Duration::from_millis(300)).await;
        let _ = state_clone
            .task_manager
            .update_status(&task_id_clone, TaskState::Completed, None)
            .await;

        // Send final status event
        let final_status_event = StreamingEvent::TaskStatus {
            task_id: task_id_clone,
            context_id: params.context_id,
            status: crate::a2a::types::TaskStatus::new(TaskState::Completed),
            kind: "status-update".to_string(),
            r#final: true,
        };
        let _ = state_clone.event_tx.send(final_status_event.clone());

        // Fire webhook if configured
        let notifier = state_clone.webhook_notifier.clone();
        let task_manager = state_clone.task_manager.clone();
        let task_id_for_hook = final_status_event.task_id().unwrap_or_default().to_string();
        tokio::spawn(async move {
            if let Some(cfg) = task_manager.get_webhook_config(&task_id_for_hook).await {
                let _ = notifier.send_event(&cfg, final_status_event).await;
            }
        });
    });

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
async fn handle_message_send(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let params: MessageSendParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid message/send params"))?;

    // Create or get task
    let task_id = if let Some(task_id) = params.task_id {
        task_id
    } else {
        let task = state.task_manager.create_task(params.context_id).await;
        task.id.clone()
    };

    // Add message to history
    state
        .task_manager
        .add_message(&task_id, params.message)
        .await?;

    // Update status to working
    let task = state
        .task_manager
        .update_status(&task_id, TaskState::Working, None)
        .await?;

    // Return task as response
    Ok(serde_json::to_value(task)?)
}

/// Handle tasks/pushNotificationConfig/set RPC method
async fn handle_push_config_set(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let config: crate::a2a::rpc::TaskPushNotificationConfig =
        serde_json::from_value(params.unwrap_or_default()).map_err(|_| {
            A2aError::rpc(
                A2aErrorCode::InvalidParams,
                "Invalid pushNotificationConfig/set params",
            )
        })?;

    state.task_manager.set_webhook_config(config).await?;

    Ok(json!({ "success": true }))
}

/// Handle tasks/pushNotificationConfig/get RPC method
async fn handle_push_config_get(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let params: TaskIdParams =
        serde_json::from_value(params.unwrap_or_default()).map_err(|_| {
            A2aError::rpc(
                A2aErrorCode::InvalidParams,
                "Invalid pushNotificationConfig/get params",
            )
        })?;

    let config = state.task_manager.get_webhook_config(&params.id).await;

    Ok(serde_json::to_value(config)?)
}

/// Handle message/stream RPC method
async fn handle_message_stream(
    state: &A2aServerState,
    params: Option<Value>,
    id: Value,
) -> A2aResult<Value> {
    // Same as message_send for now, but would support streaming
    handle_message_send(state, params, id).await
}

/// Handle tasks/get RPC method
async fn handle_tasks_get(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let params: TaskQueryParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid tasks/get params"))?;

    let task = state.task_manager.get_task_or_error(&params.id).await?;

    Ok(serde_json::to_value(task)?)
}

/// Handle tasks/list RPC method
async fn handle_tasks_list(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let params: ListTasksParams =
        serde_json::from_value(params.unwrap_or_default()).unwrap_or_default();

    let result = state.task_manager.list_tasks(params).await;

    Ok(serde_json::to_value(result)?)
}

/// Handle tasks/cancel RPC method
async fn handle_tasks_cancel(
    state: &A2aServerState,
    params: Option<Value>,
    _id: Value,
) -> A2aResult<Value> {
    let params: TaskIdParams = serde_json::from_value(params.unwrap_or_default())
        .map_err(|_| A2aError::rpc(A2aErrorCode::InvalidParams, "Invalid tasks/cancel params"))?;

    let task = state.task_manager.cancel_task(&params.id).await?;

    Ok(serde_json::to_value(task)?)
}

// ============================================================================
// Error Response Handler
// ============================================================================

/// A2A error response for Axum
pub struct A2aErrorResponse {
    response: JsonRpcResponse,
    status_code: StatusCode,
}

impl A2aErrorResponse {
    /// Create a new error response
    pub fn new(error: JsonRpcError, id: Value, status_code: StatusCode) -> Self {
        Self {
            response: JsonRpcResponse::error(error, id),
            status_code,
        }
    }

    /// Create an invalid request error response
    pub fn invalid_request(message: &str, id: Value) -> Self {
        Self::new(
            JsonRpcError::invalid_request(message),
            id,
            StatusCode::BAD_REQUEST,
        )
    }

    /// Create a method not found error response
    pub fn method_not_found(method: &str, id: Value) -> Self {
        Self::new(
            JsonRpcError::method_not_found(method),
            id,
            StatusCode::NOT_FOUND,
        )
    }

    /// Create an error response from an A2aError
    pub fn from_error(error: A2aError, id: Value) -> Self {
        let code: i32 = error.code().into();
        let message = error.to_string();
        let status_code = match error {
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
        (self.status_code, Json(self.response)).into_response()
    }
}

// ============================================================================
// Server Startup
// ============================================================================

/// Run the A2A server
pub async fn run(
    state: A2aServerState,
    addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("A2A server listening on {}", addr);
    axum::serve(listener, create_router(state)).await?;
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
        let err_response =
            A2aErrorResponse::from_error(A2aError::TaskNotFound("test-id".to_string()), json!(1));
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

        state.event_tx.send(test_event.clone()).expect("send event");

        // Receive the event
        let received = rx.recv().await.expect("receive event");
        assert!(!received.is_final());
    }
}
