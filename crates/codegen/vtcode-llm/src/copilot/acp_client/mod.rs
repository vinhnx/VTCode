use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use tokio::time::timeout;
use vtcode_commons::sanitizer::sanitize_provider_diagnostic;
use vtcode_commons::serde_helpers::json_to_string_pretty;
use vtcode_config::auth::CopilotAuthConfig;

use super::command::{CopilotModelSelectionMode, resolve_copilot_command, spawn_copilot_acp_process};
use super::transport::StdioTransport;
use super::types::{
    CopilotAcpCompatibilityState, CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotPermissionDecision,
    CopilotPermissionRequest, CopilotShellCommandSummary, CopilotTerminalCreateRequest, CopilotTerminalCreateResponse,
    CopilotTerminalEnvVar, CopilotTerminalExitStatus, CopilotTerminalKillRequest, CopilotTerminalOutputRequest,
    CopilotTerminalOutputResponse, CopilotTerminalReleaseRequest, CopilotTerminalWaitForExitRequest,
    CopilotToolCallFailure, CopilotToolCallRequest, CopilotToolCallResponse,
};
use crate::provider::ToolDefinition;
use vtcode_config::constants::tools;

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
    updates: tokio::sync::mpsc::UnboundedReceiver<PromptUpdate>,
    runtime_requests: tokio::sync::mpsc::UnboundedReceiver<CopilotRuntimeRequest>,
    completion: tokio::task::JoinHandle<Result<PromptCompletion>>,
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
        (self.updates, self.runtime_requests, self.completion, self.cancel_handle)
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
            .map_err(|_e| anyhow!("copilot permission response channel closed"))
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
                self.response_tx.send(response).map_err(|_e| anyhow!($error_message))
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
                self.response_tx.send(()).map_err(|_e| anyhow!($error_message))
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
    pub(super) inner: Arc<CopilotAcpClientInner>,
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

pub(super) struct CopilotAcpClientInner {
    /// Generic JSON-RPC-over-stdio transport.
    pub(super) transport: StdioTransport,
    /// State for the currently active prompt session (if any).
    pub(super) active_prompt: StdMutex<Option<ActivePrompt>>,
    /// Copilot session identifier (set after session.create succeeds).
    pub(super) session_id: StdMutex<Option<String>>,
    /// Copilot ACP compatibility state (updated as messages arrive).
    pub(super) compatibility_state: StdMutex<CopilotAcpCompatibilityState>,
}

pub(super) struct ActivePrompt {
    pub(super) updates: tokio::sync::mpsc::UnboundedSender<PromptUpdate>,
    pub(super) runtime_requests: tokio::sync::mpsc::UnboundedSender<CopilotRuntimeRequest>,
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
        Self::Error { code: ACP_RUNTIME_UNAVAILABLE_CODE, message }
    }
}

mod client;
mod terminal;

use terminal::{
    handle_terminal_create_request, handle_terminal_kill_request, handle_terminal_output_request,
    handle_terminal_release_request, handle_terminal_wait_for_exit_request,
};

// ---------------------------------------------------------------------------
// CopilotAcpClient implementation
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// ACP message dispatch (Copilot-specific protocol)
// ---------------------------------------------------------------------------
// StdioTransport already handles JSON-RPC response routing (id → pending).
// This function receives only server-initiated requests and notifications.

pub(super) fn handle_acp_message(inner: &Arc<CopilotAcpClientInner>, message: Value) -> Result<()> {
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
                match enqueue_runtime_request(inner, CopilotRuntimeRequest::ObservedToolCall(tool_call)) {
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
        RpcReply::Result(value) => inner.transport.respond(id, value).map_err(anyhow::Error::from),
        RpcReply::Error { code, message } => {
            inner.transport.respond_error(id, code, message).map_err(anyhow::Error::from)
        }
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
            let safe_error = sanitize_provider_diagnostic(err.to_string().as_bytes());
            tracing::warn!(target: "copilot.acp", context = warn_context, error = %safe_error, "copilot acp response failed");
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

    spawn_runtime_response_task(inner.clone(), id, response_rx, build_success_reply, closed_reply, warn_context);
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
                .unwrap_or(CopilotPermissionRequest::Unknown { kind: None, raw: Value::Null }))
        },
        |request, response_tx| {
            CopilotRuntimeRequest::Permission(PendingPermissionRequest {
                request,
                response_tx,
                response_format: PermissionResponseFormat::CopilotCli,
            })
        },
        RpcReply::result,
        RpcReply::result(PermissionResponseFormat::CopilotCli.render(CopilotPermissionDecision::DeniedNoApprovalRule)),
        RpcReply::result(PermissionResponseFormat::CopilotCli.render(CopilotPermissionDecision::DeniedNoApprovalRule)),
        "permission.respond",
    )
}

