//! A2A HTTP Server using axum
//!
//! Provides HTTP endpoints for the A2A Protocol, enabling VTCode to operate as an A2A agent.
//! The server exposes:
//! - Agent discovery via `/.well-known/agent-card.json`
//! - RPC endpoints at `/a2a` for message sending and task management
//! - Streaming endpoint at `/a2a/stream` for real-time updates via Server-Sent Events

#![cfg(feature = "a2a-server")]

use crate::a2a::agent_card::AgentCard;
use crate::a2a::errors::{A2aError, A2aErrorCode, A2aResult};
use crate::a2a::rpc::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, ListTasksParams, MessageSendParams,
    TaskIdParams, TaskQueryParams, JSONRPC_VERSION, METHOD_MESSAGE_SEND, METHOD_MESSAGE_STREAM,
    METHOD_TASKS_CANCEL, METHOD_TASKS_GET, METHOD_TASKS_LIST,
};
use crate::a2a::task_manager::TaskManager;
use crate::a2a::types::TaskState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
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
}

impl A2aServerState {
    /// Create a new server state
    pub fn new(task_manager: TaskManager, agent_card: AgentCard) -> Self {
        Self {
            task_manager: Arc::new(task_manager),
            agent_card: Arc::new(agent_card),
        }
    }

    /// Create a server state with default settings for VTCode
    pub fn vtcode_default(base_url: impl Into<String>) -> Self {
        Self::new(
            TaskManager::new(),
            AgentCard::vtcode_default(base_url),
        )
    }
}

// ============================================================================
// Router Creation
// ============================================================================

/// Create the A2A HTTP router
pub fn create_router(state: A2aServerState) -> Router {
    Router::new()
        .route("/.well-known/agent-card.json", axum::routing::get(get_agent_card))
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
        METHOD_MESSAGE_SEND => handle_message_send(&state, request.params, request.id.clone()).await,
        METHOD_MESSAGE_STREAM => {
            handle_message_stream(&state, request.params, request.id.clone()).await
        }
        METHOD_TASKS_GET => handle_tasks_get(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_LIST => handle_tasks_list(&state, request.params, request.id.clone()).await,
        METHOD_TASKS_CANCEL => {
            handle_tasks_cancel(&state, request.params, request.id.clone()).await
        }
        _ => {
            return Err(A2aErrorResponse::method_not_found(
                &request.method,
                request.id,
            ))
        }
    };

    match result {
        Ok(result_value) => Ok(Json(JsonRpcResponse::success(result_value, request.id))),
        Err(err) => Err(A2aErrorResponse::from_error(err, request.id)),
    }
}

/// Handle Server-Sent Events streaming
async fn handle_stream(
    State(_state): State<A2aServerState>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<impl IntoResponse, A2aErrorResponse> {
    // Validate request
    if request.jsonrpc != JSONRPC_VERSION {
        return Err(A2aErrorResponse::invalid_request(
            "Invalid JSON-RPC version",
            request.id.clone(),
        ));
    }

    if request.method != METHOD_MESSAGE_STREAM {
        return Err(A2aErrorResponse::method_not_found(
            &request.method,
            request.id,
        ));
    }

    // For now, return a simple response indicating streaming would be handled
    // Full implementation requires tokio::sync::broadcast and proper SSE setup
    let response = Json(JsonRpcResponse::success(
        json!({
            "message": "Streaming would be handled here",
            "status": "not-fully-implemented"
        }),
        request.id,
    ));
    Ok(response)
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
    let params: ListTasksParams = serde_json::from_value(params.unwrap_or_default())
        .unwrap_or_default();

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
        assert_eq!(
            err_response.status_code,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn test_error_response_invalid_request() {
        use serde_json::json;
        let err_response = A2aErrorResponse::invalid_request("Invalid JSON", json!(1));
        assert_eq!(err_response.status_code, StatusCode::BAD_REQUEST);
    }
}
