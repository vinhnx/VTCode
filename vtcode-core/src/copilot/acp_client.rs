use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicI64, Ordering};

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{resolve_copilot_command, spawn_copilot_acp_process};

#[derive(Debug)]
pub enum PromptUpdate {
    Text(String),
    Thought(String),
}

#[derive(Debug)]
pub struct PromptCompletion {
    pub stop_reason: String,
}

pub struct PromptSession {
    pub updates: mpsc::UnboundedReceiver<PromptUpdate>,
    pub completion: tokio::task::JoinHandle<Result<PromptCompletion>>,
}

#[derive(Clone)]
pub struct CopilotAcpClient {
    inner: Arc<CopilotAcpClientInner>,
}

struct CopilotAcpClientInner {
    request_counter: AtomicI64,
    write_tx: mpsc::UnboundedSender<String>,
    pending: StdMutex<HashMap<i64, oneshot::Sender<Result<Value>>>>,
    active_prompt: StdMutex<Option<ActivePrompt>>,
    child: StdMutex<Option<Child>>,
    session_id: StdMutex<Option<String>>,
    timeout: std::time::Duration,
}

struct ActivePrompt {
    updates: mpsc::UnboundedSender<PromptUpdate>,
    unsupported_error: Arc<StdMutex<Option<String>>>,
}

impl CopilotAcpClient {
    pub async fn connect(
        config: &CopilotAuthConfig,
        workspace_root: &Path,
        raw_model: Option<&str>,
    ) -> Result<Self> {
        let resolved = resolve_copilot_command(config)?;
        let mut child = spawn_copilot_acp_process(&resolved, workspace_root, raw_model)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stderr unavailable"))?;

        let (write_tx, write_rx) = mpsc::unbounded_channel();
        let inner = Arc::new(CopilotAcpClientInner {
            request_counter: AtomicI64::new(1),
            write_tx,
            pending: StdMutex::new(HashMap::new()),
            active_prompt: StdMutex::new(None),
            child: StdMutex::new(Some(child)),
            session_id: StdMutex::new(None),
            timeout: resolved.auth_timeout,
        });

        spawn_acp_writer(write_rx, stdin);
        spawn_acp_stderr(stderr);
        spawn_acp_reader(stdout, inner.clone());

        let client = Self { inner };
        timeout(resolved.startup_timeout, async {
            client.initialize().await?;
            let session_id = client.create_session(workspace_root.to_path_buf()).await?;
            *client
                .inner
                .session_id
                .lock()
                .map_err(|_| anyhow!("copilot acp session mutex poisoned"))? = Some(session_id);
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("copilot acp startup timeout")??;
        Ok(client)
    }

    fn session_id(&self) -> Result<String> {
        self.inner
            .session_id
            .lock()
            .map_err(|_| anyhow!("copilot acp session mutex poisoned"))?
            .clone()
            .ok_or_else(|| anyhow!("copilot acp session not initialized"))
    }

    pub async fn start_prompt(&self, prompt_text: String) -> Result<PromptSession> {
        let (updates_tx, updates_rx) = mpsc::unbounded_channel();
        let unsupported_error = Arc::new(StdMutex::new(None));
        {
            let mut active_prompt = self
                .inner
                .active_prompt
                .lock()
                .map_err(|_| anyhow!("copilot acp active prompt mutex poisoned"))?;
            if active_prompt.is_some() {
                return Err(anyhow!("copilot acp only supports one active prompt"));
            }
            *active_prompt = Some(ActivePrompt {
                updates: updates_tx,
                unsupported_error: unsupported_error.clone(),
            });
        }

        let client = self.clone();
        let session_id = self.session_id()?;
        let completion = tokio::spawn(async move {
            let result = client
                .call(
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": [
                            {
                                "type": "text",
                                "text": prompt_text,
                            }
                        ]
                    }),
                )
                .await
                .context("copilot acp session/prompt")?;

            client.clear_active_prompt();

            if let Some(message) = unsupported_error
                .lock()
                .map_err(|_| anyhow!("copilot acp unsupported state poisoned"))?
                .clone()
            {
                return Err(anyhow!(message));
            }

            let stop_reason = result
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string();
            Ok(PromptCompletion { stop_reason })
        });

