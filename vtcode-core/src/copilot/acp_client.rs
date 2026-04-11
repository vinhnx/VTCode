use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::time::timeout;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{
    CopilotModelSelectionMode, resolve_copilot_command, spawn_copilot_acp_process,
};
use super::transport::StdioTransport;
use super::types::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus,
    CopilotPermissionDecision, CopilotPermissionRequest, CopilotShellCommandSummary,
    CopilotTerminalCreateRequest, CopilotTerminalCreateResponse, CopilotTerminalEnvVar,
    CopilotTerminalExitStatus, CopilotTerminalKillRequest, CopilotTerminalOutputRequest,
    CopilotTerminalOutputResponse, CopilotTerminalReleaseRequest,
    CopilotTerminalWaitForExitRequest, CopilotToolCallFailure, CopilotToolCallRequest,
    CopilotToolCallResponse,
};
use crate::config::constants::tools;
use crate::llm::provider::ToolDefinition;

type RpcId = i64;

const ACP_METHOD_NOT_FOUND_CODE: i32 = -32601;
const ACP_RUNTIME_UNAVAILABLE_CODE: i32 = -32000;
const MAX_TERMINAL_OUTPUT_BYTE_LIMIT: usize = 1_048_576;
const MAX_TERMINAL_ARG_COUNT: usize = 256;
const MAX_TERMINAL_ENV_VAR_COUNT: usize = 128;

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
    TerminalCreate(PendingTerminalCreateRequest),
    TerminalOutput(PendingTerminalOutputRequest),
    TerminalRelease(PendingTerminalReleaseRequest),
    TerminalKill(PendingTerminalKillRequest),
    TerminalWaitForExit(PendingTerminalWaitForExitRequest),
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

macro_rules! define_pending_request {
    ($name:ident, $request_ty:ty, $response_ty:ty, $error_message:literal) => {
        #[derive(Debug)]
        pub struct $name {
            pub request: $request_ty,
            response_tx: tokio::sync::oneshot::Sender<$response_ty>,
        }

        impl $name {
            pub fn respond(self, response: $response_ty) -> Result<()> {
                self.response_tx
                    .send(response)
                    .map_err(|_| anyhow!($error_message))
            }
        }
    };
}

macro_rules! define_pending_signal_request {
    ($name:ident, $request_ty:ty, $error_message:literal) => {
        #[derive(Debug)]
        pub struct $name {
            pub request: $request_ty,
            response_tx: tokio::sync::oneshot::Sender<()>,
        }

        impl $name {
            pub fn respond(self) -> Result<()> {
                self.response_tx
                    .send(())
                    .map_err(|_| anyhow!($error_message))
            }
        }
    };
}

define_pending_request!(
    PendingToolCallRequest,
    CopilotToolCallRequest,
    CopilotToolCallResponse,
    "copilot tool response channel closed"
);
define_pending_request!(
    PendingTerminalCreateRequest,
    CopilotTerminalCreateRequest,
    CopilotTerminalCreateResponse,
    "copilot terminal create response channel closed"
);
define_pending_request!(
    PendingTerminalOutputRequest,
    CopilotTerminalOutputRequest,
    CopilotTerminalOutputResponse,
    "copilot terminal output response channel closed"
);
define_pending_signal_request!(
    PendingTerminalReleaseRequest,
    CopilotTerminalReleaseRequest,
    "copilot terminal release response channel closed"
);
define_pending_signal_request!(
    PendingTerminalKillRequest,
    CopilotTerminalKillRequest,
    "copilot terminal kill response channel closed"
);
define_pending_request!(
    PendingTerminalWaitForExitRequest,
    CopilotTerminalWaitForExitRequest,
    CopilotTerminalExitStatus,
    "copilot terminal wait response channel closed"
);

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

#[derive(Clone)]
enum RpcReply {
    Result(Value),
    Error { code: i32, message: &'static str },
}

impl RpcReply {
    fn result(value: Value) -> Self {
        Self::Result(value)
    }

