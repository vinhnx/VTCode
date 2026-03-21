use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::time::timeout;
use vtcode_acp_client::StdioTransport;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{
    CopilotModelSelectionMode, resolve_copilot_command, spawn_copilot_acp_process,
};
use super::types::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus,
    CopilotPermissionDecision, CopilotPermissionRequest, CopilotShellCommandSummary,
    CopilotToolCallFailure, CopilotToolCallRequest, CopilotToolCallResponse,
};
use crate::llm::provider::ToolDefinition;

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
    pub updates: tokio::sync::mpsc::UnboundedReceiver<PromptUpdate>,
    pub runtime_requests: tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>,
    pub completion: tokio::task::JoinHandle<Result<PromptCompletion>>,
    cancel_handle: PromptSessionCancelHandle,
}

#[derive(Clone)]
pub struct PromptSessionCancelHandle {
    client: CopilotAcpClient,
    completion_abort: tokio::task::AbortHandle,
}

impl PromptSessionCancelHandle {
    pub fn cancel(&self) {
        let _ = self.client.cancel();
        self.client.clear_active_prompt();
        self.completion_abort.abort();
    }
}

impl PromptSession {
    pub fn into_parts(
        self,
    ) -> (
        tokio::sync::mpsc::UnboundedReceiver<PromptUpdate>,
        tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>,
        tokio::task::JoinHandle<Result<PromptCompletion>>,
        PromptSessionCancelHandle,
    ) {
        (
            self.updates,
            self.runtime_requests,
            self.completion,
            self.cancel_handle,
        )
    }
}

#[derive(Debug)]
pub enum CopilotRuntimeRequest {
    Permission(PendingPermissionRequest),
    ToolCall(PendingToolCallRequest),
    ObservedToolCall(CopilotObservedToolCall),
    CompatibilityNotice(CopilotCompatibilityNotice),
}

#[derive(Debug)]
pub struct PendingPermissionRequest {
    pub request: CopilotPermissionRequest,
    response_tx: tokio::sync::oneshot::Sender<Value>,
    response_format: PermissionResponseFormat,
}

impl PendingPermissionRequest {
    pub fn respond(self, decision: CopilotPermissionDecision) -> Result<()> {
        self.response_tx
            .send(self.response_format.render(decision))
            .map_err(|_| anyhow!("copilot permission response channel closed"))
    }
}

#[derive(Debug)]
pub struct PendingToolCallRequest {
    pub request: CopilotToolCallRequest,
    response_tx: tokio::sync::oneshot::Sender<CopilotToolCallResponse>,
}

impl PendingToolCallRequest {
    pub fn respond(self, response: CopilotToolCallResponse) -> Result<()> {
        self.response_tx
            .send(response)
            .map_err(|_| anyhow!("copilot tool response channel closed"))
    }
}