fn handle_legacy_permission_request(inner: &Arc<CopilotAcpClientInner>, message: &Value) -> Result<()> {
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
        tool_call_id: params.get("toolCallId").and_then(Value::as_str).unwrap_or_default().to_string(),
        tool_name: params.get("toolName").and_then(Value::as_str).unwrap_or("unknown").to_string(),
        arguments: params.get("arguments").cloned().unwrap_or(Value::Null),
    };
    let tool_name = request.tool_name.clone();
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    dispatch_runtime_request(
        inner,
        CopilotRuntimeRequest::ToolCall(PendingToolCallRequest { request, response_tx }),
        response_rx,
        id,
        |response| RpcReply::result(build_tool_call_result(response)),
        tool_call_closed_reply(&tool_name),
        tool_call_unavailable_reply(&tool_name),
        "tool_call.respond",
    )
}

// ---------------------------------------------------------------------------
// Active prompt helpers
// ---------------------------------------------------------------------------

fn send_prompt_update(inner: &Arc<CopilotAcpClientInner>, update: PromptUpdate) -> Result<()> {
    // Clone the sender under the lock, then drop the guard before sending or
    // clearing. `clear_active_prompt_state` re-locks the same `std::sync::Mutex`,
    // and the let-chain guard stays alive through the whole block — calling
    // into it while the guard is held self-deadlocks on a `StdMutex`.
    let sender = inner
        .active_prompt
        .lock()
        .map_err(|_e| anyhow!("copilot acp active prompt mutex poisoned"))?
        .as_ref()
        .map(|active_prompt| active_prompt.updates.clone());
    let Some(sender) = sender else {
        return Ok(());
    };

    if sender.send(update).is_err() {
        clear_active_prompt_state(inner);
    }
    Ok(())
}

fn mark_prompt_degraded(inner: &Arc<CopilotAcpClientInner>, message: String) -> Result<()> {
    {
        let mut compatibility_state = inner
            .compatibility_state
            .lock()
            .map_err(|_e| anyhow!("copilot acp compatibility mutex poisoned"))?;
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

pub(super) fn enqueue_runtime_request(
    inner: &Arc<CopilotAcpClientInner>,
    request: CopilotRuntimeRequest,
) -> Result<bool> {
    let sender = inner
        .active_prompt
        .lock()
        .map_err(|_e| anyhow!("copilot acp active prompt mutex poisoned"))?
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
    err.to_string().contains("copilot runtime request channel closed")
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
    RpcReply::result(build_tool_call_result(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!("VT Code could not complete the client tool `{tool_name}`."),
        error: format!("tool '{tool_name}' response channel closed"),
    })))
}

fn tool_call_unavailable_reply(tool_name: &str) -> RpcReply {
    RpcReply::result(build_tool_call_result(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!("VT Code does not expose the client tool `{tool_name}` to GitHub Copilot."),
        error: format!("tool '{tool_name}' not supported by VT Code"),
    })))
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

mod observed;

use observed::parse_observed_tool_call;

