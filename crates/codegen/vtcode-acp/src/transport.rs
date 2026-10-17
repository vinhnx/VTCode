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
//!
//! ## Backpressure
//!
//! The write channel is bounded (capacity 64) to prevent unbounded memory growth
//! if the subprocess is slow to consume stdin. `send_raw` uses
//! `try_send` and returns an error if the channel is full, rather than blocking
//! or growing without bound.

use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use hashbrown::HashMap;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use crate::error::{AcpError, AcpResult};
use vtcode_commons::sanitizer::{PROVIDER_DIAGNOSTIC_MAX_BYTES, sanitize_provider_diagnostic};

/// Capacity of the bounded write channel. Limits in-flight JSON-RPC messages
/// to prevent unbounded memory growth when the subprocess is slow.
const WRITE_CHANNEL_CAPACITY: usize = 64;

/// Maximum size of one newline-delimited JSON-RPC frame received from or sent
/// to a subprocess. Oversized input is drained before being rejected so the
/// next frame remains aligned.
const MAX_JSON_RPC_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

type PendingRequestMap = HashMap<String, oneshot::Sender<AcpResult<Value>>>;
type PendingRequestStore = Arc<StdMutex<PendingRequestMap>>;

/// Callback type for incoming server→client requests and notifications.
///
/// The handler receives the raw JSON-RPC message value. It should return
/// `Ok(())` on success; errors are logged as warnings by the transport.
type NotificationHandler = Arc<dyn Fn(Value) -> anyhow::Result<()> + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub struct StdioTransportOptions {
    pub include_jsonrpc_version: bool,
}

impl Default for StdioTransportOptions {
    fn default() -> Self {
        Self { include_jsonrpc_version: true }
    }
}

