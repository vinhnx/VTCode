use anyhow::{Context, Result, anyhow, bail};
use futures::{TryFutureExt, future::BoxFuture};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::broadcast;
use vtcode_acp::{AcpError, StdioTransport, StdioTransportOptions};
use vtcode_config::{AgentCodexAppServerConfig, VTCodeConfig};

pub(crate) const CODEX_PROVIDER: &str = "codex";
const STDIO_LISTEN_TARGET: &str = "stdio://";
const DEFAULT_RPC_TIMEOUT_SECS: u64 = 30;
const SERVER_ERROR_CODE: i32 = -32000;
const SERVER_OVERLOADED_ERROR_CODE: i32 = -32001;
const CODEX_SIDECAR_UNAVAILABLE_PREFIX: &str = "Codex app-server sidecar is unavailable.";
const IDEMPOTENT_REQUEST_RETRY_LIMIT: usize = 3;

type RefreshHandler = Arc<
    dyn Fn(ChatGptAuthTokensRefreshParams) -> BoxFuture<'static, Result<ChatGptAuthTokens>>
        + Send
        + Sync,
>;

#[derive(Debug, Clone)]
pub(crate) struct ServerEvent {
    pub(crate) method: String,
    pub(crate) params: Value,
    pub(crate) id: Option<Value>,
}

pub(crate) struct CodexAppServerClient {
    transport: Arc<StdioTransport>,
    events: broadcast::Sender<ServerEvent>,
    refresh_handler: Arc<Mutex<Option<RefreshHandler>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestRetryPolicy {
    Never,
    Idempotent,
}

impl CodexAppServerClient {
    pub(crate) async fn connect(vt_cfg: Option<&VTCodeConfig>) -> Result<Self> {
        let sidecar_cfg = sidecar_config(vt_cfg);
        ensure_codex_sidecar_available(vt_cfg)?;
        let mut command = Command::new(&sidecar_cfg.command);
        command
            .args(&sidecar_cfg.args)
            .arg("--listen")
            .arg(STDIO_LISTEN_TARGET)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                codex_cli_unavailable_error()
            } else {
                anyhow!(
                    "failed to launch Codex app-server via '{}': {}",
                    sidecar_cfg.command,
                    err
                )
            }
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("codex app-server stdin was not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("codex app-server stdout was not piped"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("codex app-server stderr was not piped"))?;
        let transport = Arc::new(StdioTransport::from_child_with_options(
            child,
            stdin,
            stdout,
            stderr,
            Duration::from_secs(DEFAULT_RPC_TIMEOUT_SECS),
            StdioTransportOptions {
                include_jsonrpc_version: false,
            },
        ));
        let refresh_handler = Arc::new(Mutex::new(None));
        let (event_tx, _) = broadcast::channel(256);
        install_event_handler(&transport, event_tx.clone(), Arc::clone(&refresh_handler));

        initialize_connection(&transport, &sidecar_cfg).await?;

        let client = Self {
            transport,
            events: event_tx,
            refresh_handler,
        };
        client.set_refresh_handler(None);
        Ok(client)
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.events.subscribe()
    }

    pub(crate) fn set_refresh_handler(&self, handler: Option<RefreshHandler>) {
        if let Ok(mut refresh_handler) = self.refresh_handler.lock() {
            *refresh_handler = handler;
        }
    }

    pub(crate) fn account_read(
        &self,
    ) -> impl Future<Output = Result<CodexAccountReadResponse>> + '_ {
        self.request_idempotent("account/read", json!({}))
    }