#[derive(Clone)]
pub struct CopilotAcpClient {
    inner: Arc<CopilotAcpClientInner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotCompatibilityNotice {
    pub state: CopilotAcpCompatibilityState,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Inner state — Copilot-specific only.
// The generic transport machinery (request correlation, child I/O) lives in
// StdioTransport.
// ---------------------------------------------------------------------------

struct CopilotAcpClientInner {
    /// Generic JSON-RPC-over-stdio transport.
    transport: StdioTransport,
    /// State for the currently active prompt session (if any).
    active_prompt: StdMutex<Option<ActivePrompt>>,
    /// Copilot session identifier (set after session.create succeeds).
    session_id: StdMutex<Option<String>>,
    /// Copilot ACP compatibility state (updated as messages arrive).
    compatibility_state: StdMutex<CopilotAcpCompatibilityState>,
}

struct ActivePrompt {
    updates: tokio::sync::mpsc::UnboundedSender<PromptUpdate>,
    runtime_requests: tokio::sync::mpsc::UnboundedSender<CopilotRuntimeRequest>,
}

#[derive(Debug, Clone)]
enum PermissionResponseFormat {
    CopilotCli,
    AcpLegacy { options: Vec<AcpPermissionOption> },
}

impl PermissionResponseFormat {
    fn render(self, decision: CopilotPermissionDecision) -> Value {
        match self {
            Self::CopilotCli => json!({
                "result": decision.to_rpc_result(),
            }),
            Self::AcpLegacy { options } => json!({
                "outcome": legacy_permission_outcome(&options, &decision),
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct AcpPermissionOption {
    option_id: String,
    kind: AcpPermissionOptionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AcpPermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Other,
}

// ---------------------------------------------------------------------------
// CopilotAcpClient implementation
// ---------------------------------------------------------------------------

impl CopilotAcpClient {
    pub async fn connect(
        config: &CopilotAuthConfig,
        workspace_root: &Path,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<Self> {
        match Self::connect_once(
            config,
            workspace_root,
            raw_model,
            custom_tools,
            CopilotModelSelectionMode::CliArgument,
        )
        .await
        {
            Ok(client) => Ok(client),
            Err(primary_error) if raw_model.is_some() => Self::connect_once(
                config,
                workspace_root,
                raw_model,
                custom_tools,
                CopilotModelSelectionMode::EnvironmentVariable,
            )
            .await
            .with_context(|| {
                format!(
                    "copilot acp startup with --model failed first: {}",
                    primary_error
                )
            }),
            Err(error) => Err(error),
        }
    }

    async fn connect_once(
        config: &CopilotAuthConfig,
        workspace_root: &Path,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
        model_selection_mode: CopilotModelSelectionMode,
    ) -> Result<Self> {
        let resolved = resolve_copilot_command(config)?;
        let mut child = spawn_copilot_acp_process(
            &resolved,
            config,
            workspace_root,
            raw_model,
            model_selection_mode,
        )?;
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

        let transport =
            StdioTransport::from_child(child, stdin, stdout, stderr, resolved.auth_timeout);

        let inner = Arc::new(CopilotAcpClientInner {
            transport,
            active_prompt: StdMutex::new(None),
            session_id: StdMutex::new(None),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::Unavailable),
        });

        // Register the Copilot-specific notification handler.
        // Use a Weak reference to avoid a retain cycle:
        //   Arc<Inner> → StdioTransport → handler → Weak<Inner>
        let inner_weak = Arc::downgrade(&inner);
        inner
            .transport
            .set_notification_handler(Arc::new(move |message| {
                if let Some(inner_strong) = inner_weak.upgrade() {
                    handle_acp_message(&inner_strong, message)?;
                }
                Ok(())
            }));

        let client = Self { inner };
        timeout(resolved.startup_timeout, async {
            client.initialize().await?;
            let session_id = client
                .create_session(
                    config,
                    workspace_root.to_path_buf(),
                    raw_model,
                    custom_tools,
                )
                .await?;
            *client
                .inner
                .session_id
                .lock()
                .map_err(|_| anyhow!("copilot acp session mutex poisoned"))? = Some(session_id);
            *client
                .inner
                .compatibility_state
                .lock()
                .map_err(|_| anyhow!("copilot acp compatibility mutex poisoned"))? =
                CopilotAcpCompatibilityState::FullTools;
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
        let (updates_tx, updates_rx) = tokio::sync::mpsc::unbounded_channel();
        let (runtime_tx, runtime_rx) = tokio::sync::mpsc::unbounded_channel();
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
                runtime_requests: runtime_tx,
            });
        }

        if self.compatibility_state()? == CopilotAcpCompatibilityState::PromptOnly {
            enqueue_runtime_request(
                &self.inner,
                CopilotRuntimeRequest::CompatibilityNotice(CopilotCompatibilityNotice {
                    state: CopilotAcpCompatibilityState::PromptOnly,
                    message: "GitHub Copilot ACP is running in prompt-only degraded mode. VT Code will keep the session alive, but Copilot-native runtime hooks are partially incompatible.".to_string(),
                }),
            )?;
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
                .context("copilot acp session/prompt");

            client.clear_active_prompt();
            let result = result?;

            let stop_reason = result
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string();
            Ok(PromptCompletion { stop_reason })
        });
        let cancel_handle = PromptSessionCancelHandle {
            client: self.clone(),
            completion_abort: completion.abort_handle(),
        };

        Ok(PromptSession {
            updates: updates_rx,
            runtime_requests: runtime_rx,
            completion,
            cancel_handle,
        })
    }

    pub fn cancel(&self) -> Result<()> {
        self.inner
            .transport
            .notify(
                "session/cancel",
                json!({
                    "sessionId": self.session_id()?,
                }),
            )
            .map_err(anyhow::Error::from)
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

    async fn create_session(
        &self,
        config: &CopilotAuthConfig,
        workspace_root: PathBuf,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<String> {
        match self
            .create_session_v2(config, workspace_root.clone(), raw_model, custom_tools)
            .await
        {
            Ok(session_id) => Ok(session_id),
            Err(v2_error) => self
                .create_session_v1(workspace_root)
                .await
                .with_context(|| format!("copilot acp session.create failed first: {v2_error}")),
        }
    }

    async fn create_session_v2(
        &self,
        config: &CopilotAuthConfig,
        workspace_root: PathBuf,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<String> {
        let mut params = serde_json::Map::from_iter([
            (
                "clientName".to_string(),
                Value::String("VT Code".to_string()),
            ),
            ("workingDirectory".to_string(), json!(workspace_root)),
            ("requestPermission".to_string(), Value::Bool(true)),
            ("streaming".to_string(), Value::Bool(true)),
            ("mcpServers".to_string(), Value::Array(Vec::new())),
        ]);

        if let Some(raw_model) = raw_model.filter(|value| !value.trim().is_empty()) {
            params.insert("model".to_string(), Value::String(raw_model.to_string()));
        }
        let custom_tools = custom_tools_payload(custom_tools);
        if !custom_tools.is_empty() {
            params.insert("tools".to_string(), Value::Array(custom_tools));
        }
        if !config.available_tools.is_empty() {
            params.insert("availableTools".to_string(), json!(config.available_tools));
        }
        if !config.excluded_tools.is_empty() {
            params.insert("excludedTools".to_string(), json!(config.excluded_tools));
        }

        let response = self
            .call("session.create", Value::Object(params))
            .await
            .context("copilot acp session.create")?;

        response
            .get("sessionId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("copilot acp session.create missing sessionId"))
    }

    async fn create_session_v1(&self, workspace_root: PathBuf) -> Result<String> {
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
        self.inner
            .transport
            .call(method, params)
            .await
            .map_err(anyhow::Error::from)
    }

    fn clear_active_prompt(&self) {
        if let Ok(mut active_prompt) = self.inner.active_prompt.lock() {
            *active_prompt = None;
        }
    }

    pub fn compatibility_state(&self) -> Result<CopilotAcpCompatibilityState> {
        self.inner
            .compatibility_state
            .lock()
            .map(|state| *state)
            .map_err(|_| anyhow!("copilot acp compatibility mutex poisoned"))
    }
}

// ---------------------------------------------------------------------------
// ACP message dispatch (Copilot-specific protocol)
// ---------------------------------------------------------------------------
// StdioTransport already handles JSON-RPC response routing (id → pending).
// This function receives only server-initiated requests and notifications.

fn handle_acp_message(inner: &Arc<CopilotAcpClientInner>, message: Value) -> Result<()> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Ok(());
    };

    match method {
        "session/update" => handle_session_update(inner, message.get("params"))?,
        "permission.request" => handle_permission_request(inner, &message)?,
        "session/request_permission" => handle_legacy_permission_request(inner, &message)?,
        "tool.call" => handle_tool_call_request(inner, &message)?,
        client_method => {
            if let Some(id) = request_id(&message) {
                let error_message = unsupported_client_capability_message(client_method);
                mark_prompt_degraded(inner, error_message.clone())?;
                inner
                    .transport
                    .respond_error(id, -32601, error_message)
                    .map_err(anyhow::Error::from)?;
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
        "tool_call" | "tool_call_update" => {
            if let Some(tool_call) = parse_observed_tool_call(update) {
                match enqueue_runtime_request(
                    inner,
                    CopilotRuntimeRequest::ObservedToolCall(tool_call),
                ) {
                    Ok(_) => {}
                    Err(err) if is_runtime_request_channel_closed_error(&err) => {}
                    Err(err) => return Err(err),
                }
            } else {
                mark_prompt_degraded(
                    inner,
                    "GitHub Copilot ACP sent an unparseable tool call update; VT Code is continuing in prompt-only degraded mode.".to_string(),
                )?;
            }
        }
        "plan" | "available_commands_update" | "mode_update" => {}
        _ => {}
    }

    Ok(())
}

fn handle_permission_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    let Some(id) = request_id(message) else {
        return Ok(());
    };

    let request = message
        .get("params")
        .and_then(|params| params.get("permissionRequest"))
        .cloned()
        .map(parse_permission_request)
        .transpose()?
        .unwrap_or(CopilotPermissionRequest::Unknown {
            kind: None,
            raw: Value::Null,
        });

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let dispatched = match enqueue_runtime_request(
        inner,
        CopilotRuntimeRequest::Permission(PendingPermissionRequest {
            request,
            response_tx,
            response_format: PermissionResponseFormat::CopilotCli,
        }),
    ) {
        Ok(dispatched) => dispatched,
        Err(err) if is_runtime_request_channel_closed_error(&err) => false,
        Err(err) => return Err(err),
    };
    if dispatched {
        let inner = inner.clone();
        tokio::spawn(async move {
            match response_rx.await {
                Ok(result) => {
                    let _ = inner.transport.respond(id, result).map_err(|e| {
                        tracing::warn!("copilot acp permission respond failed: {e}");
                    });
                }
                Err(_) => {
                    let fallback = PermissionResponseFormat::CopilotCli
                        .render(CopilotPermissionDecision::DeniedNoApprovalRule);
                    let _ = inner.transport.respond(id, fallback).map_err(|e| {
                        tracing::warn!("copilot acp permission fallback respond failed: {e}");
                    });
                }
            }
        });
        Ok(())
    } else {
        let fallback = PermissionResponseFormat::CopilotCli
            .render(CopilotPermissionDecision::DeniedNoApprovalRule);
        inner
            .transport
            .respond(id, fallback)
            .map_err(anyhow::Error::from)
    }
}

fn handle_legacy_permission_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    let Some(id) = request_id(message) else {
        return Ok(());
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let request = params
        .get("toolCall")
        .cloned()
        .map(parse_legacy_permission_request)
        .transpose()?
        .unwrap_or(CopilotPermissionRequest::Unknown {
            kind: Some("session/request_permission".to_string()),
            raw: Value::Null,
        });
    let options = parse_permission_options(params.get("options"));

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let dispatched = match enqueue_runtime_request(
        inner,
        CopilotRuntimeRequest::Permission(PendingPermissionRequest {
            request,
            response_tx,
            response_format: PermissionResponseFormat::AcpLegacy { options },
        }),
    ) {
        Ok(dispatched) => dispatched,
        Err(err) if is_runtime_request_channel_closed_error(&err) => false,
        Err(err) => return Err(err),
    };
    if dispatched {
        let inner = inner.clone();
        tokio::spawn(async move {
            match response_rx.await {
                Ok(result) => {
                    let _ = inner.transport.respond(id, result).map_err(|e| {
                        tracing::warn!("copilot acp legacy permission respond failed: {e}");
                    });
                }
                Err(_) => {
                    let fallback = json!({ "outcome": { "outcome": "cancelled" } });
                    let _ = inner.transport.respond(id, fallback).map_err(|e| {
                        tracing::warn!(
                            "copilot acp legacy permission fallback respond failed: {e}"
                        );
                    });
                }
            }
        });
        Ok(())
    } else {
        let fallback = json!({ "outcome": { "outcome": "cancelled" } });
        inner
            .transport
            .respond(id, fallback)
            .map_err(anyhow::Error::from)
    }
}

fn handle_tool_call_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    let Some(id) = request_id(message) else {
        return Ok(());
    };

    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let request = CopilotToolCallRequest {
        tool_call_id: params
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tool_name: params
            .get("toolName")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        arguments: params.get("arguments").cloned().unwrap_or(Value::Null),
    };
    let fallback_tool_name = request.tool_name.clone();

    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
    let dispatched = match enqueue_runtime_request(
        inner,
        CopilotRuntimeRequest::ToolCall(PendingToolCallRequest {
            request,
            response_tx,
        }),
    ) {
        Ok(dispatched) => dispatched,
        Err(err) if is_runtime_request_channel_closed_error(&err) => false,
        Err(err) => return Err(err),
    };
    if dispatched {
        let inner = inner.clone();
        tokio::spawn(async move {
            match response_rx.await {
                Ok(response) => {
                    let result = build_tool_call_result(response);
                    let _ = inner.transport.respond(id, result).map_err(|e| {
                        tracing::warn!("copilot acp tool call respond failed: {e}");
                    });
                }
                Err(_) => {
                    let result = build_tool_call_result(CopilotToolCallResponse::Failure(
                        CopilotToolCallFailure {
                            text_result_for_llm: format!(
                                "VT Code could not complete the client tool `{fallback_tool_name}`."
                            ),
                            error: format!("tool '{fallback_tool_name}' response channel closed"),
                        },
                    ));
                    let _ = inner.transport.respond(id, result).map_err(|e| {
                        tracing::warn!("copilot acp tool call fallback respond failed: {e}");
                    });
                }
            }
        });
        Ok(())
    } else {
        let result = build_tool_call_result(CopilotToolCallResponse::Failure(
            CopilotToolCallFailure {
                text_result_for_llm: format!(
                    "VT Code does not expose the client tool `{fallback_tool_name}` to GitHub Copilot."
                ),
                error: format!("tool '{fallback_tool_name}' not supported by VT Code"),
            },
        ));
        inner
            .transport
            .respond(id, result)
            .map_err(anyhow::Error::from)
    }
}

// ---------------------------------------------------------------------------
// Active prompt helpers
// ---------------------------------------------------------------------------

fn send_prompt_update(inner: &Arc<CopilotAcpClientInner>, update: PromptUpdate) -> Result<()> {
    if let Some(active_prompt) = inner
        .active_prompt
        .lock()
        .map_err(|_| anyhow!("copilot acp active prompt mutex poisoned"))?
        .as_ref()
        && active_prompt.updates.send(update).is_err()
    {
        clear_active_prompt_state(inner);
    }
    Ok(())
}

fn mark_prompt_degraded(inner: &Arc<CopilotAcpClientInner>, message: String) -> Result<()> {
    {
        let mut compatibility_state = inner
            .compatibility_state
            .lock()
            .map_err(|_| anyhow!("copilot acp compatibility mutex poisoned"))?;
        if *compatibility_state == CopilotAcpCompatibilityState::PromptOnly {
            return Ok(());
        }
        *compatibility_state = CopilotAcpCompatibilityState::PromptOnly;
    }
    tracing::warn!(
        target: "copilot.acp",
        message = %message,
        "GitHub Copilot ACP switched to prompt-only degraded mode"
    );
    match enqueue_runtime_request(
        inner,
        CopilotRuntimeRequest::CompatibilityNotice(CopilotCompatibilityNotice {
            state: CopilotAcpCompatibilityState::PromptOnly,
            message,
        }),
    ) {
        Ok(_) => {}
        Err(err) if is_runtime_request_channel_closed_error(&err) => {}
        Err(err) => return Err(err),
    }
    Ok(())
}

fn enqueue_runtime_request(
    inner: &Arc<CopilotAcpClientInner>,
    request: CopilotRuntimeRequest,
) -> Result<bool> {
    let sender = inner
        .active_prompt
        .lock()
        .map_err(|_| anyhow!("copilot acp active prompt mutex poisoned"))?
        .as_ref()
        .map(|active_prompt| active_prompt.runtime_requests.clone());
    let Some(sender) = sender else {
        return Ok(false);
    };

    if sender.send(request).is_err() {
        clear_active_prompt_state(inner);
        return Err(anyhow!("copilot runtime request channel closed"));
    }
    Ok(true)
}

fn is_runtime_request_channel_closed_error(err: &anyhow::Error) -> bool {
    err.to_string()
        .contains("copilot runtime request channel closed")
}

fn clear_active_prompt_state(inner: &Arc<CopilotAcpClientInner>) {
    if let Ok(mut active_prompt) = inner.active_prompt.lock() {
        *active_prompt = None;
    }
}

// ---------------------------------------------------------------------------
// Payload builders
// ---------------------------------------------------------------------------

/// Build the JSON-RPC `result` value for a `tool.call` response.
fn build_tool_call_result(response: CopilotToolCallResponse) -> Value {
    let inner = match response {
        CopilotToolCallResponse::Success(success) => json!({
            "textResultForLlm": success.text_result_for_llm,
            "resultType": "success",
            "toolTelemetry": {},
        }),
        CopilotToolCallResponse::Failure(failure) => json!({
            "textResultForLlm": failure.text_result_for_llm,
            "resultType": "failure",
            "error": failure.error,
            "toolTelemetry": {},
        }),
    };
    json!({ "result": inner })
}

fn unsupported_client_capability_message(method: &str) -> String {
    format!("VT Code's builtin Copilot client does not implement `{method}`.")
}

fn parse_observed_tool_call(update: &Value) -> Option<CopilotObservedToolCall> {
    let tool_call_id = update.get("toolCallId")?.as_str()?.to_string();
    let tool_name = update
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            update
                .get("kind")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|kind| format!("copilot_{kind}"))
        })
        .unwrap_or_else(|| "copilot_tool".to_string());
    let status = parse_observed_tool_status(
        update.get("status").and_then(Value::as_str),
        update.get("sessionUpdate").and_then(Value::as_str),
    );
    let arguments = update.get("rawInput").cloned();
    let output = update
        .get("rawOutput")
        .map(render_json_value)
        .or_else(|| extract_tool_call_content_text(update.get("content")));

    Some(CopilotObservedToolCall {
        tool_call_id,
        tool_name,
        status,
        arguments,
        output,
    })
}

fn parse_observed_tool_status(
    status: Option<&str>,
    session_update: Option<&str>,
) -> CopilotObservedToolCallStatus {
    match status.unwrap_or_else(|| {
        if session_update == Some("tool_call") {
            "pending"
        } else {
            "in_progress"
        }
    }) {
        "pending" => CopilotObservedToolCallStatus::Pending,
        "in_progress" => CopilotObservedToolCallStatus::InProgress,
        "completed" => CopilotObservedToolCallStatus::Completed,
        "failed" => CopilotObservedToolCallStatus::Failed,
        _ => CopilotObservedToolCallStatus::InProgress,
    }
}

fn extract_tool_call_content_text(content: Option<&Value>) -> Option<String> {
    content
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|item| {
            item.get("content").and_then(|content| {
                content
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|value| *value == "text")
                    .and_then(|_| content.get("text"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
        })
}

fn render_json_value(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn parse_legacy_permission_request(value: Value) -> Result<CopilotPermissionRequest> {
    let Some(object) = value.as_object() else {
        return Ok(CopilotPermissionRequest::Unknown {
            kind: Some("session/request_permission".to_string()),
            raw: value,
        });
    };

    let tool_call_id = object
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let tool_name = object
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            object
                .get("kind")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(|kind| format!("copilot_{kind}"))
        })
        .unwrap_or_else(|| "copilot_tool".to_string());

    Ok(CopilotPermissionRequest::CustomTool {
        tool_call_id,
        tool_name: tool_name.clone(),
        tool_description: object
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("GitHub Copilot ACP permission request")
            .to_string(),
        args: object.get("rawInput").cloned(),
    })
}

fn parse_permission_options(value: Option<&Value>) -> Vec<AcpPermissionOption> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(AcpPermissionOption {
                        option_id: item.get("optionId")?.as_str()?.to_string(),
                        kind: parse_permission_option_kind(
                            item.get("kind").and_then(Value::as_str),
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_permission_option_kind(kind: Option<&str>) -> AcpPermissionOptionKind {
    match kind {
        Some("allow_once") => AcpPermissionOptionKind::AllowOnce,
        Some("allow_always") => AcpPermissionOptionKind::AllowAlways,
        Some("reject_once") => AcpPermissionOptionKind::RejectOnce,
        Some("reject_always") => AcpPermissionOptionKind::RejectAlways,
        _ => AcpPermissionOptionKind::Other,
    }
}

fn legacy_permission_outcome(
    options: &[AcpPermissionOption],
    decision: &CopilotPermissionDecision,
) -> Value {
    let selected = match decision {
        CopilotPermissionDecision::Approved => pick_permission_option(
            options,
            &[
                AcpPermissionOptionKind::AllowOnce,
                AcpPermissionOptionKind::AllowAlways,
            ],
        ),
        CopilotPermissionDecision::ApprovedAlways => pick_permission_option(
            options,
            &[
                AcpPermissionOptionKind::AllowAlways,
                AcpPermissionOptionKind::AllowOnce,
            ],
        ),
        CopilotPermissionDecision::DeniedByRules
        | CopilotPermissionDecision::DeniedByContentExclusionPolicy { .. } => {
            pick_permission_option(
                options,
                &[
                    AcpPermissionOptionKind::RejectAlways,
                    AcpPermissionOptionKind::RejectOnce,
                ],
            )
        }
        CopilotPermissionDecision::DeniedNoApprovalRule
        | CopilotPermissionDecision::DeniedInteractivelyByUser { .. } => pick_permission_option(
            options,
            &[
                AcpPermissionOptionKind::RejectOnce,
                AcpPermissionOptionKind::RejectAlways,
            ],
        ),
    };

    if let Some(option_id) = selected {
        json!({
            "outcome": "selected",
            "optionId": option_id,
        })
    } else {
        json!({
            "outcome": "cancelled",
        })
    }
}

fn pick_permission_option(
    options: &[AcpPermissionOption],
    preferred_kinds: &[AcpPermissionOptionKind],
) -> Option<String> {
    preferred_kinds.iter().find_map(|preferred| {
        options
            .iter()
            .find(|option| option.kind == *preferred)
            .map(|option| option.option_id.clone())
    })
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

fn custom_tools_payload(custom_tools: &[ToolDefinition]) -> Vec<Value> {
    custom_tools
        .iter()
        .filter_map(|tool| {
            let function = tool.function.as_ref()?;
            Some(json!({
                "name": function.name,
                "description": function.description,
                "parameters": function.parameters,
                "skipPermission": true,
            }))
        })
        .collect()
}

fn parse_permission_request(value: Value) -> Result<CopilotPermissionRequest> {
    let Some(object) = value.as_object() else {
        return Ok(CopilotPermissionRequest::Unknown {
            kind: None,
            raw: value,
        });
    };

    let kind = object
        .get("kind")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let tool_call_id = object
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    Ok(match kind.as_deref() {
        Some("shell") => CopilotPermissionRequest::Shell {
            tool_call_id,
            full_command_text: object
                .get("fullCommandText")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            intention: object
                .get("intention")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            commands: object
                .get("commands")
                .and_then(Value::as_array)
                .map(|commands| {
                    commands
                        .iter()
                        .filter_map(|command| {
                            Some(CopilotShellCommandSummary {
                                identifier: command
                                    .get("identifier")
                                    .and_then(Value::as_str)?
                                    .to_string(),
                                read_only: command
                                    .get("readOnly")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            possible_paths: string_array(object.get("possiblePaths")),
            possible_urls: object
                .get("possibleUrls")
                .and_then(Value::as_array)
                .map(|urls| {
                    urls.iter()
                        .filter_map(|entry| {
                            entry
                                .get("url")
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            has_write_file_redirection: object
                .get("hasWriteFileRedirection")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            can_offer_session_approval: object
                .get("canOfferSessionApproval")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            warning: object
                .get("warning")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        },
        Some("write") => CopilotPermissionRequest::Write {
            tool_call_id,
            intention: object
                .get("intention")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            file_name: object
                .get("fileName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            diff: object
                .get("diff")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            new_file_contents: object
                .get("newFileContents")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        },
        Some("read") => CopilotPermissionRequest::Read {
            tool_call_id,
            intention: object
                .get("intention")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            path: object
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        Some("mcp") => CopilotPermissionRequest::Mcp {
            tool_call_id,
            server_name: object
                .get("serverName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tool_name: object
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tool_title: object
                .get("toolTitle")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            args: object.get("args").cloned(),
            read_only: object
                .get("readOnly")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        Some("url") => CopilotPermissionRequest::Url {
            tool_call_id,
            intention: object
                .get("intention")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            url: object
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        Some("memory") => CopilotPermissionRequest::Memory {
            tool_call_id,
            subject: object
                .get("subject")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            fact: object
                .get("fact")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            citations: object
                .get("citations")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        Some("custom-tool") => CopilotPermissionRequest::CustomTool {
            tool_call_id,
            tool_name: object
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tool_description: object
                .get("toolDescription")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            args: object.get("args").cloned(),
        },
        Some("hook") => CopilotPermissionRequest::Hook {
            tool_call_id,
            tool_name: object
                .get("toolName")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tool_args: object.get("toolArgs").cloned(),
            hook_message: object
                .get("hookMessage")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        },
        _ => CopilotPermissionRequest::Unknown { kind, raw: value },
    })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn request_id(message: &Value) -> Option<i64> {
    message.get("id").and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use vtcode_acp_client::StdioTransport;

    fn make_inner(write_tx: mpsc::UnboundedSender<String>) -> Arc<CopilotAcpClientInner> {
        Arc::new(CopilotAcpClientInner {
            transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
            active_prompt: StdMutex::new(None),
            session_id: StdMutex::new(None),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
        })
    }

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

        assert!(message.contains("does not implement"));
        assert!(message.contains("tool_call"));
    }

    #[test]
    fn permission_render_denies_without_prompt() {
        let result = PermissionResponseFormat::CopilotCli
            .render(CopilotPermissionDecision::DeniedNoApprovalRule);

        assert_eq!(
            result["result"]["kind"],
            "denied-no-approval-rule-and-could-not-request-from-user"
        );
    }

    #[test]
    fn legacy_permission_payload_selects_session_approval_option() {
        let outcome = legacy_permission_outcome(
            &[
                AcpPermissionOption {
                    option_id: "allow-once".to_string(),
                    kind: AcpPermissionOptionKind::AllowOnce,
                },
                AcpPermissionOption {
                    option_id: "allow-always".to_string(),
                    kind: AcpPermissionOptionKind::AllowAlways,
                },
            ],
            &CopilotPermissionDecision::ApprovedAlways,
        );

        assert_eq!(outcome["outcome"], "selected");
        assert_eq!(outcome["optionId"], "allow-always");
    }

    #[test]
    fn tool_call_result_returns_failure_structure() {
        let result =
            build_tool_call_result(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                text_result_for_llm: "failed".to_string(),
                error: "boom".to_string(),
            }));

        assert_eq!(result["result"]["resultType"], "failure");
        assert_eq!(result["result"]["error"], "boom");
    }

    #[test]
    fn parses_shell_permission_request() {
        let request = parse_permission_request(json!({
            "kind": "shell",
            "toolCallId": "call_1",
            "fullCommandText": "git status",
            "intention": "inspect repository state",
            "commands": [{ "identifier": "git", "readOnly": true }],
            "possiblePaths": ["./"],
            "possibleUrls": [{ "url": "https://github.com" }],
            "hasWriteFileRedirection": false,
            "canOfferSessionApproval": true
        }))
        .unwrap();

        match request {
            CopilotPermissionRequest::Shell {
                full_command_text,
                possible_paths,
                possible_urls,
                can_offer_session_approval,
                ..
            } => {
                assert_eq!(full_command_text, "git status");
                assert_eq!(possible_paths, vec!["./"]);
                assert_eq!(possible_urls, vec!["https://github.com"]);
                assert!(can_offer_session_approval);
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    #[test]
    fn custom_tools_payload_marks_skip_permission() {
        let payload = custom_tools_payload(&[ToolDefinition::function(
            "demo_tool".to_string(),
            "Run demo".to_string(),
            json!({"type": "object"}),
        )]);

        assert_eq!(payload.len(), 1);
        assert_eq!(payload[0]["name"], "demo_tool");
        assert_eq!(payload[0]["skipPermission"], true);
    }

    #[test]
    fn string_array_ignores_non_string_values() {
        let values = string_array(Some(&json!(["a", 1, "b"])));
        assert_eq!(values, vec!["a", "b"]);
    }

    #[test]
    fn parses_observed_tool_call_updates() {
        let observed = parse_observed_tool_call(&json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "call_9",
            "title": "Reading configuration file",
            "status": "completed",
            "rawInput": { "path": "vtcode.toml" },
            "content": [
                {
                    "type": "content",
                    "content": {
                        "type": "text",
                        "text": "Done"
                    }
                }
            ]
        }))
        .expect("observed tool call");

        assert_eq!(observed.tool_call_id, "call_9");
        assert_eq!(observed.tool_name, "Reading configuration file");
        assert_eq!(observed.status, CopilotObservedToolCallStatus::Completed);
        assert_eq!(observed.output.as_deref(), Some("Done"));
    }

    #[test]
    fn enqueue_runtime_request_clears_stale_active_prompt_when_receiver_is_gone() {
        let (write_tx, _write_rx) = mpsc::unbounded_channel();
        let (updates, updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
        let (runtime_requests, runtime_requests_rx) =
            mpsc::unbounded_channel::<CopilotRuntimeRequest>();
        drop(updates_rx);
        drop(runtime_requests_rx);

        let inner = Arc::new(CopilotAcpClientInner {
            transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
            active_prompt: StdMutex::new(Some(ActivePrompt {
                updates,
                runtime_requests,
            })),
            session_id: StdMutex::new(None),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
        });

        let err = enqueue_runtime_request(
            &inner,
            CopilotRuntimeRequest::CompatibilityNotice(CopilotCompatibilityNotice {
                state: CopilotAcpCompatibilityState::PromptOnly,
                message: "prompt-only degraded mode".to_string(),
            }),
        )
        .expect_err("closed runtime receiver should fail");

        assert!(
            err.to_string()
                .contains("copilot runtime request channel closed")
        );
        assert!(
            inner
                .active_prompt
                .lock()
                .expect("active_prompt lock")
                .is_none()
        );
    }

    #[test]
    fn handle_permission_request_falls_back_when_runtime_receiver_is_gone() {
        let (write_tx, mut write_rx) = mpsc::unbounded_channel();
        let (updates, _updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
        let (runtime_requests, runtime_requests_rx) =
            mpsc::unbounded_channel::<CopilotRuntimeRequest>();
        drop(runtime_requests_rx);

        let inner = Arc::new(CopilotAcpClientInner {
            transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
            active_prompt: StdMutex::new(Some(ActivePrompt {
                updates,
                runtime_requests,
            })),
            session_id: StdMutex::new(None),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
        });

        handle_permission_request(
            &inner,
            &json!({
                "jsonrpc": "2.0",
                "id": 9,
                "method": "permission.request",
                "params": {
                    "permissionRequest": {
                        "kind": "shell",
                        "fullCommandText": "git status",
                        "intention": "inspect repository state"
                    }
                }
            }),
        )
        .expect("stale runtime receiver should fall back cleanly");

        let payload = write_rx.try_recv().expect("fallback response payload");
        let payload: Value = serde_json::from_str(&payload).expect("valid json payload");
        assert_eq!(payload["jsonrpc"], "2.0");
        assert_eq!(payload["id"], 9);
        assert_eq!(
            payload["result"]["result"]["kind"],
            "denied-no-approval-rule-and-could-not-request-from-user"
        );
    }

    #[tokio::test]
    async fn prompt_session_cancel_handle_cancels_active_prompt_and_aborts_completion() {
        let (write_tx, mut write_rx) = mpsc::unbounded_channel();
        let (updates, _updates_rx) = mpsc::unbounded_channel::<PromptUpdate>();
        let (runtime_requests, _runtime_requests_rx) =
            mpsc::unbounded_channel::<CopilotRuntimeRequest>();

        let client = CopilotAcpClient {
            inner: Arc::new(CopilotAcpClientInner {
                transport: StdioTransport::new_for_testing(write_tx, Duration::from_secs(1)),
                active_prompt: StdMutex::new(Some(ActivePrompt {
                    updates,
                    runtime_requests,
                })),
                session_id: StdMutex::new(Some("session_123".to_string())),
                compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::FullTools),
            }),
        };

        let completion = tokio::spawn(async {
            std::future::pending::<()>().await;
            Ok::<PromptCompletion, anyhow::Error>(PromptCompletion {
                stop_reason: "cancelled".to_string(),
            })
        });
        let abort_handle = completion.abort_handle();

        let cancel_handle = PromptSessionCancelHandle {
            client: client.clone(),
            completion_abort: abort_handle,
        };

        cancel_handle.cancel();

        let payload = write_rx.recv().await.expect("session cancel payload");
        let payload: Value = serde_json::from_str(&payload).expect("valid cancel payload");
        assert_eq!(payload["method"], "session/cancel");
        assert_eq!(payload["params"]["sessionId"], "session_123");
        assert!(
            client
                .inner
                .active_prompt
                .lock()
                .expect("active_prompt lock")
                .is_none()
        );

        let err = completion.await.expect_err("completion should be aborted");
        assert!(err.is_cancelled(), "expected cancelled task, got {err}");
    }

    #[test]
    fn make_inner_helper_creates_valid_inner() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let inner = make_inner(tx);
        assert!(inner.active_prompt.lock().unwrap().is_none());
        assert!(inner.session_id.lock().unwrap().is_none());
        assert_eq!(
            *inner.compatibility_state.lock().unwrap(),
            CopilotAcpCompatibilityState::FullTools
        );
    }
}