        Ok(PromptSession {
            updates: updates_rx,
            completion,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        self.send_notification(
            "session/cancel",
            json!({
                "sessionId": self.session_id()?,
            }),
        )
    }

    async fn initialize(&self) -> Result<()> {
        let response = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": {
                            "readTextFile": false,
                            "writeTextFile": false,
                        },
                        "terminal": false,
                    },
                    "clientInfo": {
                        "name": "vtcode",
                        "title": "VT Code",
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            )
            .await
            .context("copilot acp initialize")?;

        let protocol_version = response
            .get("protocolVersion")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        if protocol_version != 1 {
            return Err(anyhow!(
                "unsupported copilot acp protocol version {protocol_version}"
            ));
        }

        Ok(())
    }

    async fn create_session(&self, workspace_root: PathBuf) -> Result<String> {
        let response = self
            .call(
                "session/new",
                json!({
                    "cwd": workspace_root,
                    "mcpServers": [],
                }),
            )
            .await
            .context("copilot acp session/new")?;

        response
            .get("sessionId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("copilot acp session/new missing sessionId"))
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.inner.request_counter.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .map_err(|_| anyhow!("copilot acp pending mutex poisoned"))?
            .insert(id, tx);

        self.send_request(id, method, params)?;
        timeout(self.inner.timeout, rx)
            .await
            .with_context(|| format!("copilot acp {method} timeout"))?
            .map_err(|_| anyhow!("copilot acp {method} response channel closed"))?
    }

    fn send_request(&self, id: i64, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        self.send_payload(payload)
    }

    fn send_notification(&self, method: &str, params: Value) -> Result<()> {
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.send_payload(payload)
    }

    fn send_payload(&self, payload: Value) -> Result<()> {
        let payload =
            serde_json::to_string(&payload).context("copilot acp json serialization failed")?;
        self.inner
            .write_tx
            .send(payload)
            .map_err(|_| anyhow!("copilot acp writer channel closed"))
    }

    fn clear_active_prompt(&self) {
        if let Ok(mut active_prompt) = self.inner.active_prompt.lock() {
            *active_prompt = None;
        }
    }
}

impl Drop for CopilotAcpClientInner {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

fn spawn_acp_writer(
    mut write_rx: mpsc::UnboundedReceiver<String>,
    mut stdin: tokio::process::ChildStdin,
) {
    tokio::spawn(async move {
        while let Some(payload) = write_rx.recv().await {
            if let Err(err) = stdin.write_all(payload.as_bytes()).await {
                tracing::warn!(error = %err, "copilot acp writer failed");
                break;
            }
            if let Err(err) = stdin.write_all(b"\n").await {
                tracing::warn!(error = %err, "copilot acp newline write failed");
                break;
            }
            if let Err(err) = stdin.flush().await {
                tracing::warn!(error = %err, "copilot acp writer flush failed");
                break;
            }
        }
    });
}

fn spawn_acp_stderr(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => tracing::debug!(target: "copilot.acp.stderr", "{}", line.trim_end()),
                Err(err) => {
                    tracing::warn!(error = %err, "copilot acp stderr read failed");
                    break;
                }
            }
        }
    });
}

fn spawn_acp_reader(stdout: ChildStdout, inner: Arc<CopilotAcpClientInner>) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(&line) {
                Ok(message) => {
                    if let Err(err) = handle_acp_message(&inner, message) {
                        tracing::warn!(error = %err, "copilot acp message handling failed");
                    }
                }
                Err(err) => tracing::warn!(error = %err, "copilot acp json decode failed"),
            }
        }
    });
}