    pub(crate) fn account_login_chatgpt(
        &self,
    ) -> impl Future<Output = Result<CodexLoginAccountResponse>> + '_ {
        self.request("account/login/start", json!({ "type": "chatgpt" }))
    }

    pub(crate) fn account_login_chatgpt_device_code(
        &self,
    ) -> impl Future<Output = Result<CodexLoginAccountResponse>> + '_ {
        self.request(
            "account/login/start",
            json!({ "type": "chatgptDeviceCode" }),
        )
    }

    pub(crate) fn account_logout(&self) -> impl Future<Output = Result<()>> + '_ {
        self.request("account/logout", json!({}))
            .map_ok(|_: CodexLogoutAccountResponse| ())
    }

    pub(crate) fn mcp_server_status_list(
        &self,
    ) -> impl Future<Output = Result<CodexMcpServerStatusListResponse>> + '_ {
        self.request_idempotent("mcpServerStatus/list", mcp_server_status_list_params())
    }

    pub(crate) fn collaboration_mode_list(
        &self,
    ) -> impl Future<Output = Result<CodexCollaborationModeListResponse>> + '_ {
        self.request_idempotent("collaborationMode/list", json!({}))
    }

    pub(crate) fn thread_start(
        &self,
        params: CodexThreadRequest,
        ephemeral: bool,
    ) -> impl Future<Output = Result<CodexThreadEnvelope>> + '_ {
        self.request("thread/start", params.thread_start_params(ephemeral))
    }

    pub(crate) fn thread_resume(
        &self,
        thread_id: &str,
    ) -> impl Future<Output = Result<CodexThreadEnvelope>> + '_ {
        self.request("thread/resume", json!({ "threadId": thread_id }))
    }

    pub(crate) fn thread_fork(
        &self,
        thread_id: &str,
        params: CodexThreadRequest,
        ephemeral: bool,
    ) -> impl Future<Output = Result<CodexThreadEnvelope>> + '_ {
        let mut request = params.thread_start_params(ephemeral);
        if let Some(object) = request.as_object_mut() {
            object.insert("threadId".to_string(), Value::String(thread_id.to_string()));
        }
        self.request("thread/fork", request)
    }

    pub(crate) fn turn_start(
        &self,
        params: CodexTurnRequest,
    ) -> impl Future<Output = Result<CodexTurnStartResponse>> + '_ {
        self.request("turn/start", params.as_json())
    }

    pub(crate) fn turn_interrupt(
        &self,
        thread_id: &str,
        turn_id: &str,
    ) -> impl Future<Output = Result<()>> + '_ {
        self.request(
            "turn/interrupt",
            json!({
                "threadId": thread_id,
                "turnId": turn_id,
            }),
        )
        .map_ok(|_: CodexEmptyResponse| ())
    }

    pub(crate) fn turn_steer(
        &self,
        thread_id: &str,
        turn_id: &str,
        input: String,
    ) -> impl Future<Output = Result<CodexTurnSteerResponse>> + '_ {
        self.request(
            "turn/steer",
            json!({
                "expectedTurnId": turn_id,
                "input": [{
                    "type": "text",
                    "text": input,
                }],
                "threadId": thread_id,
            }),
        )
    }

    pub(crate) fn review_start(
        &self,
        params: CodexReviewStartRequest,
    ) -> impl Future<Output = Result<CodexReviewStartResponse>> + '_ {
        self.request("review/start", params.as_json())
    }

    #[allow(dead_code)]
    pub(crate) fn command_exec(
        &self,
        params: CodexCommandExecRequest,
    ) -> impl Future<Output = Result<CodexCommandExecResponse>> + '_ {
        self.request("command/exec", params.as_json())
    }

    pub(crate) fn respond_to_server_request(&self, id: Value, result: Value) -> Result<()> {
        self.transport
            .respond_value(id, result)
            .map_err(|err| anyhow!(err.to_string()))
    }

    fn request<'a, T>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> impl Future<Output = Result<T>> + 'a
    where
        T: DeserializeOwned + 'a,
    {
        self.request_with_policy(method, params, RequestRetryPolicy::Never)
    }

    fn request_idempotent<'a, T>(
        &'a self,
        method: &'a str,
        params: Value,
    ) -> impl Future<Output = Result<T>> + 'a
    where
        T: DeserializeOwned + 'a,
    {
        self.request_with_policy(method, params, RequestRetryPolicy::Idempotent)
    }

    async fn request_with_policy<T>(
        &self,
        method: &str,
        params: Value,
        retry_policy: RequestRetryPolicy,
    ) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let mut attempts = 0usize;
        loop {
            match self.transport.call(method, params.clone()).await {
                Ok(response) => {
                    return serde_json::from_value(response).with_context(|| {
                        format!("failed to decode codex app-server response for {method}")
                    });
                }
                Err(err) if is_server_overloaded_error(&err) => {
                    attempts += 1;
                    if retry_policy == RequestRetryPolicy::Idempotent
                        && attempts < IDEMPOTENT_REQUEST_RETRY_LIMIT
                    {
                        tokio::time::sleep(idempotent_retry_delay(attempts)).await;
                        continue;
                    }
                    return Err(overloaded_request_error(method, retry_policy))
                        .with_context(|| format!("codex app-server request failed: {method}"));
                }
                Err(err) => {
                    return Err(anyhow!(err.to_string()))
                        .with_context(|| format!("codex app-server request failed: {method}"));
                }
            }
        }
    }
}