/// Generic JSON-RPC-over-stdio transport for local subprocess agents.
///
/// Wraps a child process and provides:
/// - [`call`](Self::call): send a request and await its response.
/// - [`notify`](Self::notify): send a fire-and-forget notification.
/// - [`respond_value`](Self::respond_value) / [`respond_error_value`](Self::respond_error_value): reply to
///   incoming server-initiated requests.
/// - [`set_notification_handler`](Self::set_notification_handler): register the handler
///   that receives all incoming server→client messages.
///
/// The child process is killed when this struct is dropped.
pub struct StdioTransport {
    write_tx: mpsc::Sender<String>,
    pending: PendingRequestStore,
    request_counter: AtomicI64,
    notification_handler: Arc<StdMutex<Option<NotificationHandler>>>,
    child: StdMutex<Option<Child>>,
    rpc_timeout: Duration,
    options: StdioTransportOptions,
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
        Self::from_child_with_options(child, stdin, stdout, stderr, rpc_timeout, StdioTransportOptions::default())
    }

    pub fn from_child_with_options(
        child: Child,
        stdin: ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
        rpc_timeout: Duration,
        options: StdioTransportOptions,
    ) -> Self {
        let (write_tx, write_rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let pending = Arc::new(StdMutex::new(HashMap::new()));
        let notification_handler = Arc::new(StdMutex::new(None));

        spawn_writer(write_rx, stdin);
        spawn_stderr_logger(stderr);
        spawn_reader(stdout, Arc::clone(&pending), Arc::clone(&notification_handler));

        Self {
            write_tx,
            pending,
            request_counter: AtomicI64::new(1),
            notification_handler,
            child: StdMutex::new(Some(child)),
            rpc_timeout,
            options,
        }
    }

    /// Construct a transport with a pre-wired channel for unit tests.
    ///
    /// No subprocess is spawned and no background tasks are started. The caller
    /// can drive the mock by reading from the paired receiver.
    #[cfg(test)]
    fn new_for_testing(write_tx: mpsc::Sender<String>, rpc_timeout: Duration) -> Self {
        Self::new_for_testing_with_options(write_tx, rpc_timeout, StdioTransportOptions::default())
    }

    #[cfg(test)]
    fn new_for_testing_with_options(
        write_tx: mpsc::Sender<String>,
        rpc_timeout: Duration,
        options: StdioTransportOptions,
    ) -> Self {
        Self {
            write_tx,
            pending: Arc::new(StdMutex::new(HashMap::new())),
            request_counter: AtomicI64::new(1),
            notification_handler: Arc::new(StdMutex::new(None)),
            child: StdMutex::new(None),
            rpc_timeout,
            options,
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
        let id = self.request_counter.fetch_add(1, Ordering::Relaxed);
        let id_value = Value::from(id);
        let pending_key = response_id_key(&id_value);
        let (tx, rx) = oneshot::channel();
        drop(
            self.pending
                .lock()
                .map_err(|_err| AcpError::Internal("stdio transport pending mutex poisoned".into()))?
                .insert(pending_key.clone(), tx),
        );
        let _pending_guard = PendingRequestGuard::new(Arc::clone(&self.pending), pending_key.clone());

        let mut payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        maybe_strip_jsonrpc_field(&mut payload, self.options);
        self.send_raw(payload)?;

        timeout(self.rpc_timeout, rx)
            .await
            .map_err(|_err| AcpError::Timeout(format!("{method} timed out")))?
            .map_err(|_err| AcpError::Internal(format!("{method} response channel closed")))
            .and_then(|r| r)
    }

    /// Send a JSON-RPC notification (no response expected).
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the writer task has shut down.
    pub fn notify(&self, method: &str, params: Value) -> AcpResult<()> {
        let mut payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        maybe_strip_jsonrpc_field(&mut payload, self.options);
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
    fn respond(&self, id: i64, result: Value) -> AcpResult<()> {
        self.respond_value(Value::from(id), result)
    }

    pub fn respond_value(&self, id: Value, result: Value) -> AcpResult<()> {
        let mut payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        maybe_strip_jsonrpc_field(&mut payload, self.options);
        self.send_raw(payload)
    }

    /// Send a JSON-RPC error response to an incoming server request.
    ///
    /// # Errors
    ///
    /// Returns an error if serialisation fails or the writer task has shut down.
    fn respond_error(&self, id: i64, code: i32, message: impl Into<String>) -> AcpResult<()> {
        self.respond_error_value(Value::from(id), code, message)
    }

    pub fn respond_error_value(&self, id: Value, code: i32, message: impl Into<String>) -> AcpResult<()> {
        let mut payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message.into(),
            },
        });
        maybe_strip_jsonrpc_field(&mut payload, self.options);
        self.send_raw(payload)
    }

    fn send_raw(&self, payload: Value) -> AcpResult<()> {
        let text = serde_json::to_string(&payload)?;
        if text.len() > MAX_JSON_RPC_MESSAGE_BYTES {
            return Err(AcpError::Internal(format!(
                "stdio transport JSON-RPC frame exceeds {MAX_JSON_RPC_MESSAGE_BYTES} byte limit"
            )));
        }
        self.write_tx.try_send(text).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                AcpError::Internal("stdio transport write channel full; subprocess may be slow".into())
            }
            mpsc::error::TrySendError::Closed(_) => AcpError::Internal("stdio transport writer channel closed".into()),
        })
    }
}

struct PendingRequestGuard {
    pending: PendingRequestStore,
    key: String,
}

impl PendingRequestGuard {
    fn new(pending: PendingRequestStore, key: String) -> Self {
        Self { pending, key }
    }
}

impl Drop for PendingRequestGuard {
    fn drop(&mut self) {
        drop(self.pending.lock().unwrap_or_else(|error| error.into_inner()).remove(&self.key));
    }
}

impl fmt::Debug for StdioTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StdioTransport")
            .field("request_counter", &self.request_counter.load(Ordering::Relaxed))
            .field("rpc_timeout", &self.rpc_timeout)
            .finish_non_exhaustive()
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            drop(child.start_kill());
        }
    }
}

// ============================================================================
// Background tasks
// ============================================================================

fn spawn_writer(mut write_rx: mpsc::Receiver<String>, mut stdin: ChildStdin) {
    drop(tokio::spawn(async move {
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
    }));
}