fn handle_acp_message(inner: &Arc<CopilotAcpClientInner>, message: Value) -> Result<()> {
    if let Some(id) = response_id(&message) {
        let response = if let Some(error) = message.get("error") {
            let code = error
                .get("code")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let detail = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            Err(anyhow!("copilot acp rpc error {code}: {detail}"))
        } else {
            Ok(message.get("result").cloned().unwrap_or(Value::Null))
        };

        if let Some(sender) = inner
            .pending
            .lock()
            .map_err(|_| anyhow!("copilot acp pending mutex poisoned"))?
            .remove(&id)
        {
            let _ = sender.send(response);
        }
        return Ok(());
    }

    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };

    match method {
        "session/update" => handle_session_update(inner, message.get("params"))?,
        client_method => {
            if let Some(id) = request_id(&message) {
                let error_message = unsupported_client_capability_message(client_method);
                mark_prompt_unsupported(inner, error_message.clone())?;
                let payload = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": error_message,
                    }
                });
                inner
                    .write_tx
                    .send(serde_json::to_string(&payload)?)
                    .map_err(|_| anyhow!("copilot acp writer channel closed"))?;
            }
        }
    }

    Ok(())
}

fn handle_session_update(inner: &Arc<CopilotAcpClientInner>, params: Option<&Value>) -> Result<()> {
    let Some(update) = params.and_then(|params| params.get("update")) else {
        return Ok(());
    };
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
        return Ok(());
    };

    match kind {
        "agent_message_chunk" => {
            if let Some(text) = extract_text(update.get("content")) {
                send_prompt_update(inner, PromptUpdate::Text(text))?;
            }
        }
        "agent_thought_chunk" => {
            if let Some(text) = extract_text(update.get("content")) {
                send_prompt_update(inner, PromptUpdate::Thought(text))?;
            }
        }
        "tool_call" | "tool_call_update" | "plan" | "available_commands_update" | "mode_update" => {
            mark_prompt_unsupported(inner, unsupported_client_capability_message(kind))?;
        }
        _ => {}
    }

    Ok(())
}

fn send_prompt_update(inner: &Arc<CopilotAcpClientInner>, update: PromptUpdate) -> Result<()> {
    if let Some(active_prompt) = inner
        .active_prompt
        .lock()
        .map_err(|_| anyhow!("copilot acp active prompt mutex poisoned"))?
        .as_ref()
    {
        let _ = active_prompt.updates.send(update);
    }
    Ok(())
}

fn mark_prompt_unsupported(inner: &Arc<CopilotAcpClientInner>, message: String) -> Result<()> {
    if let Some(active_prompt) = inner
        .active_prompt
        .lock()
        .map_err(|_| anyhow!("copilot acp active prompt mutex poisoned"))?
        .as_ref()
        && let Ok(mut unsupported) = active_prompt.unsupported_error.lock()
        && unsupported.is_none()
    {
        *unsupported = Some(message);
    }
    Ok(())
}

fn unsupported_client_capability_message(method: &str) -> String {
    format!(
        "VT Code's builtin Copilot provider is text-only in v1 and does not support `{method}`."
    )
}

fn extract_text(content: Option<&Value>) -> Option<String> {
    match content {
        Some(Value::Object(map)) => {
            if map.get("type").and_then(Value::as_str) == Some("text") {
                map.get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            } else {
                None
            }
        }
        Some(Value::String(text)) => Some(text.clone()),
        _ => None,
    }
}

fn request_id(message: &Value) -> Option<i64> {
    message.get("id").and_then(Value::as_i64)
}

fn response_id(message: &Value) -> Option<i64> {
    if message.get("result").is_some() || message.get("error").is_some() {
        message.get("id").and_then(Value::as_i64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{extract_text, unsupported_client_capability_message};
    use serde_json::json;

    #[test]
    fn extracts_text_from_text_objects() {
        let text = extract_text(Some(&json!({
            "type": "text",
            "text": "hello",
        })));

        assert_eq!(text.as_deref(), Some("hello"));
    }

    #[test]
    fn formats_unsupported_capability_message() {
        let message = unsupported_client_capability_message("tool_call");

        assert!(message.contains("text-only in v1"));
        assert!(message.contains("tool_call"));
    }
}