fn parse_legacy_permission_request(value: Value) -> Result<CopilotPermissionRequest> {
    let Some(object) = value.as_object() else {
        return Ok(CopilotPermissionRequest::Unknown {
            kind: Some("session/request_permission".to_string()),
            raw: value,
        });
    };

    let tool_call_id = object.get("toolCallId").and_then(Value::as_str).map(ToString::to_string);
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
                        kind: parse_permission_option_kind(item.get("kind").and_then(Value::as_str)),
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

fn legacy_permission_outcome(options: &[AcpPermissionOption], decision: &CopilotPermissionDecision) -> Value {
    let selected = match decision {
        CopilotPermissionDecision::Approved => {
            pick_permission_option(options, &[AcpPermissionOptionKind::AllowOnce, AcpPermissionOptionKind::AllowAlways])
        }
        CopilotPermissionDecision::ApprovedAlways => {
            pick_permission_option(options, &[AcpPermissionOptionKind::AllowAlways, AcpPermissionOptionKind::AllowOnce])
        }
        CopilotPermissionDecision::DeniedByRules | CopilotPermissionDecision::DeniedByContentExclusionPolicy { .. } => {
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
                map.get("text").and_then(Value::as_str).map(ToString::to_string)
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
        return Ok(CopilotPermissionRequest::Unknown { kind: None, raw: value });
    };

    let kind = object.get("kind").and_then(Value::as_str).map(ToString::to_string);
    let tool_call_id = object.get("toolCallId").and_then(Value::as_str).map(ToString::to_string);

    Ok(match kind.as_deref() {
        Some(tools::SHELL) => CopilotPermissionRequest::Shell {
            tool_call_id,
            full_command_text: object
                .get("fullCommandText")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            intention: object.get("intention").and_then(Value::as_str).unwrap_or_default().to_string(),
            commands: object
                .get("commands")
                .and_then(Value::as_array)
                .map(|commands| {
                    commands
                        .iter()
                        .filter_map(|command| {
                            Some(CopilotShellCommandSummary {
                                identifier: command.get("identifier").and_then(Value::as_str)?.to_string(),
                                read_only: command.get("readOnly").and_then(Value::as_bool).unwrap_or(false),
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
                        .filter_map(|entry| entry.get("url").and_then(Value::as_str).map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            has_write_file_redirection: object.get("hasWriteFileRedirection").and_then(Value::as_bool).unwrap_or(false),
            can_offer_session_approval: object.get("canOfferSessionApproval").and_then(Value::as_bool).unwrap_or(false),
            warning: object.get("warning").and_then(Value::as_str).map(ToString::to_string),
        },
        Some("write") => CopilotPermissionRequest::Write {
            tool_call_id,
            intention: object.get("intention").and_then(Value::as_str).unwrap_or_default().to_string(),
            file_name: object.get("fileName").and_then(Value::as_str).unwrap_or_default().to_string(),
            diff: object.get("diff").and_then(Value::as_str).unwrap_or_default().to_string(),
            new_file_contents: object.get("newFileContents").and_then(Value::as_str).map(ToString::to_string),
        },
        Some("read") => CopilotPermissionRequest::Read {
            tool_call_id,
            intention: object.get("intention").and_then(Value::as_str).unwrap_or_default().to_string(),
            path: object.get("path").and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        Some("mcp") => CopilotPermissionRequest::Mcp {
            tool_call_id,
            server_name: object.get("serverName").and_then(Value::as_str).unwrap_or_default().to_string(),
            tool_name: object.get("toolName").and_then(Value::as_str).unwrap_or_default().to_string(),
            tool_title: object.get("toolTitle").and_then(Value::as_str).unwrap_or_default().to_string(),
            args: object.get("args").cloned(),
            read_only: object.get("readOnly").and_then(Value::as_bool).unwrap_or(false),
        },
        Some("url") => CopilotPermissionRequest::Url {
            tool_call_id,
            intention: object.get("intention").and_then(Value::as_str).unwrap_or_default().to_string(),
            url: object.get("url").and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        Some("memory") => CopilotPermissionRequest::Memory {
            tool_call_id,
            subject: object.get("subject").and_then(Value::as_str).unwrap_or_default().to_string(),
            fact: object.get("fact").and_then(Value::as_str).unwrap_or_default().to_string(),
            citations: object.get("citations").and_then(Value::as_str).unwrap_or_default().to_string(),
        },
        Some("custom-tool") => CopilotPermissionRequest::CustomTool {
            tool_call_id,
            tool_name: object.get("toolName").and_then(Value::as_str).unwrap_or_default().to_string(),
            tool_description: object
                .get("toolDescription")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            args: object.get("args").cloned(),
        },
        Some("hook") => CopilotPermissionRequest::Hook {
            tool_call_id,
            tool_name: object.get("toolName").and_then(Value::as_str).unwrap_or_default().to_string(),
            tool_args: object.get("toolArgs").cloned(),
            hook_message: object.get("hookMessage").and_then(Value::as_str).map(ToString::to_string),
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