fn spawn_stderr_logger(stderr: ChildStderr) {
    drop(tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        loop {
            match read_bounded_line(&mut reader, PROVIDER_DIAGNOSTIC_MAX_BYTES).await {
                Ok(Some(line)) => {
                    let diagnostic = sanitize_provider_diagnostic(&line.bytes);
                    tracing::debug!(
                        target: "vtcode.stdio_transport.stderr",
                        truncated = line.truncated,
                        "{diagnostic}"
                    );
                }
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(
                        target: "vtcode.stdio_transport.stderr",
                        error = %error,
                        "stderr reader failed"
                    );
                    break;
                }
            }
        }
    }));
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<BoundedLine>> {
    let mut line = Vec::with_capacity(max_bytes.min(256));
    let truncated = vtcode_commons::line_framing::read_bounded_line(
        reader,
        &mut line,
        max_bytes,
        vtcode_commons::line_framing::LineEnding::ExcludeLf,
    )
    .await?;
    Ok(truncated.map(|truncated| BoundedLine { bytes: line, truncated }))
}

#[derive(Debug)]
struct BoundedLine {
    bytes: Vec<u8>,
    truncated: bool,
}

fn fail_pending_calls(pending: &PendingRequestStore, message: &str) {
    let pending_calls = pending
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .drain()
        .map(|(_, sender)| sender)
        .collect::<Vec<_>>();
    for sender in pending_calls {
        drop(sender.send(Err(AcpError::Internal(message.to_string()))));
    }
}

fn spawn_reader(
    stdout: ChildStdout,
    pending: PendingRequestStore,
    notification_handler: Arc<StdMutex<Option<NotificationHandler>>>,
) {
    drop(tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        loop {
            let line = match read_bounded_line(&mut reader, MAX_JSON_RPC_MESSAGE_BYTES).await {
                Ok(Some(line)) => line,
                Ok(None) => {
                    fail_pending_calls(&pending, "stdio transport stdout reached EOF");
                    break;
                }
                Err(error) => {
                    tracing::warn!("stdio transport: stdout read failed: {error}");
                    fail_pending_calls(&pending, "stdio transport stdout read failed");
                    break;
                }
            };
            if line.truncated {
                tracing::warn!(
                    target: "vtcode.stdio_transport",
                    "stdout JSON-RPC frame exceeded {MAX_JSON_RPC_MESSAGE_BYTES} byte limit; discarded"
                );
                continue;
            }
            if line.bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let message: Value = match serde_json::from_slice(&line.bytes) {
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
                let tx = pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&response_id_key(&id));
                if let Some(tx) = tx {
                    drop(tx.send(result));
                }
                continue;
            }

            // Clone the handler Arc out of the lock so the lock is released
            // before the handler runs (prevents re-entrancy / call-site latency).
            if let Some(handler) = notification_handler.lock().unwrap_or_else(|e| e.into_inner()).as_ref().cloned()
                && let Err(e) = handler(message)
            {
                tracing::warn!("stdio transport: notification handler error: {e}");
            }
        }
    }));
}

// ============================================================================
// Helpers
// ============================================================================

/// Returns the `id` if the message is a JSON-RPC *response* (has `result` or `error`).
fn response_id(message: &Value) -> Option<Value> {
    if message.get("result").is_some() || message.get("error").is_some() {
        message.get("id").cloned()
    } else {
        None
    }
}

fn response_id_key(id: &Value) -> String {
    serde_json::to_string(id).unwrap_or_else(|_| "null".to_string())
}

fn maybe_strip_jsonrpc_field(payload: &mut Value, options: StdioTransportOptions) {
    if options.include_jsonrpc_version {
        return;
    }

    if let Some(object) = payload.as_object_mut() {
        drop(object.remove("jsonrpc"));
    }
}

