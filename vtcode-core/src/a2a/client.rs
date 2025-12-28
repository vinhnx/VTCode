//! A2A client for interacting with remote A2A agents.
//! Provides helper methods for discovery, task operations, and streaming.

use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

use anyhow::Context;
use futures::{Stream, StreamExt};
use reqwest::Client;
use serde_json::Value;

use crate::a2a::agent_card::AgentCard;
use crate::a2a::errors::{A2aError, A2aErrorCode, A2aResult};
use crate::a2a::rpc::{
    JsonRpcRequest, ListTasksParams, MessageSendParams, SendStreamingMessageResponse,
    StreamingEvent, TaskIdParams, TaskPushNotificationConfig, TaskQueryParams,
    JSONRPC_VERSION, METHOD_MESSAGE_SEND, METHOD_MESSAGE_STREAM, METHOD_TASKS_CANCEL,
    METHOD_TASKS_GET, METHOD_TASKS_LIST, METHOD_TASKS_PUSH_CONFIG_GET, METHOD_TASKS_PUSH_CONFIG_SET,
};
use crate::a2a::types::Task;

/// HTTP client for interacting with A2A agents
#[derive(Clone, Debug)]
pub struct A2aClient {
    base_url: String,
    http: Client,
    request_id: Arc<AtomicU64>,
}

