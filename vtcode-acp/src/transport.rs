//! Generic JSON-RPC-over-stdio transport for subprocess agents.
//!
//! [`StdioTransport`] handles the low-level framing of newline-delimited JSON
//! over a child process's stdin/stdout pair. It is intentionally protocol-agnostic:
//! it knows nothing about Copilot, ACP sessions, or any other higher-level concept.
//!
//! ## Message routing
//!
//! The internal reader task inspects each incoming line and dispatches it as follows:
//!
//! - **Response** (has `result` or `error` field with a numeric `id`): looked up in the
//!   pending table populated by [`StdioTransport::call`] and delivered to the waiting
//!   caller via a [`tokio::sync::oneshot`] channel.
//! - **Request / notification** (anything else): forwarded to the closure registered
//!   via [`StdioTransport::set_notification_handler`].
//!
//! Stderr lines are forwarded to `tracing::debug!` under the
//! `vtcode.stdio_transport.stderr` target.

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::error::{AcpError, AcpResult};

/// Callback type for incoming server→client requests and notifications.
///
/// The handler receives the raw JSON-RPC message value. It should return
/// `Ok(())` on success; errors are logged as warnings by the transport.
type NotificationHandler = Arc<dyn Fn(Value) -> anyhow::Result<()> + Send + Sync>;

/// Generic JSON-RPC-over-stdio transport for local subprocess agents.
///
/// Wraps a child process and provides:
/// - [`call`](Self::call): send a request and await its response.
/// - [`notify`](Self::notify): send a fire-and-forget notification.
/// - [`respond`](Self::respond) / [`respond_error`](Self::respond_error): reply to
///   incoming server-initiated requests.
/// - [`set_notification_handler`](Self::set_notification_handler): register the handler
///   that receives all incoming server→client messages.
///
/// The child process is killed when this struct is dropped.
pub struct StdioTransport {
    write_tx: mpsc::UnboundedSender<String>,
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResult<Value>>>>>,
    request_counter: AtomicI64,
    notification_handler: Arc<StdMutex<Option<NotificationHandler>>>,
    child: StdMutex<Option<Child>>,
    rpc_timeout: Duration,
}

impl StdioTransport {
    /// Wire up transport from a spawned subprocess's stdin/stdout/stderr.
    ///
    /// Spawns background tasks for the writer (stdin), stderr logger, and the
    /// reader (stdout) that dispatches JSON-RPC messages.
    pub fn from_child(
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
        rpc_timeout: Duration,
    ) -> Self {
        let (write_tx, write_rx) = mpsc::unbounded_channel();
        let pending = Arc::new(StdMutex::new(HashMap::new()));
        let notification_handler = Arc::new(StdMutex::new(None));

        spawn_writer(write_rx, stdin);
        spawn_stderr_logger(stderr);
        spawn_reader(
            stdout,
            Arc::clone(&pending),
            Arc::clone(&notification_handler),
        );

        Self {
            write_tx,
            pending,
            request_counter: AtomicI64::new(1),
            notification_handler,
            child: StdMutex::new(Some(child)),
            rpc_timeout,
        }
    }

    /// Construct a transport with a pre-wired channel for unit tests.
    ///
    /// No subprocess is spawned and no background tasks are started. The caller
    /// can drive the mock by reading from the paired receiver.
    #[cfg(test)]
    pub fn new_for_testing(write_tx: mpsc::UnboundedSender<String>, rpc_timeout: Duration) -> Self {
        Self {
            write_tx,
            pending: Arc::new(StdMutex::new(HashMap::new())),
            request_counter: AtomicI64::new(1),
            notification_handler: Arc::new(StdMutex::new(None)),
            child: StdMutex::new(None),
            rpc_timeout,
        }
    }

    /// Register a handler for incoming server→client requests and notifications.
    ///
    /// Must be called once after construction. Subsequent calls overwrite the
    /// previous handler. The handler receives the raw JSON message value for
    /// every incoming message that is **not** a response to a pending [`call`](Self::call).
    pub fn set_notification_handler(&self, handler: NotificationHandler) {
        if let Ok(mut guard) = self.notification_handler.lock() {
            *guard = Some(handler);
        }
    }

    /// Send a JSON-RPC request and wait for its response.
    ///
    /// Assigns a monotonically increasing `id`, inserts it into the pending
    /// table, serialises the message, and awaits the reply up to `rpc_timeout`.
    ///
    /// # Errors
    ///
    /// Returns [`AcpError::Timeout`] if the peer does not reply in time, or
    /// [`AcpError::Internal`] if the transport is shut down.
    pub async fn call(&self, method: &str, params: Value) -> AcpResult<Value> {
        let id = self.request_counter.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| AcpError::Internal("stdio transport pending mutex poisoned".into()))?
            .insert(id, tx);

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        if let Err(e) = self.send_raw(payload) {
            // Clean up the pending entry so it doesn't linger until timeout.
            self.pending.lock().ok().map(|mut g| g.remove(&id));
            return Err(e);
        }