    fn runtime_error(message: &'static str) -> Self {
        Self::Error {
            code: ACP_RUNTIME_UNAVAILABLE_CODE,
            message,
        }
    }
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
                        "terminal": true,
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
        "terminal/create" => handle_terminal_create_request(inner, &message)?,
        "terminal/output" => handle_terminal_output_request(inner, &message)?,
        "terminal/release" => handle_terminal_release_request(inner, &message)?,
        "terminal/kill" => handle_terminal_kill_request(inner, &message)?,
        "terminal/wait_for_exit" => handle_terminal_wait_for_exit_request(inner, &message)?,
        client_method => {
            if let Some(id) = request_id(&message) {
                let error_message = unsupported_client_capability_message(client_method);
                mark_prompt_degraded(inner, error_message.clone())?;
                inner
                    .transport
                    .respond_error(id, ACP_METHOD_NOT_FOUND_CODE, error_message)
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

fn send_rpc_reply(inner: &CopilotAcpClientInner, id: RpcId, reply: RpcReply) -> Result<()> {
    match reply {
        RpcReply::Result(value) => inner
            .transport
            .respond(id, value)
            .map_err(anyhow::Error::from),
        RpcReply::Error { code, message } => inner
            .transport
            .respond_error(id, code, message)
            .map_err(anyhow::Error::from),
    }
}

fn spawn_runtime_response_task<TResponse, F>(
    inner: Arc<CopilotAcpClientInner>,
    id: RpcId,
    response_rx: tokio::sync::oneshot::Receiver<TResponse>,
    build_success_reply: F,
    closed_reply: RpcReply,
    warn_context: &'static str,
) where
    TResponse: Send + 'static,
    F: FnOnce(TResponse) -> RpcReply + Send + 'static,
{
    tokio::spawn(async move {
        let reply = match response_rx.await {
            Ok(response) => build_success_reply(response),
            Err(_) => closed_reply,
        };

        if let Err(err) = send_rpc_reply(inner.as_ref(), id, reply) {
            tracing::warn!(target: "copilot.acp", context = warn_context, error = %err, "copilot acp response failed");
        }
    });
}

fn dispatch_runtime_request<TResponse, F>(
    inner: &Arc<CopilotAcpClientInner>,
    request: CopilotRuntimeRequest,
    response_rx: tokio::sync::oneshot::Receiver<TResponse>,
    id: RpcId,
    build_success_reply: F,
    closed_reply: RpcReply,
    unavailable_reply: RpcReply,
    warn_context: &'static str,
) -> Result<()>
where
    TResponse: Send + 'static,
    F: FnOnce(TResponse) -> RpcReply + Send + 'static,
{
    let dispatched = match enqueue_runtime_request(inner, request) {
        Ok(dispatched) => dispatched,
        Err(err) if is_runtime_request_channel_closed_error(&err) => false,
        Err(err) => return Err(err),
    };

    if !dispatched {
        return send_rpc_reply(inner.as_ref(), id, unavailable_reply);
    }

    spawn_runtime_response_task(
        inner.clone(),
        id,
        response_rx,
        build_success_reply,
        closed_reply,
        warn_context,
    );
    Ok(())
}

fn handle_runtime_request_message<TRequest, TResponse, Parse, Wrap, Build>(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
    parse_request: Parse,
    wrap_request: Wrap,
    build_success_reply: Build,
    closed_reply: RpcReply,
    unavailable_reply: RpcReply,
    warn_context: &'static str,
) -> Result<()>
where
    TResponse: Send + 'static,
    Parse: FnOnce(&Value) -> Result<TRequest>,
    Wrap: FnOnce(TRequest, tokio::sync::oneshot::Sender<TResponse>) -> CopilotRuntimeRequest,
    Build: FnOnce(TResponse) -> RpcReply + Send + 'static,
{
    let Some(id) = request_id(message) else {
        return Ok(());
    };

    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let request = parse_request(&params)?;
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    dispatch_runtime_request(
        inner,
        wrap_request(request, response_tx),
        response_rx,
        id,
        build_success_reply,
        closed_reply,
        unavailable_reply,
        warn_context,
    )
}

fn handle_permission_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            Ok(params
                .get("permissionRequest")
                .cloned()
                .map(parse_permission_request)
                .transpose()?
                .unwrap_or(CopilotPermissionRequest::Unknown {
                    kind: None,
                    raw: Value::Null,
                }))
        },
        |request, response_tx| {
            CopilotRuntimeRequest::Permission(PendingPermissionRequest {
                request,
                response_tx,
                response_format: PermissionResponseFormat::CopilotCli,
            })
        },
        RpcReply::result,
        RpcReply::result(
            PermissionResponseFormat::CopilotCli
                .render(CopilotPermissionDecision::DeniedNoApprovalRule),
        ),
        RpcReply::result(
            PermissionResponseFormat::CopilotCli
                .render(CopilotPermissionDecision::DeniedNoApprovalRule),
        ),
        "permission.respond",
    )
}

fn handle_legacy_permission_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            let request = params
                .get("toolCall")
                .cloned()
                .map(parse_legacy_permission_request)
                .transpose()?
                .unwrap_or(CopilotPermissionRequest::Unknown {
                    kind: Some("session/request_permission".to_string()),
                    raw: Value::Null,
                });
            Ok((request, parse_permission_options(params.get("options"))))
        },
        |(request, options), response_tx| {
            CopilotRuntimeRequest::Permission(PendingPermissionRequest {
                request,
                response_tx,
                response_format: PermissionResponseFormat::AcpLegacy { options },
            })
        },
        RpcReply::result,
        RpcReply::result(json!({ "outcome": { "outcome": "cancelled" } })),
        RpcReply::result(json!({ "outcome": { "outcome": "cancelled" } })),
        "legacy_permission.respond",
    )
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
    let tool_name = request.tool_name.clone();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    dispatch_runtime_request(
        inner,
        CopilotRuntimeRequest::ToolCall(PendingToolCallRequest {
            request,
            response_tx,
        }),
        response_rx,
        id,
        |response| RpcReply::result(build_tool_call_result(response)),
        tool_call_closed_reply(&tool_name),
        tool_call_unavailable_reply(&tool_name),
        "tool_call.respond",
    )
}