pub(crate) async fn launch_app_server_proxy(
    vt_cfg: Option<&VTCodeConfig>,
    listen: &str,
) -> Result<()> {
    let listen = validate_listen_target(listen)?;
    ensure_codex_sidecar_available(vt_cfg)?;

    let sidecar_cfg = sidecar_config(vt_cfg);
    let status = Command::new(&sidecar_cfg.command)
        .args(&sidecar_cfg.args)
        .arg("--listen")
        .arg(listen)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                codex_cli_unavailable_error()
            } else {
                anyhow!(
                    "failed to launch Codex app-server via '{}': {}",
                    sidecar_cfg.command,
                    err
                )
            }
        })?;

    if !status.success() {
        bail!("Codex app-server exited with status {status}");
    }

    Ok(())
}

fn install_event_handler(
    transport: &Arc<StdioTransport>,
    event_tx: broadcast::Sender<ServerEvent>,
    refresh_handler: Arc<Mutex<Option<RefreshHandler>>>,
) {
    let notification_transport = Arc::clone(transport);
    transport.set_notification_handler(Arc::new(move |message| {
        let method = message.get("method").and_then(Value::as_str);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = message.get("id").cloned();

        if let Some(method) = method {
            if method == "account/chatgptAuthTokens/refresh"
                && let Some(id) = id.clone()
            {
                let transport = Arc::clone(&notification_transport);
                let refresh_handler = Arc::clone(&refresh_handler);
                let params = params.clone();
                tokio::spawn(async move {
                    handle_refresh_request(transport, refresh_handler, id, params).await;
                });
            }

            let _ = event_tx.send(ServerEvent {
                method: method.to_string(),
                params,
                id,
            });
        }

        Ok(())
    }));
}

async fn initialize_connection(
    transport: &Arc<StdioTransport>,
    sidecar_cfg: &AgentCodexAppServerConfig,
) -> Result<()> {
    tokio::time::timeout(
        Duration::from_secs(sidecar_cfg.startup_timeout_secs),
        transport.call(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "vtcode",
                    "title": "VT Code",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "experimentalApi": true,
                },
            }),
        ),
    )
    .await
    .with_context(|| {
        format!(
            "failed to initialize Codex app-server within {}s",
            sidecar_cfg.startup_timeout_secs
        )
    })?
    .map_err(|err| anyhow!(err.to_string()))?;
    transport
        .notify("initialized", json!({}))
        .map_err(|err| anyhow!(err.to_string()))
}

async fn handle_refresh_request(
    transport: Arc<StdioTransport>,
    refresh_handler: Arc<Mutex<Option<RefreshHandler>>>,
    id: Value,
    params: Value,
) {
    let refresh_handler = refresh_handler
        .lock()
        .ok()
        .and_then(|handler| handler.clone());
    let Some(refresh_handler) = refresh_handler else {
        let _ = transport.respond_error_value(
            id,
            SERVER_ERROR_CODE,
            "VT Code does not have a ChatGPT token refresh handler configured",
        );
        return;
    };

    let params: ChatGptAuthTokensRefreshParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(err) => {
            let _ = transport.respond_error_value(
                id,
                SERVER_ERROR_CODE,
                format!("invalid ChatGPT refresh request: {err}"),
            );
            return;
        }
    };

    match refresh_handler(params).await {
        Ok(tokens) => {
            let result = match serde_json::to_value(tokens) {
                Ok(value) => value,
                Err(err) => {
                    let _ = transport.respond_error_value(
                        id,
                        SERVER_ERROR_CODE,
                        format!("failed to encode ChatGPT refresh response: {err}"),
                    );
                    return;
                }
            };
            let _ = transport.respond_value(id, result);
        }
        Err(err) => {
            let _ = transport.respond_error_value(id, SERVER_ERROR_CODE, err.to_string());
        }
    }
}

fn sidecar_config(vt_cfg: Option<&VTCodeConfig>) -> AgentCodexAppServerConfig {
    vt_cfg
        .map(|cfg| cfg.agent.codex_app_server.clone())
        .unwrap_or_default()
}