fn extract_rpc_result(message: &Value) -> AcpResult<Value> {
    if let Some(error) = message.get("error") {
        let code = error.get("code").and_then(Value::as_i64).unwrap_or_default();
        let detail = error.get("message").and_then(Value::as_str).unwrap_or("unknown error");
        Err(AcpError::RemoteError {
            agent_id: "stdio".into(),
            message: format!("rpc error {code}: {detail}"),
            code: Some(i32::try_from(code).unwrap_or(if code < 0 { i32::MIN } else { i32::MAX })),
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
            Some(Value::from(3))
        );

        // Error response
        assert_eq!(
            response_id(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 5,
                "error": { "code": -32601, "message": "method not found" }
            })),
            Some(Value::from(5))
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
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport
            .notify("session/cancel", serde_json::json!({ "sessionId": "s1" }))
            .unwrap();

        let raw = rx.try_recv().expect("notification payload");
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["method"], "session/cancel");
        assert_eq!(payload["params"]["sessionId"], "s1");
        assert!(payload.get("id").is_none(), "notifications must not have id");
    }

    #[test]
    fn respond_writes_jsonrpc_result() {
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport.respond(42, serde_json::json!({ "ok": true })).unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["jsonrpc"], "2.0");
        assert_eq!(payload["id"], 42);
        assert_eq!(payload["result"]["ok"], true);
    }

    #[test]
    fn respond_error_writes_jsonrpc_error() {
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport.respond_error(9, -32601, "method not found").unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["id"], 9);
        assert_eq!(payload["error"]["code"], -32601);
        assert_eq!(payload["error"]["message"], "method not found");
    }

    #[test]
    fn respond_value_supports_string_ids() {
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing(tx, Duration::from_secs(5));

        transport
            .respond_value(Value::String("request-1".to_string()), serde_json::json!({ "ok": true }))
            .unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(payload["id"], "request-1");
        assert_eq!(payload["result"]["ok"], true);
    }

    #[test]
    fn can_omit_jsonrpc_field_for_codex_mode() {
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing_with_options(
            tx,
            Duration::from_secs(5),
            StdioTransportOptions { include_jsonrpc_version: false },
        );

        transport.notify("initialized", serde_json::json!({})).unwrap();

        let raw = rx.try_recv().unwrap();
        let payload: Value = serde_json::from_str(&raw).unwrap();
        assert!(payload.get("jsonrpc").is_none());
        assert_eq!(payload["method"], "initialized");
    }

    #[tokio::test]
    async fn call_timeout_removes_pending_request() {
        let (tx, mut rx) = mpsc::channel(WRITE_CHANNEL_CAPACITY);
        let transport = StdioTransport::new_for_testing(tx, Duration::from_millis(1));

        let result = transport.call("slow", Value::Null).await;

        assert!(matches!(result, Err(AcpError::Timeout(_))));
        drop(rx.recv().await);
        assert!(transport.pending.lock().unwrap_or_else(|error| error.into_inner()).is_empty());
    }

    #[tokio::test]
    async fn bounded_line_cap_excludes_lf_but_retains_cr() -> std::io::Result<()> {
        let mut reader = BufReader::with_capacity(1, b"abc\nxy\r\n".as_slice());
        let first = read_bounded_line(&mut reader, 3).await?.expect("first line");
        assert_eq!(first.bytes, b"abc");
        assert!(!first.truncated);
        let second = read_bounded_line(&mut reader, 3).await?.expect("second line");
        assert_eq!(second.bytes, b"xy\r");
        assert!(!second.truncated);
        Ok(())
    }

    #[tokio::test]
    async fn bounded_line_reader_drains_oversized_frames() -> std::io::Result<()> {
        let mut data = vec![b'x'; PROVIDER_DIAGNOSTIC_MAX_BYTES + 32];
        data.extend_from_slice(b"\nnext\n");
        let mut reader = BufReader::new(data.as_slice());

        let first = read_bounded_line(&mut reader, PROVIDER_DIAGNOSTIC_MAX_BYTES)
            .await?
            .expect("first line");
        assert!(first.truncated);
        assert_eq!(first.bytes.len(), PROVIDER_DIAGNOSTIC_MAX_BYTES);

        let second = read_bounded_line(&mut reader, PROVIDER_DIAGNOSTIC_MAX_BYTES)
            .await?
            .expect("second line");
        assert!(!second.truncated);
        assert_eq!(second.bytes, b"next");
        Ok(())
    }

    #[tokio::test]
    async fn fail_pending_calls_wakes_all_waiters_and_clears_map() {
        let pending = Arc::new(StdMutex::new(HashMap::new()));
        let (first_tx, first_rx) = oneshot::channel();
        let (second_tx, second_rx) = oneshot::channel();
        drop(pending.lock().unwrap().insert("1".into(), first_tx));
        drop(pending.lock().unwrap().insert("2".into(), second_tx));

        fail_pending_calls(&pending, "stdout closed");

        assert!(pending.lock().unwrap().is_empty());
        assert_eq!(first_rx.await.unwrap().unwrap_err().to_string(), "Internal error: stdout closed");
        assert_eq!(second_rx.await.unwrap().unwrap_err().to_string(), "Internal error: stdout closed");
    }
}