fn handle_terminal_create_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        parse_terminal_create_request,
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalCreate(PendingTerminalCreateRequest {
                request,
                response_tx,
            })
        },
        |response| RpcReply::result(build_terminal_create_result(response)),
        RpcReply::runtime_error("VT Code could not create the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not create the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_create.respond",
    )
}

fn handle_terminal_output_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| {
                CopilotTerminalOutputRequest {
                    session_id,
                    terminal_id,
                }
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalOutput(PendingTerminalOutputRequest {
                request,
                response_tx,
            })
        },
        |response| RpcReply::result(build_terminal_output_result(response)),
        RpcReply::runtime_error("VT Code could not read the requested terminal output."),
        RpcReply::runtime_error(
            "VT Code could not read the requested terminal output because the Copilot runtime is unavailable.",
        ),
        "terminal_output.respond",
    )
}

fn handle_terminal_release_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| {
                CopilotTerminalReleaseRequest {
                    session_id,
                    terminal_id,
                }
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalRelease(PendingTerminalReleaseRequest {
                request,
                response_tx,
            })
        },
        |_| RpcReply::result(json!({})),
        RpcReply::runtime_error("VT Code could not release the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not release the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_release.respond",
    )
}

fn handle_terminal_kill_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| {
                CopilotTerminalKillRequest {
                    session_id,
                    terminal_id,
                }
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalKill(PendingTerminalKillRequest {
                request,
                response_tx,
            })
        },
        |_| RpcReply::result(json!({})),
        RpcReply::runtime_error("VT Code could not kill the requested terminal command."),
        RpcReply::runtime_error(
            "VT Code could not kill the requested terminal command because the Copilot runtime is unavailable.",
        ),
        "terminal_kill.respond",
    )
}