        timeout(self.rpc_timeout, rx)
            .await
            .map_err(|_| AcpError::Timeout(format!("{method} timed out")))?
            .map_err(|_| AcpError::Internal(format!("{method} response channel closed")))
            .and_then(|r| r)
    }

    /// Send a JSON-RPC notification (no response expected).
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the writer task has shut down.
    pub fn notify(&self, method: &str, params: Value) -> AcpResult<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_raw(payload)
    }

    /// Send a JSON-RPC success response to an incoming server request.
    ///
    /// Use this to reply to messages received by the notification handler when
    /// they carry an `id` field (i.e. they expect a response).
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the writer task has shut down.
    pub fn respond(&self, id: i64, result: Value) -> AcpResult<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.send_raw(payload)
    }

    /// Send a JSON-RPC error response to an incoming server request.
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the writer task has shut down.
    pub fn respond_error(&self, id: i64, code: i32, message: impl Into<String>) -> AcpResult<()> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message.into(),
            },
        });
        self.send_raw(payload)
    }

    fn send_raw(&self, payload: Value) -> AcpResult<()> {
        let text = serde_json::to_string(&payload)?;
        self.write_tx
            .send(text)
            .map_err(|_| AcpError::Internal("stdio transport writer channel closed".into()))
    }
}

impl fmt::Debug for StdioTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdioTransport")
            .field(
                "request_counter",
                &self.request_counter.load(Ordering::Relaxed),
            )
            .field("rpc_timeout", &self.rpc_timeout)
            .finish_non_exhaustive()
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

// ============================================================================
// Background tasks
// ============================================================================

fn spawn_writer(mut write_rx: mpsc::UnboundedReceiver<String>, mut stdin: ChildStdin) {
    tokio::spawn(async move {
        while let Some(payload) = write_rx.recv().await {
            if stdin.write_all(payload.as_bytes()).await.is_err()
                || stdin.write_all(b"\n").await.is_err()
                || stdin.flush().await.is_err()
            {
                tracing::warn!(
                    target: "vtcode.stdio_transport",
                    "stdin write failed; writer task exiting"
                );
                break;
            }
        }
    });
}

fn spawn_stderr_logger(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    tracing::debug!(target: "vtcode.stdio_transport.stderr", "{}", line.trim_end())
                }
            }
        }
    });
}

fn spawn_reader(
    stdout: ChildStdout,
    pending: Arc<StdMutex<HashMap<i64, oneshot::Sender<AcpResult<Value>>>>>,
    notification_handler: Arc<StdMutex<Option<NotificationHandler>>>,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("stdio transport: JSON decode failed: {e}");
                    continue;
                }
            };

            // Dispatch JSON-RPC responses to pending callers.
            // Extract tx before releasing the lock so `tx.send` runs lock-free.
            if let Some(id) = response_id(&message) {
                let result = extract_rpc_result(&message);
                let tx = pending.lock().ok().and_then(|mut g| g.remove(&id));
                if let Some(tx) = tx {
                    let _ = tx.send(result);
                }
                continue;
            }

            // Clone the handler Arc out of the lock so the lock is released
            // before the handler runs (prevents re-entrancy / call-site latency).
            if let Some(handler) = notification_handler
                .lock()
                .ok()
                .and_then(|g| g.as_ref().cloned())
                && let Err(e) = handler(message)
            {
                tracing::warn!("stdio transport: notification handler error: {e}");
            }
        }
    });
}

// ============================================================================
// Helpers
// ============================================================================

/// Returns the `id` if the message is a JSON-RPC *response* (has `result` or `error`).
fn response_id(message: &Value) -> Option<i64> {
    if message.get("result").is_some() || message.get("error").is_some() {
        message.get("id").and_then(Value::as_i64)
    } else {
        None
    }
}

fn extract_rpc_result(message: &Value) -> AcpResult<Value> {
    if let Some(error) = message.get("error") {
        let code = error
            .get("code")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let detail = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        Err(AcpError::RemoteError {
            agent_id: "stdio".into(),
            message: format!("rpc error {code}: {detail}"),
            code: Some(code as i32),
        })
    } else {
        Ok(message.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_id_requires_result_or_error() {
        // Pure notification: no result/error
        assert!(
            response_id(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "some/notification",
                "params": {}
            }))
            .is_none()
        );

        // Server-initiated request with id but no result
        assert!(
            response_id(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "permission.request",
                "params": {}
            }))
            .is_none()
        );

        // Response has result
        assert_eq!(
            response_id(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": { "ok": true }
            })),
            Some(3)
        );

        // Error response
        assert_eq!(
            response_id(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 5,
                "error": { "code": -32601, "message": "method not found" }
            })),
            Some(5)
        );
    }

    #[test]
    fn extract_rpc_result_propagates_error() {
        let result = extract_rpc_result(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32600, "message": "invalid request" }
        }));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid request"));
    }

    #[test]
    fn extract_rpc_result_returns_result_value() {
        let result = extract_rpc_result(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": { "sessionId": "abc" }
        }))
        .unwrap();
        assert_eq!(result["sessionId"], "abc");
    }

    #[test]
    fn notify_serialises_payload_to_write_channel() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport
            .notify("session/cancel", serde_json::json!({ "sessionId": "s1" }))
            .unwrap();

        let raw = rx.try_recv().expect("notification payload");
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["method"], "session/cancel");
        assert_eq!(payload["params"]["sessionId"], "s1");
        assert!(
            payload.get("id").is_none(),
            "notifications must not have id"
        );
    }

    #[test]
    fn respond_writes_jsonrpc_result() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport
            .respond(42, serde_json::json!({ "ok": true }))
            .unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["jsonrpc"], "2.0");
        assert_eq!(payload["id"], 42);
        assert_eq!(payload["result"]["ok"], true);
    }

    #[test]
    fn respond_error_writes_jsonrpc_error() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport
            .respond_error(9, -32601, "method not found")
            .unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["id"], 9);
        assert_eq!(payload["error"]["code"], -32601);
        assert_eq!(payload["error"]["message"], "method not found");
    }
}