impl A2aClient {
    /// Create a new client with default reqwest settings
    pub fn new(base_url: impl Into<String>) -> A2aResult<Self> {
        let http = Client::builder()
            .build()
            .context("Failed to build HTTP client")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        Ok(Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http,
            request_id: Arc::new(AtomicU64::new(1)),
        })
    }

    fn next_id(&self) -> String {
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        format!("a2a-{}", id)
    }

    fn rpc_url(&self) -> String {
        format!("{}/a2a", self.base_url)
    }

    fn stream_url(&self) -> String {
        format!("{}/a2a/stream", self.base_url)
    }

    fn agent_card_url(&self) -> String {
        format!("{}/.well-known/agent-card.json", self.base_url)
    }

    /// Fetch the remote agent card
    pub async fn agent_card(&self) -> A2aResult<AgentCard> {
        let resp = self
            .http
            .get(self.agent_card_url())
            .send()
            .await
            .context("Failed to fetch agent card")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(A2aError::rpc(
                A2aErrorCode::InvalidAgentResponse,
                format!("Agent card request failed with status {status}"),
            ));
        }

        let card = resp
            .json::<AgentCard>()
            .await
            .context("Invalid agent card response")
            .map_err(|e| A2aError::Internal(e.to_string()))?;
        Ok(card)
    }

    /// Send a message/send RPC
    pub async fn send_message(&self, params: MessageSendParams) -> A2aResult<Task> {
        let result_value = self
            .call_rpc(METHOD_MESSAGE_SEND, Some(serde_json::to_value(&params)?))
            .await?;
        let task: Task = serde_json::from_value(result_value)
            .context("Failed to deserialize task")
            .map_err(|e| A2aError::Internal(e.to_string()))?;
        Ok(task)
    }

    /// Send a message/stream RPC and consume streaming events
    pub async fn stream_message(
        &self,
        params: MessageSendParams,
    ) -> A2aResult<impl Stream<Item = A2aResult<StreamingEvent>>> {
        let req = JsonRpcRequest::with_string_id(
            METHOD_MESSAGE_STREAM,
            Some(serde_json::to_value(&params)?),
            self.next_id(),
        );

        let response = self
            .http
            .post(self.stream_url())
            .header("accept", "text/event-stream")
            .json(&req)
            .send()
            .await
            .context("Failed to open streaming request")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(A2aError::rpc(
                A2aErrorCode::InvalidAgentResponse,
                format!("Streaming request failed with status {status}"),
            ));
        }

        let byte_stream = response.bytes_stream();

        let stream = async_stream::try_stream! {
            let mut buffer = Vec::new();
            futures::pin_mut!(byte_stream);

            while let Some(chunk) = byte_stream.next().await {
                let chunk = chunk.context("Failed to read streaming chunk")
                    .map_err(|e| A2aError::Internal(e.to_string()))?;
                buffer.extend_from_slice(&chunk);

                // Process complete SSE events separated by double newlines
                loop {
                    if let Some(pos) = find_double_newline(&buffer) {
                        let event_bytes = buffer.drain(..pos + 2).collect::<Vec<u8>>();
                        if let Some(event) = parse_sse_event(&event_bytes)? {
                            yield event;
                        }
                    } else {
                        break;
                    }
                }
            }

            // Process any remaining buffered event
            if !buffer.is_empty() {
                if let Some(event) = parse_sse_event(&buffer)? {
                    yield event;
                }
            }
        };

        Ok(stream)
    }

    /// Get a task by ID
    pub async fn get_task(&self, task_id: String) -> A2aResult<Task> {
        let params = serde_json::to_value(TaskQueryParams { id: task_id, history_length: None })?;
        let result_value = self.call_rpc(METHOD_TASKS_GET, Some(params)).await?;
        let task: Task = serde_json::from_value(result_value)
            .context("Failed to deserialize task")
            .map_err(|e| A2aError::Internal(e.to_string()))?;
        Ok(task)
    }

    /// List tasks with filters
    pub async fn list_tasks(&self, params: Option<ListTasksParams>) -> A2aResult<Value> {
        let result_value = self
            .call_rpc(
                METHOD_TASKS_LIST,
                params.map(|p| serde_json::to_value(p)).transpose()?,
            )
            .await?;
        Ok(result_value)
    }

    /// Cancel a task
    pub async fn cancel_task(&self, task_id: String) -> A2aResult<Task> {
        let params = serde_json::to_value(TaskIdParams { id: task_id })?;
        let result_value = self
            .call_rpc(METHOD_TASKS_CANCEL, Some(params))
            .await?;
        let task: Task = serde_json::from_value(result_value)
            .context("Failed to deserialize task")
            .map_err(|e| A2aError::Internal(e.to_string()))?;
        Ok(task)
    }

    /// Set push notification config
    pub async fn set_push_config(&self, config: TaskPushNotificationConfig) -> A2aResult<bool> {
        let value = self
            .call_rpc(METHOD_TASKS_PUSH_CONFIG_SET, Some(serde_json::to_value(config)?))
            .await?;
        // Server returns {"success": true}
        let success = value
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        Ok(success)
    }

    /// Get push notification config
    pub async fn get_push_config(&self, task_id: String) -> A2aResult<Option<TaskPushNotificationConfig>> {
        let params = serde_json::to_value(TaskIdParams { id: task_id })?;
        let value = self
            .call_rpc(METHOD_TASKS_PUSH_CONFIG_GET, Some(params))
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        let cfg: TaskPushNotificationConfig = serde_json::from_value(value)
            .context("Failed to deserialize push notification config")
            .map_err(|e| A2aError::Internal(e.to_string()))?;
        Ok(Some(cfg))
    }

    async fn call_rpc(&self, method: &str, params: Option<Value>) -> A2aResult<Value> {
        let request = JsonRpcRequest::with_string_id(method, params, self.next_id());

        let resp = self
            .http
            .post(self.rpc_url())
            .json(&request)
            .send()
            .await
            .context("RPC request failed")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        let status = resp.status();
        let json: Value = resp
            .json()
            .await
            .context("Failed to parse RPC response")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        if !status.is_success() {
            return Err(A2aError::rpc(
                A2aErrorCode::InvalidAgentResponse,
                format!("RPC failed with status {status}: {json:?}"),
            ));
        }

        // Deserialize JSON-RPC envelope
        let rpc_response: crate::a2a::rpc::JsonRpcResponse = serde_json::from_value(json.clone())
            .context("Invalid JSON-RPC response")
            .map_err(|e| A2aError::Internal(e.to_string()))?;

        if let Some(result) = rpc_response.result {
            Ok(result)
        } else if let Some(err) = rpc_response.error {
            Err(A2aError::rpc(err.code.into(), err.message))
        } else {
            Err(A2aError::rpc(
                A2aErrorCode::InvalidAgentResponse,
                "Empty RPC response",
            ))
        }
    }
}

/// Find the position of the first double newline delimiter ("\n\n")
fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

/// Parse a single SSE event from raw bytes
fn parse_sse_event(bytes: &[u8]) -> A2aResult<Option<StreamingEvent>> {
    // SSE events are lines starting with "data: " and separated by blank line
    let text = std::str::from_utf8(bytes)
        .context("Invalid UTF-8 in SSE event")
        .map_err(|e| A2aError::Internal(e.to_string()))?;

    for line in text.lines() {
        if let Some(payload) = line.strip_prefix("data: ") {
            // Parse the streaming response wrapper
            let wrapper: SendStreamingMessageResponse = serde_json::from_str(payload)
                .context("Failed to deserialize streaming event")
                .map_err(|e| A2aError::Internal(e.to_string()))?;
            return Ok(Some(wrapper.event));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_double_newline() {
        let data = b"data: x\n\nrest";
        assert_eq!(find_double_newline(data), Some(7));
    }

    #[test]
    fn test_parse_sse_event_empty() {
        let res = parse_sse_event(b"event: ping\n\n").unwrap();
        assert!(res.is_none());
    }
}