fn handle_terminal_wait_for_exit_request(
    inner: &Arc<CopilotAcpClientInner>,
    message: &Value,
) -> Result<()> {
    handle_runtime_request_message(
        inner,
        message,
        |params| {
            parse_terminal_request(params, |session_id, terminal_id| {
                CopilotTerminalWaitForExitRequest {
                    session_id,
                    terminal_id,
                }
            })
        },
        |request, response_tx| {
            CopilotRuntimeRequest::TerminalWaitForExit(PendingTerminalWaitForExitRequest {
                request,
                response_tx,
            })
        },
        |response| RpcReply::result(build_terminal_wait_for_exit_result(response)),
        RpcReply::runtime_error("VT Code could not wait for the requested terminal."),
        RpcReply::runtime_error(
            "VT Code could not wait for the requested terminal because the Copilot runtime is unavailable.",
        ),
        "terminal_wait_for_exit.respond",
    )
}

fn parse_terminal_create_request(params: &Value) -> Result<CopilotTerminalCreateRequest> {
    let session_id = optional_session_id(params);
    let command =
        required_non_empty_string(params, "command", "copilot terminal/create missing command")?;
    let args = parse_string_array(
        params.get("args"),
        MAX_TERMINAL_ARG_COUNT,
        "copilot terminal args must be strings",
        "copilot terminal/create has too many args",
    )?;
    let env = parse_terminal_env_vars(params.get("env"))?;
    let cwd = params.get("cwd").and_then(Value::as_str).map(PathBuf::from);
    let output_byte_limit = params
        .get("outputByteLimit")
        .and_then(Value::as_u64)
        .map(|value| {
            usize::try_from(value)
                .unwrap_or(MAX_TERMINAL_OUTPUT_BYTE_LIMIT)
                .min(MAX_TERMINAL_OUTPUT_BYTE_LIMIT)
        });

    Ok(CopilotTerminalCreateRequest {
        session_id,
        command,
        args,
        env,
        cwd,
        output_byte_limit,
    })
}

fn parse_terminal_request<T>(params: &Value, build: impl FnOnce(String, String) -> T) -> Result<T> {
    let session_id = optional_session_id(params);
    let terminal_id = required_non_empty_string(
        params,
        "terminalId",
        "copilot terminal request missing terminalId",
    )?;
    Ok(build(session_id, terminal_id))
}

fn optional_session_id(params: &Value) -> String {
    params
        .get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn required_non_empty_string(
    params: &Value,
    key: &str,
    error_message: &'static str,
) -> Result<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!(error_message))
}

fn parse_string_array(
    value: Option<&Value>,
    max_items: usize,
    item_error: &'static str,
    limit_error: &'static str,
) -> Result<Vec<String>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    if values.len() > max_items {
        anyhow::bail!(limit_error);
    }

    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| anyhow!(item_error))
        })
        .collect()
}

fn parse_terminal_env_vars(value: Option<&Value>) -> Result<Vec<CopilotTerminalEnvVar>> {
    let Some(values) = value.and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    if values.len() > MAX_TERMINAL_ENV_VAR_COUNT {
        anyhow::bail!("copilot terminal/create has too many env entries");
    }

    values
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .ok_or_else(|| anyhow!("copilot terminal env entries must be objects"))?;
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| anyhow!("copilot terminal env entries require a name"))?;
            let value = object
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| anyhow!("copilot terminal env entries require a value"))?;
            Ok(CopilotTerminalEnvVar { name, value })
        })
        .collect()
}

fn build_terminal_create_result(response: CopilotTerminalCreateResponse) -> Value {
    json!({
        "terminalId": response.terminal_id,
    })
}

fn build_terminal_output_result(response: CopilotTerminalOutputResponse) -> Value {
    let exit_status = response.exit_status.map(build_terminal_exit_status_json);
    let mut result = serde_json::Map::from_iter([
        ("output".to_string(), Value::String(response.output)),
        ("truncated".to_string(), Value::Bool(response.truncated)),
    ]);
    if let Some(exit_status) = exit_status {
        result.insert("exitStatus".to_string(), exit_status);
    }
    Value::Object(result)
}

