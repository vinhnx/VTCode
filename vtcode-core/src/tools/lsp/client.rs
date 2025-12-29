use anyhow::{Context, Result};
use lsp_types::{
    ClientCapabilities, ClientInfo, InitializeParams, InitializeResult, InitializedParams,
    ServerCapabilities, TraceValue, WorkDoneProgressParams,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct LspClient {
    #[allow(dead_code)]
    child: Mutex<Option<Child>>,
    outbox_tx: tokio::sync::mpsc::Sender<String>,
    next_id: AtomicI64,
    pending_requests: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value>>>>>,
    #[allow(dead_code)]
    reader_handle: Mutex<Option<JoinHandle<()>>>,
    #[allow(dead_code)]
    stderr_handle: Mutex<Option<JoinHandle<()>>>,
    server_capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    workspace_root: PathBuf,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Value, // ID can be number or string
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcNotification {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
    id: Option<i64>, // Response to our requests (which use i64)
}

#[allow(dead_code)]
#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl LspClient {
    pub async fn new(command: &str, args: &[String], workspace_root: PathBuf) -> Result<Arc<Self>> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("Failed to spawn LSP server: {} {:?}", command, args))?;

        let stdin = child.stdin.take().context("Failed to open stdin")?;
        let stdout = child.stdout.take().context("Failed to open stdout")?;
        let stderr = child.stderr.take().context("Failed to open stderr")?;

        // Clone stdin for the reader loop to reply to requests
        // let stdin_for_reader = stdin.try_clone().await.ok();
        // Actually ChildStdin in tokio doesn't implement Clone. We need to wrap it in Arc<Mutex> to share,
        // but it's already in the struct. The reader loop needs a way to write back.
        // Solution: Create a generic "Sender" channel that the reader can use to queue messages to be written to stdin?
        // OR: Just rely on the main "send_request" mechanism? No, because we might arguably be blocked.
        // For simplicity in this iteration: We won't support replying to server requests in the *reader* loop directly nicely
        // without architecture change. However, we CAN just safely ignore them but LOG them,
        // OR we can try to separate the Writer entirely.

        // Let's use a split approach for Stdin if we want to write from multiple places,
        // but Mutex<ChildStdin> is fine if we just want to write.
        // We can't easily pass the Mutex into the task if it's held by the struct.
        // Let's make the writer logic a separate task receiving from a channel.

        let (outbox_tx, mut outbox_rx) = tokio::sync::mpsc::channel::<String>(32);

        // Writer Loop
        let mut stdin_actor = stdin;
        tokio::spawn(async move {
            while let Some(msg) = outbox_rx.recv().await {
                let body = msg;
                let packet = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
                if let Err(_e) = stdin_actor.write_all(packet.as_bytes()).await {
                    break;
                }
                if let Err(_e) = stdin_actor.flush().await {
                    break;
                }
            }
        });

        let pending_requests: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value>>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Reader Loop
        let pending_requests_clone = pending_requests.clone();
        let outbox_tx_clone = outbox_tx.clone();

        let reader_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);
            loop {
                // Read Content-Length header
                let mut content_length = 0;
                let mut header_line = String::new();

                loop {
                    header_line.clear();
                    match reader.read_line(&mut header_line).await {
                        Ok(0) => return, // EOF
                        Ok(_) => {
                            let line = header_line.trim();
                            if line.is_empty() {
                                // End of headers
                                break;
                            }
                            if let Some(len_str) = line.strip_prefix("Content-Length: ")
                                && let Ok(len) = len_str.parse::<usize>()
                            {
                                content_length = len;
                            }
                        }
                        Err(_) => return, // Error
                    }
                }

                if content_length > 0 {
                    let mut body_buffer = vec![0u8; content_length];
                    if reader.read_exact(&mut body_buffer).await.is_ok()
                        && let Ok(body_str) = String::from_utf8(body_buffer)
                    {
                        // Try to parse as generic generic JSON first to determine type
                        if let Ok(value) = serde_json::from_str::<Value>(&body_str) {
                            if let Some(id_val) = value.get("id") {
                                if value.get("method").is_some() {
                                    // It's a Request from Server
                                    // We should reply with MethodNotFound or similar to avoid hanging
                                    let id = id_val.clone();
                                    let response = json!({
                                        "jsonrpc": "2.0",
                                        "id": id,
                                        "error": {
                                            "code": -32601,
                                            "message": "Method not found (client side)"
                                        }
                                    });
                                    if let Ok(resp_str) = serde_json::to_string(&response) {
                                        let _ = outbox_tx_clone.send(resp_str).await;
                                    }
                                } else {
                                    // It's a Response to our request
                                    if let Some(id) = id_val.as_i64() {
                                        let mut pending = pending_requests_clone.lock().await;
                                        if let Some(sender) = pending.remove(&id) {
                                            let result = if let Some(error) = value.get("error") {
                                                let code = error
                                                    .get("code")
                                                    .and_then(|c| c.as_i64())
                                                    .unwrap_or(0);
                                                let msg = error
                                                    .get("message")
                                                    .and_then(|s| s.as_str())
                                                    .unwrap_or("Unknown error");
                                                Err(anyhow::anyhow!("LSP Error {}: {}", code, msg))
                                            } else {
                                                Ok(value
                                                    .get("result")
                                                    .cloned()
                                                    .unwrap_or(Value::Null))
                                            };
                                            let _ = sender.send(result);
                                        }
                                    }
                                }
                            } else {
                                // Notification (no id)
                                if value.get("method").and_then(|s| s.as_str())
                                    == Some("window/showMessage")
                                {
                                    // Handle log message?
                                    // println!("LSP Message: {:?}", value.get("params"));
                                }
                            }
                        }
                    }
                }
            }
        });

        // Start stderr logger
        let stderr_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while let Ok(len) = reader.read_line(&mut line).await {
                if len == 0 {
                    break;
                }
                // Log stderr (could use tracing or eprintln)
                // tracing::debug!("LSP Stderr: {}", line);
                line.clear();
            }
        });

        let client = Arc::new(Self {
            child: Mutex::new(Some(child)),
            outbox_tx,
            next_id: AtomicI64::new(1),
            pending_requests,
            reader_handle: Mutex::new(Some(reader_handle)),
            stderr_handle: Mutex::new(Some(stderr_handle)),
            server_capabilities: Arc::new(Mutex::new(None)),
            workspace_root,
        });

        Ok(client)
    }

    #[allow(deprecated)]
    pub async fn initialize(&self) -> Result<()> {
        let root_uri: lsp_types::Uri = url::Url::from_directory_path(&self.workspace_root)
            .map_err(|_| anyhow::anyhow!("Invalid workspace root path"))?
            .to_string()
            .parse()
            .map_err(|_| anyhow::anyhow!("Failed to parse Uri"))?;

        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri.clone()),
            capabilities: ClientCapabilities::default(),
            trace: Some(TraceValue::Off),
            workspace_folders: None,
            client_info: Some(ClientInfo {
                name: "vtcode".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            locale: None,
            initialization_options: None,
            root_path: None, // Deprecated but some servers use it
            work_done_progress_params: WorkDoneProgressParams {
                work_done_token: None,
            },
        };

        let result_value = self
            .send_request("initialize", serde_json::to_value(params)?)
            .await?;
        let result: InitializeResult = serde_json::from_value(result_value)?;

        *self.server_capabilities.lock().await = Some(result.capabilities);

        // Send initialized notification
        self.send_notification("initialized", serde_json::to_value(InitializedParams {})?)
            .await?;

        Ok(())
    }

    pub async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
            id: Value::Number(id.into()),
        };

        let body = serde_json::to_string(&request)?;
        let (sender, receiver) = oneshot::channel();

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, sender);
        }

        self.outbox_tx
            .send(body)
            .await
            .map_err(|_| anyhow::anyhow!("LSP writer closed"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(30), receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(anyhow::anyhow!("LSP request channel closed")),
            Err(_) => {
                // Timeout, remove pending request
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&id);
                Err(anyhow::anyhow!("LSP request timed out"))
            }
        }
    }

    async fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: Some(params),
        };

        let body = serde_json::to_string(&notification)?;
        self.outbox_tx
            .send(body)
            .await
            .map_err(|_| anyhow::anyhow!("LSP writer closed"))?;

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        let _ = self.send_request("shutdown", Value::Null).await;
        let _ = self.send_notification("exit", Value::Null).await;
        Ok(())
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // We can't do async drop easily, but we can signal the handles to stop
        // and attempt to kill the child process if it's still running.
        // Actual cleanup relies on OS releasing process resources if the parent dies,
        // but we try to clean up gracefully.

        // Try to kill the child process if it exists
        let child_guard = self.child.try_lock();
        if let Ok(mut child_opt) = child_guard {
            if let Some(mut child) = child_opt.take() {
                // Try to kill the child process gracefully
                let _ = child.start_kill();
            }
        }

        // Try to abort the reader handle if it exists
        let reader_guard = self.reader_handle.try_lock();
        if let Ok(mut handle_opt) = reader_guard {
            if let Some(handle) = handle_opt.take() {
                handle.abort();
            }
        }

        // Try to abort the stderr handle if it exists
        let stderr_guard = self.stderr_handle.try_lock();
        if let Ok(mut handle_opt) = stderr_guard {
            if let Some(handle) = handle_opt.take() {
                handle.abort();
            }
        }
    }
}