pub(crate) fn ensure_codex_sidecar_available(vt_cfg: Option<&VTCodeConfig>) -> Result<()> {
    let sidecar_cfg = sidecar_config(vt_cfg);
    ensure_codex_sidecar_command_available(&sidecar_cfg.command)
}

pub(crate) fn codex_sidecar_requirement_note() -> &'static str {
    "The default `[agent.codex_app_server].command = \"codex\"` requires the `codex` CLI to be installed and available on `$PATH`. Install `codex` and ensure it is on `$PATH`, or set `[agent.codex_app_server].command` to a custom executable path."
}

fn codex_cli_unavailable_error() -> anyhow::Error {
    anyhow!(
        "{} {}",
        CODEX_SIDECAR_UNAVAILABLE_PREFIX,
        codex_sidecar_requirement_note()
    )
}

fn ensure_codex_sidecar_command_available(command: &str) -> Result<()> {
    let command = command.trim();
    if command.is_empty() {
        bail!(
            "{} `[agent.codex_app_server].command` is empty. {}",
            CODEX_SIDECAR_UNAVAILABLE_PREFIX,
            codex_sidecar_requirement_note()
        );
    }

    if resolve_sidecar_command_path(command).is_some() {
        return Ok(());
    }

    if is_path_like_command(command) {
        bail!(
            "{} Configured `[agent.codex_app_server].command = \"{}\"` was not found or is not executable. {}",
            CODEX_SIDECAR_UNAVAILABLE_PREFIX,
            command,
            codex_sidecar_requirement_note()
        );
    }

    Err(codex_cli_unavailable_error())
}

fn resolve_sidecar_command_path(command: &str) -> Option<PathBuf> {
    resolve_sidecar_command_path_with_path(command, std::env::var_os("PATH").as_deref())
}

fn resolve_sidecar_command_path_with_path(
    command: &str,
    path_env: Option<&OsStr>,
) -> Option<PathBuf> {
    if is_path_like_command(command) {
        let path = PathBuf::from(command);
        return candidate_command_paths(&path)
            .into_iter()
            .find(|candidate| path_is_launchable(candidate));
    }

    path_env
        .into_iter()
        .flat_map(std::env::split_paths)
        .flat_map(|dir| candidate_command_paths(&dir.join(command)))
        .find(|candidate| path_is_launchable(candidate))
}

fn is_path_like_command(command: &str) -> bool {
    let path = Path::new(command);
    path.is_absolute()
        || command.contains(std::path::MAIN_SEPARATOR)
        || command.contains('/')
        || command.contains('\\')
}

fn candidate_command_paths(base: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        if base.extension().is_some() {
            return vec![base.to_path_buf()];
        }

        let mut candidates = vec![base.to_path_buf()];
        if let Some(path_ext) = std::env::var_os("PATHEXT") {
            for ext in std::env::split_paths(&path_ext)
                .filter_map(|path| path.into_os_string().into_string().ok())
            {
                let trimmed = ext.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let suffix = trimmed.trim_start_matches('.');
                candidates.push(base.with_extension(suffix));
            }
        }
        candidates
    }
    #[cfg(not(windows))]
    {
        vec![base.to_path_buf()]
    }
}

fn path_is_launchable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn validate_listen_target(listen: &str) -> Result<&str> {
    let listen = listen.trim();
    if listen.is_empty() {
        bail!("app-server listen target cannot be empty");
    }
    Ok(listen)
}

fn is_server_overloaded_error(err: &AcpError) -> bool {
    matches!(
        err,
        AcpError::RemoteError {
            code: Some(SERVER_OVERLOADED_ERROR_CODE),
            ..
        }
    )
}

fn idempotent_retry_delay(attempt: usize) -> Duration {
    Duration::from_millis((attempt as u64) * 200)
}

fn mcp_server_status_list_params() -> Value {
    json!({
        "detail": "toolsAndAuthOnly"
    })
}

fn overloaded_request_error(method: &str, retry_policy: RequestRetryPolicy) -> anyhow::Error {
    match retry_policy {
        RequestRetryPolicy::Idempotent => anyhow!(
            "codex app-server overloaded while processing {method}; retry later if the issue persists"
        ),
        RequestRetryPolicy::Never => anyhow!(
            "codex app-server overloaded while processing {method}; the request was not retried automatically because it may not be idempotent"
        ),
    }
}