fn build_terminal_wait_for_exit_result(response: CopilotTerminalExitStatus) -> Value {
    build_terminal_exit_status_json(response)
}

fn build_terminal_exit_status_json(status: CopilotTerminalExitStatus) -> Value {
    let mut result = serde_json::Map::new();
    result.insert(
        "exitCode".to_string(),
        status
            .exit_code
            .map_or(Value::Null, |value| Value::from(u64::from(value))),
    );
    result.insert(
        "signal".to_string(),
        status.signal.map_or(Value::Null, Value::String),
    );
    Value::Object(result)
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

fn tool_call_closed_reply(tool_name: &str) -> RpcReply {
    RpcReply::result(build_tool_call_result(CopilotToolCallResponse::Failure(
        CopilotToolCallFailure {
            text_result_for_llm: format!(
                "VT Code could not complete the client tool `{tool_name}`."
            ),
            error: format!("tool '{tool_name}' response channel closed"),
        },
    )))
}

fn tool_call_unavailable_reply(tool_name: &str) -> RpcReply {
    RpcReply::result(build_tool_call_result(CopilotToolCallResponse::Failure(
        CopilotToolCallFailure {
            text_result_for_llm: format!(
                "VT Code does not expose the client tool `{tool_name}` to GitHub Copilot."
            ),
            error: format!("tool '{tool_name}' not supported by VT Code"),
        },
    )))
}

fn derived_copilot_tool_name(title: Option<&str>, kind: Option<&str>) -> String {
    title
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            kind.filter(|value| !value.trim().is_empty())
                .map(|kind| format!("copilot_{kind}"))
        })
        .unwrap_or_else(|| "copilot_tool".to_string())
}

fn parse_observed_tool_call(update: &Value) -> Option<CopilotObservedToolCall> {
    let tool_call_id = update.get("toolCallId")?.as_str()?.to_string();
    let tool_name = derived_copilot_tool_name(
        update.get("title").and_then(Value::as_str),
        update.get("kind").and_then(Value::as_str),
    );
    let status = parse_observed_tool_status(
        update.get("status").and_then(Value::as_str),
        update.get("sessionUpdate").and_then(Value::as_str),
    );
    let arguments = update.get("rawInput").cloned();
    let output = extract_observed_tool_output(update);
    let terminal_id = extract_tool_call_terminal_id(update.get("content"));

    Some(CopilotObservedToolCall {
        tool_call_id,
        tool_name,
        status,
        arguments,
        output,
        terminal_id,
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

fn extract_observed_tool_output(update: &Value) -> Option<String> {
    update
        .get("rawOutput")
        .and_then(extract_observed_tool_raw_output)
        .or_else(|| extract_tool_call_content_text(update.get("content")))
}

fn extract_observed_tool_raw_output(raw_output: &Value) -> Option<String> {
    match raw_output {
        Value::String(text) => Some(text.clone()).filter(|text| !text.trim().is_empty()),
        Value::Object(object) => object
            .get("content")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                object
                    .get("detailedContent")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .map(ToString::to_string)
            })
            .or_else(|| Some(render_json_value(raw_output))),
        _ => Some(render_json_value(raw_output)),
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

fn extract_tool_call_terminal_id(content: Option<&Value>) -> Option<String> {
    content
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|item| {
            item.get("content").and_then(|content| {
                content
                    .get("type")
                    .and_then(Value::as_str)
                    .filter(|value| *value == "terminal")
                    .and_then(|_| content.get("terminalId"))
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
    let tool_name = derived_copilot_tool_name(
        object.get("title").and_then(Value::as_str),
        object.get("kind").and_then(Value::as_str),
    );

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
        Some(tools::SHELL) => CopilotPermissionRequest::Shell {
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
mod tests;
