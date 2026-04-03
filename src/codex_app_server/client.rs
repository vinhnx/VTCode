use anyhow::{Context, Result, anyhow, bail};
use futures::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::broadcast;
use vtcode_acp::{StdioTransport, StdioTransportOptions};
use vtcode_config::{AgentCodexAppServerConfig, VTCodeConfig};

pub(crate) const CODEX_PROVIDER: &str = "codex";
const STDIO_LISTEN_TARGET: &str = "stdio://";
const DEFAULT_RPC_TIMEOUT_SECS: u64 = 30;
const SERVER_ERROR_CODE: i32 = -32000;
const CODEX_CLI_UNAVAILABLE_MESSAGE: &str = "Codex CLI is unavailable. Install `codex` or set [agent.codex_app_server].command in vtcode.toml.";

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

impl CodexAppServerClient {
    pub(crate) async fn connect(vt_cfg: Option<&VTCodeConfig>) -> Result<Self> {
        let sidecar_cfg = sidecar_config(vt_cfg);
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

    pub(crate) async fn account_read(&self) -> Result<CodexAccountReadResponse> {
        self.request("account/read", json!({})).await
    }

    pub(crate) async fn account_login_chatgpt(&self) -> Result<CodexLoginAccountResponse> {
        self.request("account/login/start", json!({ "type": "chatgpt" }))
            .await
    }

    pub(crate) async fn account_login_chatgpt_device_code(
        &self,
    ) -> Result<CodexLoginAccountResponse> {
        self.request(
            "account/login/start",
            json!({ "type": "chatgptDeviceCode" }),
        )
        .await
    }

    pub(crate) async fn account_logout(&self) -> Result<()> {
        let _: CodexLogoutAccountResponse = self.request("account/logout", json!({})).await?;
        Ok(())
    }

    pub(crate) async fn mcp_server_status_list(&self) -> Result<CodexMcpServerStatusListResponse> {
        self.request("mcpServerStatus/list", json!({})).await
    }

    pub(crate) async fn thread_start(
        &self,
        params: CodexThreadRequest,
        ephemeral: bool,
    ) -> Result<CodexThreadEnvelope> {
        self.request("thread/start", params.thread_start_params(ephemeral))
            .await
    }

    pub(crate) async fn thread_resume(&self, thread_id: &str) -> Result<CodexThreadEnvelope> {
        self.request("thread/resume", json!({ "threadId": thread_id }))
            .await
    }

    pub(crate) async fn thread_fork(
        &self,
        thread_id: &str,
        params: CodexThreadRequest,
        ephemeral: bool,
    ) -> Result<CodexThreadEnvelope> {
        let mut request = params.thread_start_params(ephemeral);
        if let Some(object) = request.as_object_mut() {
            object.insert("threadId".to_string(), Value::String(thread_id.to_string()));
        }
        self.request("thread/fork", request).await
    }

    pub(crate) async fn turn_start(
        &self,
        params: CodexTurnRequest,
    ) -> Result<CodexTurnStartResponse> {
        self.request("turn/start", params.as_json()).await
    }

    pub(crate) fn respond_to_server_request(&self, id: Value, result: Value) -> Result<()> {
        self.transport
            .respond_value(id, result)
            .map_err(|err| anyhow!(err.to_string()))
    }

    async fn request<T>(&self, method: &str, params: Value) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let response = self
            .transport
            .call(method, params)
            .await
            .map_err(|err| anyhow!(err.to_string()))
            .with_context(|| format!("codex app-server request failed: {method}"))?;

        serde_json::from_value(response)
            .with_context(|| format!("failed to decode codex app-server response for {method}"))
    }
}

pub(crate) async fn launch_app_server_proxy(
    vt_cfg: Option<&VTCodeConfig>,
    listen: &str,
) -> Result<()> {
    let listen = listen.trim();
    if listen.is_empty() {
        bail!("app-server listen target cannot be empty");
    }
    if listen != STDIO_LISTEN_TARGET {
        bail!(
            "vtcode app-server currently supports only `--listen {STDIO_LISTEN_TARGET}`; use `codex app-server` directly for other transports"
        );
    }

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

fn codex_cli_unavailable_error() -> anyhow::Error {
    anyhow!(CODEX_CLI_UNAVAILABLE_MESSAGE)
}

pub(crate) fn is_codex_cli_unavailable(err: &anyhow::Error) -> bool {
    err.to_string().contains(CODEX_CLI_UNAVAILABLE_MESSAGE)
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
}

impl CodexTurnRequest {
    fn as_json(&self) -> Value {
        json!({
            "approvalPolicy": self.approval_policy,
            "approvalsReviewer": "user",
            "cwd": self.cwd,
            "effort": self.reasoning_effort,
            "input": [
                {
                    "type": "text",
                    "text": self.input,
                }
            ],
            "model": self.model,
            "personality": "pragmatic",
            "sandboxPolicy": self.sandbox_policy,
            "summary": "concise",
            "threadId": self.thread_id,
        })
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
        CodexLoginAccountResponse, CodexThreadRequest, CodexTurnRequest, STDIO_LISTEN_TARGET,
        is_codex_cli_unavailable, launch_app_server_proxy,
    };
    use anyhow::anyhow;
    use serde_json::json;

    #[test]
    fn codex_cli_unavailable_detection_matches_install_guidance() {
        let error = anyhow!(
            "Codex CLI is unavailable. Install `codex` or set [agent.codex_app_server].command in vtcode.toml."
        );
        assert!(is_codex_cli_unavailable(&error));
        assert!(!is_codex_cli_unavailable(&anyhow!("other failure")));
    }

    #[test]
    fn stdio_listen_target_matches_supported_proxy_transport() {
        assert_eq!(STDIO_LISTEN_TARGET, "stdio://");
    }

    #[tokio::test]
    async fn proxy_rejects_non_stdio_transports() {
        let error = launch_app_server_proxy(None, "ws://127.0.0.1:0")
            .await
            .expect_err("non-stdio listen target should fail");

        assert!(
            error
                .to_string()
                .contains("supports only `--listen stdio://`")
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
        };

        assert_eq!(
            thread.thread_start_params(false)["approvalsReviewer"],
            json!("user")
        );
        assert_eq!(turn.as_json()["approvalsReviewer"], json!("user"));
    }
}