pub(crate) fn is_codex_cli_unavailable(err: &anyhow::Error) -> bool {
    err.to_string().contains(CODEX_SIDECAR_UNAVAILABLE_PREFIX)
}

#[derive(Debug, Clone)]
pub(crate) struct CodexThreadRequest {
    pub(crate) cwd: String,
    pub(crate) model: Option<String>,
    pub(crate) approval_policy: &'static str,
    pub(crate) sandbox: &'static str,
}

impl CodexThreadRequest {
    fn thread_start_params(&self, ephemeral: bool) -> Value {
        json!({
            "approvalPolicy": self.approval_policy,
            "approvalsReviewer": "user",
            "cwd": self.cwd,
            "ephemeral": ephemeral,
            "model": self.model,
            "personality": "pragmatic",
            "sandbox": self.sandbox,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CodexTurnRequest {
    pub(crate) thread_id: String,
    pub(crate) input: String,
    pub(crate) cwd: String,
    pub(crate) model: Option<String>,
    pub(crate) approval_policy: &'static str,
    pub(crate) sandbox_policy: Value,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) collaboration_mode: Option<CodexCollaborationMode>,
}

impl CodexTurnRequest {
    fn as_json(&self) -> Value {
        let mut payload = json!({
            "approvalPolicy": self.approval_policy,
            "approvalsReviewer": "user",
            "cwd": self.cwd,
            "effort": self
                .collaboration_mode
                .as_ref()
                .map(|_| None::<String>)
                .unwrap_or_else(|| self.reasoning_effort.clone()),
            "input": [
                {
                    "type": "text",
                    "text": self.input,
                }
            ],
            "model": self
                .collaboration_mode
                .as_ref()
                .map(|_| None::<String>)
                .unwrap_or_else(|| self.model.clone()),
            "personality": "pragmatic",
            "sandboxPolicy": self.sandbox_policy,
            "summary": "concise",
            "threadId": self.thread_id,
        });
        if let Some(collaboration_mode) = self.collaboration_mode.clone()
            && let Some(object) = payload.as_object_mut()
        {
            object.insert("collaborationMode".to_string(), json!(collaboration_mode));
        }
        payload
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexAccountReadResponse {
    #[serde(default)]
    pub(crate) account: Option<CodexAccount>,
    #[serde(rename = "requiresOpenaiAuth")]
    pub(crate) requires_openai_auth: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum CodexAccount {
    #[serde(rename = "apiKey")]
    ApiKey,
    #[serde(rename = "chatgpt")]
    ChatGpt {
        email: String,
        #[serde(rename = "planType")]
        plan_type: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum CodexLoginAccountResponse {
    #[serde(rename = "apiKey")]
    ApiKey,
    #[serde(rename = "chatgpt")]
    ChatGpt {
        #[serde(rename = "authUrl")]
        auth_url: String,
        #[serde(rename = "loginId")]
        login_id: String,
    },
    #[serde(rename = "chatgptDeviceCode")]
    ChatGptDeviceCode {
        #[serde(rename = "loginId")]
        login_id: String,
        #[serde(rename = "verificationUrl")]
        verification_url: String,
        #[serde(rename = "userCode")]
        user_code: String,
    },
    #[serde(rename = "chatgptAuthTokens")]
    ChatGptAuthTokens,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexLogoutAccountResponse {}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexAccountLoginCompleted {
    #[serde(default)]
    pub(crate) error: Option<String>,
    #[serde(rename = "loginId", default)]
    pub(crate) login_id: Option<String>,
    pub(crate) success: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexMcpServerStatusListResponse {
    pub(crate) data: Vec<CodexMcpServerStatus>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexMcpServerStatus {
    #[serde(rename = "authStatus")]
    pub(crate) auth_status: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexThreadEnvelope {
    pub(crate) thread: CodexThread,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexThread {
    pub(crate) id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexTurnStartResponse {
    pub(crate) turn: CodexTurn,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexTurn {
    pub(crate) id: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexTurnSteerResponse {
    #[serde(rename = "turnId")]
    pub(crate) turn_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexReviewStartResponse {
    #[serde(rename = "reviewThreadId")]
    pub(crate) review_thread_id: String,
    pub(crate) turn: CodexTurn,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexReviewStartRequest {
    pub(crate) thread_id: String,
    pub(crate) target: CodexReviewTarget,
}

impl CodexReviewStartRequest {
    fn as_json(&self) -> Value {
        json!({
            "target": self.target,
            "threadId": self.thread_id,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub(crate) enum CodexReviewTarget {
    #[serde(rename = "uncommittedChanges")]
    UncommittedChanges,
    #[serde(rename = "baseBranch")]
    BaseBranch { branch: String },
    #[serde(rename = "commit")]
    Commit {
        sha: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    #[serde(rename = "custom")]
    Custom { instructions: String },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexCollaborationModeListResponse {
    pub(crate) data: Vec<CodexCollaborationModeMask>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct CodexCollaborationModeMask {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) model: Option<String>,
    #[serde(rename = "reasoning_effort", default)]
    pub(crate) reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CodexCollaborationMode {
    pub(crate) mode: String,
    pub(crate) settings: CodexCollaborationModeSettings,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct CodexCollaborationModeSettings {
    #[serde(rename = "developer_instructions")]
    pub(crate) developer_instructions: Option<String>,
    pub(crate) model: String,
    #[serde(rename = "reasoning_effort", skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub(crate) struct CodexCommandExecRequest {
    pub(crate) command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cwd: Option<String>,
    #[serde(rename = "sandboxPolicy", skip_serializing_if = "Option::is_none")]
    pub(crate) sandbox_policy: Option<Value>,
    #[serde(rename = "streamStdoutStderr", skip_serializing_if = "Option::is_none")]
    pub(crate) stream_stdout_stderr: Option<bool>,
}

impl CodexCommandExecRequest {
    fn as_json(&self) -> Value {
        json!(self)
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexCommandExecResponse {
    #[serde(rename = "exitCode")]
    pub(crate) exit_code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexEmptyResponse {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ChatGptAuthTokensRefreshParams {
    #[serde(rename = "previousAccountId", default)]
    pub(crate) previous_account_id: Option<String>,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ChatGptAuthTokens {
    #[serde(rename = "accessToken")]
    pub(crate) access_token: String,
    #[serde(rename = "chatgptAccountId")]
    pub(crate) chatgpt_account_id: String,
    #[serde(rename = "chatgptPlanType", default)]
    pub(crate) chatgpt_plan_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        CodexCollaborationMode, CodexCollaborationModeSettings, CodexLoginAccountResponse,
        CodexThreadRequest, CodexTurnRequest, RequestRetryPolicy, STDIO_LISTEN_TARGET,
        codex_sidecar_requirement_note, ensure_codex_sidecar_command_available,
        idempotent_retry_delay, is_codex_cli_unavailable, mcp_server_status_list_params,
        overloaded_request_error, resolve_sidecar_command_path_with_path, validate_listen_target,
    };
    use anyhow::anyhow;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    fn write_fake_executable(path: &Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").expect("write fake executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).expect("metadata").permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).expect("set permissions");
        }
    }

    #[test]
    fn codex_cli_unavailable_detection_matches_install_guidance() {
        let error = anyhow!(
            "Codex app-server sidecar is unavailable. {}",
            codex_sidecar_requirement_note()
        );
        assert!(is_codex_cli_unavailable(&error));
        assert!(!is_codex_cli_unavailable(&anyhow!("other failure")));
    }

    #[test]
    fn sidecar_requirement_note_mentions_path_and_override() {
        let note = codex_sidecar_requirement_note();
        assert!(note.contains("`codex` CLI"));
        assert!(note.contains("`$PATH`"));
        assert!(note.contains("[agent.codex_app_server].command"));
    }

    #[test]
    fn relative_sidecar_path_is_accepted_when_executable() {
        let temp = tempdir().expect("tempdir");
        let script = temp.path().join("codex-sidecar");
        write_fake_executable(&script);

        ensure_codex_sidecar_command_available(
            script.to_str().expect("script path should be valid utf-8"),
        )
        .expect("custom executable path should be accepted");
    }

    #[test]
    fn default_command_resolves_from_path_search() {
        let temp = tempdir().expect("tempdir");
        let script = temp.path().join("codex");
        write_fake_executable(&script);

        let resolved =
            resolve_sidecar_command_path_with_path("codex", Some(temp.path().as_os_str()))
                .expect("command should resolve from provided PATH");
        assert_eq!(resolved, script);
    }

    #[test]
    fn stdio_listen_target_matches_supported_proxy_transport() {
        assert_eq!(STDIO_LISTEN_TARGET, "stdio://");
    }

    #[test]
    fn proxy_accepts_non_stdio_transports() {
        assert_eq!(
            validate_listen_target("ws://127.0.0.1:0").expect("listen target should be accepted"),
            "ws://127.0.0.1:0"
        );
    }

    #[test]
    fn deserializes_chatgpt_device_code_login_response() {
        let response: CodexLoginAccountResponse = serde_json::from_value(json!({
            "type": "chatgptDeviceCode",
            "loginId": "login-123",
            "verificationUrl": "https://auth.example.test/device",
            "userCode": "ABCD-1234"
        }))
        .expect("device-code response should deserialize");

        assert!(matches!(
            response,
            CodexLoginAccountResponse::ChatGptDeviceCode {
                login_id,
                verification_url,
                user_code
            } if login_id == "login-123"
                && verification_url == "https://auth.example.test/device"
                && user_code == "ABCD-1234"
        ));
    }

    #[test]
    fn thread_and_turn_requests_include_approvals_reviewer() {
        let thread = CodexThreadRequest {
            cwd: "/tmp/demo".to_string(),
            model: Some("gpt-5".to_string()),
            approval_policy: "interactive",
            sandbox: "workspace-write",
        };
        let turn = CodexTurnRequest {
            thread_id: "thread-123".to_string(),
            input: "hello".to_string(),
            cwd: "/tmp/demo".to_string(),
            model: Some("gpt-5".to_string()),
            approval_policy: "interactive",
            sandbox_policy: json!({"type": "workspaceWrite", "networkAccess": false}),
            reasoning_effort: Some("medium".to_string()),
            collaboration_mode: None,
        };

        assert_eq!(
            thread.thread_start_params(false)["approvalsReviewer"],
            json!("user")
        );
        assert_eq!(turn.as_json()["approvalsReviewer"], json!("user"));
        assert!(turn.as_json().get("collaborationMode").is_none());
    }

    #[test]
    fn turn_request_serializes_collaboration_mode_with_builtin_instructions() {
        let turn = CodexTurnRequest {
            thread_id: "thread-123".to_string(),
            input: "plan this change".to_string(),
            cwd: "/tmp/demo".to_string(),
            model: Some("gpt-5".to_string()),
            approval_policy: "interactive",
            sandbox_policy: json!({"type": "readOnly", "networkAccess": false}),
            reasoning_effort: Some("medium".to_string()),
            collaboration_mode: Some(CodexCollaborationMode {
                mode: "plan".to_string(),
                settings: CodexCollaborationModeSettings {
                    developer_instructions: None,
                    model: "gpt-5".to_string(),
                    reasoning_effort: Some("medium".to_string()),
                },
            }),
        };

        assert_eq!(turn.as_json()["collaborationMode"]["mode"], json!("plan"));
        assert_eq!(
            turn.as_json()["collaborationMode"]["settings"]["developer_instructions"],
            serde_json::Value::Null
        );
        assert_eq!(turn.as_json()["model"], serde_json::Value::Null);
        assert_eq!(turn.as_json()["effort"], serde_json::Value::Null);
    }

    #[test]
    fn overloaded_non_idempotent_requests_surface_retry_guidance() {
        let error = overloaded_request_error("turn/start", RequestRetryPolicy::Never);
        assert!(error.to_string().contains("not be idempotent"));
    }

    #[test]
    fn idempotent_retry_delay_increases_per_attempt() {
        assert!(idempotent_retry_delay(2) > idempotent_retry_delay(1));
    }

    #[test]
    fn mcp_server_status_list_uses_lightweight_detail_mode() {
        assert_eq!(
            mcp_server_status_list_params(),
            json!({
                "detail": "toolsAndAuthOnly"
            })
        );
    }

    #[test]
    fn checked_in_schema_fixture_matches_turn_fields_in_use() {
        let schema: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/codex_app_server_schema/v2/TurnStartParams.json"
        ))
        .expect("schema fixture should parse");

        let properties = &schema["properties"];
        assert!(properties.get("collaborationMode").is_some());
        assert!(properties.get("sandboxPolicy").is_some());
        assert!(properties.get("approvalPolicy").is_some());
    }
}
