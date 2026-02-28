use super::rmcp_transport::create_stdio_transport_with_stderr;
use super::{McpElicitationHandler, convert_to_rmcp, create_env_for_mcp_server};
use anyhow::{Context, Result, anyhow};
use futures::FutureExt;
use jsonschema::Validator;
use reqwest::header::HeaderMap;
use rmcp::handler::client::ClientHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, CancelledNotificationParam,
    CreateElicitationRequestParams, ElicitationAction, GetPromptRequestParams, GetPromptResult,
    InitializeRequestParams, InitializeResult, ListRootsResult, LoggingLevel,
    LoggingMessageNotificationParam, ProgressNotificationParam, Prompt, ReadResourceRequestParams,
    ReadResourceResult, Resource, ResourceTemplate, ResourceUpdatedNotificationParam, Root, Tool,
};
use rmcp::service::{self, NotificationContext, RequestContext, RoleClient, RunningService};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::{
    StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio::time;
use tracing::{debug, error, info, warn};
use url::Url;

/// High level MCP client responsible for managing multiple providers and
/// enforcing VT Code specific policies like tool allow lists.
pub(crate) struct RmcpClient {
    provider_name: String,
    state: Mutex<ClientState>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    /// Handle for the background stderr reader task (stdio transports only).
    /// Stored so we can abort it when the client is shut down or replaced.
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

enum ClientState {
    Connecting {
        transport: Option<PendingTransport>,
    },
    Ready {
        service: Arc<RunningService<RoleClient, LoggingClientHandler>>,
    },
    /// The underlying transport has disconnected (server crash, network loss).
    /// The client can potentially be replaced by a new one via `McpProvider::reconnect()`.
    Disconnected,
    Stopped,
}

enum PendingTransport {
    ChildProcess(TokioChildProcess),
    StreamableHttp(StreamableHttpClientTransport<reqwest::Client>),
}

impl RmcpClient {
    pub(super) async fn new_stdio_client(
        provider_name: String,
        program: OsString,
        args: Vec<OsString>,
        working_dir: Option<PathBuf>,
        env: Option<HashMap<String, String>>,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Result<Self> {
        let env = create_env_for_mcp_server(env);

        // Use rmcp_transport helper to create transport with stderr capture
        let (transport, stderr) =
            create_stdio_transport_with_stderr(&program, &args, working_dir.as_ref(), &env)?;

        // Spawn async task to log MCP server stderr
        let stderr_task = if let Some(stderr) = stderr {
            let program_name = program.to_string_lossy().into_owned();
            let provider_label = provider_name.clone();
            Some(tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                loop {
                    match reader.next_line().await {
                        Ok(Some(line)) => {
                            info!(
                                provider = provider_label.as_str(),
                                program = program_name.as_str(),
                                message = line.as_str(),
                                "MCP server stderr"
                            );
                        }
                        Ok(None) => break,
                        Err(error) => {
                            warn!(
                                provider = provider_label.as_str(),
                                program = program_name.as_str(),
                                error = %error,
                                "Failed to read MCP server stderr"
                            );
                            break;
                        }
                    }
                }
            }))
        } else {
            None
        };

        Ok(Self {
            provider_name,
            state: Mutex::new(ClientState::Connecting {
                transport: Some(PendingTransport::ChildProcess(transport)),
            }),
            elicitation_handler,
            stderr_task,
        })
    }

    pub(super) async fn new_streamable_http_client(
        provider_name: String,
        url: &str,
        bearer_token: Option<String>,
        headers: HeaderMap,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Result<Self> {
        let mut config = StreamableHttpClientTransportConfig::with_uri(url.to_string());
        if let Some(token) = bearer_token {
            config = config.auth_header(token);
        }

        info!(
            "Connecting to MCP HTTP provider '{}' at {}",
            provider_name, url
        );

        let mut client_builder = reqwest::Client::builder();
        if !headers.is_empty() {
            client_builder = client_builder.default_headers(headers);
        }

        let http_client = client_builder.build().with_context(|| {
            format!(
                "failed to construct reqwest client for MCP provider '{}'",
                provider_name
            )
        })?;

        let transport = StreamableHttpClientTransport::with_client(http_client, config);
        Ok(Self {
            provider_name,
            state: Mutex::new(ClientState::Connecting {
                transport: Some(PendingTransport::StreamableHttp(transport)),
            }),
            elicitation_handler,
            stderr_task: None,
        })
    }

    pub(super) async fn initialize(
        &self,
        params: InitializeRequestParams,
        timeout: Option<Duration>,
    ) -> Result<InitializeResult> {
        let handler = LoggingClientHandler::new(
            self.provider_name.clone(),
            params,
            self.elicitation_handler.clone(),
        );

        let (transport_future, service_label) = {
            let mut guard = self.state.lock().await;
            match &mut *guard {
                ClientState::Connecting { transport } => match transport.take() {
                    Some(PendingTransport::ChildProcess(transport)) => (
                        service::serve_client(handler.clone(), transport).boxed(),
                        "stdio",
                    ),
                    Some(PendingTransport::StreamableHttp(transport)) => (
                        service::serve_client(handler.clone(), transport).boxed(),
                        "http",
                    ),
                    None => {
                        return Err(anyhow!(
                            "MCP client for {} already initializing",
                            handler.provider_name()
                        ));
                    }
                },
                ClientState::Ready { .. } => {
                    return Err(anyhow!(
                        "MCP client for {} already initialized",
                        handler.provider_name()
                    ));
                }
                ClientState::Stopped => return Err(anyhow!("MCP client has been shut down")),
                ClientState::Disconnected => {
                    return Err(anyhow!(
                        "MCP client for {} is disconnected — use reconnect()",
                        handler.provider_name()
                    ));
                }
            }
        };

        let service = match timeout {
            Some(duration) => time::timeout(duration, transport_future)
                .await
                .with_context(|| {
                    format!("Timed out establishing {service_label} MCP transport")
                })??,
            None => transport_future.await?,
        };

        let initialize_result = service
            .peer()
            .peer_info()
            .ok_or_else(|| anyhow!("Handshake succeeded but server info missing"))?
            .clone();

        let mut guard = self.state.lock().await;
        *guard = ClientState::Ready {
            service: Arc::new(service),
        };

        Ok(initialize_result)
    }

    pub(super) async fn list_all_tools(&self, timeout: Option<Duration>) -> Result<Vec<Tool>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_tools();
        let tools = run_with_timeout(rmcp_future, timeout, "tools/list").await?;
        Ok(tools)
    }

    pub(super) async fn list_all_prompts(&self, timeout: Option<Duration>) -> Result<Vec<Prompt>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_prompts();
        let prompts = run_with_timeout(rmcp_future, timeout, "prompts/list").await?;
        Ok(prompts)
    }

    pub(super) async fn list_all_resources(
        &self,
        timeout: Option<Duration>,
    ) -> Result<Vec<Resource>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_resources();
        let resources = run_with_timeout(rmcp_future, timeout, "resources/list").await?;
        Ok(resources)
    }

    #[allow(dead_code)]
    pub(super) async fn list_all_resource_templates(
        &self,
        timeout: Option<Duration>,
    ) -> Result<Vec<ResourceTemplate>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_resource_templates();
        let templates = run_with_timeout(rmcp_future, timeout, "resources/templates/list").await?;
        Ok(templates)
    }

    pub(super) async fn call_tool(
        &self,
        params: CallToolRequestParams,
        timeout: Option<Duration>,
    ) -> Result<CallToolResult> {
        let service = self.service().await?;
        let result = run_with_timeout(service.call_tool(params), timeout, "tools/call").await?;
        Ok(result)
    }

    pub(super) async fn read_resource(
        &self,
        params: ReadResourceRequestParams,
        timeout: Option<Duration>,
    ) -> Result<ReadResourceResult> {
        let service = self.service().await?;
        let result = run_with_timeout(
            service.peer().read_resource(params),
            timeout,
            "resources/read",
        )
        .await?;
        Ok(result)
    }

    pub(super) async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        timeout: Option<Duration>,
    ) -> Result<GetPromptResult> {
        let service = self.service().await?;
        let result =
            run_with_timeout(service.peer().get_prompt(params), timeout, "prompts/get").await?;
        Ok(result)
    }

    pub(super) async fn shutdown(&self) -> Result<()> {
        let mut guard = self.state.lock().await;
        let state = std::mem::replace(&mut *guard, ClientState::Stopped);
        drop(guard);

        match state {
            ClientState::Ready { service } => {
                service.cancellation_token().cancel();
                Ok(())
            }
            ClientState::Connecting { mut transport } => {
                drop(transport.take());
                Ok(())
            }
            ClientState::Disconnected | ClientState::Stopped => Ok(()),
        }
    }

    async fn service(&self) -> Result<Arc<RunningService<RoleClient, LoggingClientHandler>>> {
        let mut guard = self.state.lock().await;
        match &*guard {
            ClientState::Ready { service } => {
                // Detect if the underlying transport has died (server crash / network loss).
                if service.is_closed() {
                    warn!(
                        provider = self.provider_name.as_str(),
                        "MCP service closed — marking disconnected"
                    );
                    *guard = ClientState::Disconnected;
                    return Err(anyhow!(
                        "MCP client for '{}' has disconnected",
                        self.provider_name
                    ));
                }
                Ok(service.clone())
            }
            ClientState::Connecting { .. } => Err(anyhow!("MCP client not initialized")),
            ClientState::Disconnected => Err(anyhow!(
                "MCP client for '{}' has disconnected",
                self.provider_name
            )),
            ClientState::Stopped => Err(anyhow!("MCP client has been shut down")),
        }
    }

    /// Returns `true` when the client is in the `Ready` state and the
    /// underlying transport has not been closed.
    pub(super) async fn is_healthy(&self) -> bool {
        let guard = self.state.lock().await;
        matches!(&*guard, ClientState::Ready { service } if !service.is_closed())
    }
}

impl Drop for RmcpClient {
    fn drop(&mut self) {
        // Abort the background stderr reader task so it doesn't outlive the client.
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }
    }
}

#[derive(Clone)]
struct LoggingClientHandler {
    provider: String,
    initialize_params: InitializeRequestParams,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
}

impl LoggingClientHandler {
    fn new(
        provider_name: String,
        params: InitializeRequestParams,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    ) -> Self {
        Self {
            provider: provider_name,
            initialize_params: params,
            elicitation_handler,
        }
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }

    fn handle_logging(&self, params: LoggingMessageNotificationParam) {
        let logger = params.logger.unwrap_or_default();
        let summary = params
            .data
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| params.data.to_string());

        match params.level {
            LoggingLevel::Debug => debug!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Info | LoggingLevel::Notice => info!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider log"
            ),
            LoggingLevel::Warning => warn!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider warning"
            ),
            LoggingLevel::Error
            | LoggingLevel::Critical
            | LoggingLevel::Alert
            | LoggingLevel::Emergency => error!(
                provider = self.provider.as_str(),
                logger = logger.as_str(),
                summary = %summary,
                payload = ?params.data,
                "MCP provider error"
            ),
        }
    }
}

impl ClientHandler for LoggingClientHandler {
    fn create_elicitation(
        &self,
        request: CreateElicitationRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<
        Output = Result<rmcp::model::CreateElicitationResult, rmcp::ErrorData>,
    > + Send
    + '_ {
        let provider = self.provider.clone();
        let handler = self.elicitation_handler.clone();
        async move {
            let default_response = rmcp::model::CreateElicitationResult {
                action: ElicitationAction::Decline,
                content: None,
            };

            if let Some(handler) = handler {
                let schema_value = match serde_json::to_value(&request.requested_schema) {
                    Ok(value) => value,
                    Err(err) => {
                        warn!(
                            provider = provider.as_str(),
                            error = %err,
                            "Failed to serialize MCP elicitation schema; using null placeholder"
                        );
                        Value::Null
                    }
                };
                let validator = build_elicitation_validator(provider.as_str(), &schema_value);
                let message = request.message.clone();
                let payload = super::McpElicitationRequest {
                    message: message.clone(),
                    requested_schema: schema_value.clone(),
                };

                match handler.handle_elicitation(&provider, payload).await {
                    Ok(response) => {
                        validate_elicitation_payload(
                            provider.as_str(),
                            validator.as_ref(),
                            &response.action,
                            response.content.as_ref(),
                        )?;
                        info!(
                            provider = provider.as_str(),
                            message = message.as_str(),
                            action = ?response.action,
                            "MCP provider elicitation handled"
                        );
                        return Ok(rmcp::model::CreateElicitationResult {
                            action: response.action,
                            content: response.content,
                        });
                    }
                    Err(err) => {
                        warn!(
                            provider = provider.as_str(),
                            message = message.as_str(),
                            error = %err,
                            "Failed to process MCP elicitation; declining"
                        );
                    }
                }
            } else {
                info!(
                    provider = provider.as_str(),
                    message = request.message.as_str(),
                    "MCP provider requested elicitation but no handler configured; declining"
                );
            }

            Ok(default_response)
        }
    }

    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl std::future::Future<Output = Result<ListRootsResult, rmcp::ErrorData>> + Send + '_
    {
        let provider = self.provider.clone();
        async move {
            let mut roots = Vec::new();
            match std::env::current_dir() {
                Ok(dir) => {
                    if let Some(uri) = directory_to_file_uri(&dir) {
                        roots.push(Root {
                            name: Some("workspace".to_owned()),
                            uri,
                        });
                    } else {
                        warn!(
                            provider = provider.as_str(),
                            path = %dir.display(),
                            "Failed to convert workspace directory to file URI for MCP roots"
                        );
                    }
                }
                Err(err) => {
                    warn!(
                        provider = provider.as_str(),
                        error = %err,
                        "Failed to resolve current directory for MCP roots"
                    );
                }
            }

            Ok(ListRootsResult { roots })
        }
    }

    fn on_cancelled(
        &self,
        params: CancelledNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        debug!(
            provider = self.provider.as_str(),
            request_id = %params.request_id,
            reason = ?params.reason,
            "MCP provider cancelled request"
        );
        async move {}
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            progress_token = ?params.progress_token,
            progress = params.progress,
            total = ?params.total,
            message = ?params.message,
            "MCP provider progress update"
        );
        async move {}
    }

    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        self.handle_logging(params);
        async move {}
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            uri = params.uri.as_str(),
            "MCP resource updated"
        );
        async move {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported resource list change"
        );
        async move {}
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported tool list change"
        );
        async move {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl std::future::Future<Output = ()> + Send + '_ {
        info!(
            provider = self.provider.as_str(),
            "MCP provider reported prompt list change"
        );
        async move {}
    }

    fn get_info(&self) -> rmcp::model::ClientInfo {
        convert_to_rmcp(self.initialize_params.clone())
            .expect("initialize params conversion should not fail")
    }
}

pub(crate) fn build_elicitation_validator(provider: &str, schema: &Value) -> Option<Validator> {
    if schema.is_null() {
        return None;
    }

    match Validator::new(schema) {
        Ok(validator) => Some(validator),
        Err(err) => {
            warn!(
                provider = provider,
                error = %err,
                "Failed to build JSON schema validator for MCP elicitation; skipping validation"
            );
            None
        }
    }
}

pub(crate) fn validate_elicitation_payload(
    provider: &str,
    validator: Option<&Validator>,
    action: &ElicitationAction,
    content: Option<&Value>,
) -> Result<(), rmcp::ErrorData> {
    if !matches!(action, ElicitationAction::Accept) {
        return Ok(());
    }

    let Some(validator) = validator else {
        return Ok(());
    };

    let Some(payload) = content else {
        warn!(
            provider = provider,
            "MCP elicitation accept action missing response content"
        );
        return Err(rmcp::ErrorData::invalid_params(
            "Elicitation response missing content for accept action",
            None,
        ));
    };

    if !validator.is_valid(payload) {
        let messages: Vec<String> = validator
            .iter_errors(payload)
            .map(|err| err.to_string())
            .collect();
        warn!(
            provider = provider,
            errors = ?messages,
            "MCP elicitation response failed schema validation"
        );
        return Err(rmcp::ErrorData::invalid_params(
            "Elicitation response failed schema validation",
            Some(json!({ "errors": messages })),
        ));
    }

    Ok(())
}

pub(crate) fn directory_to_file_uri(path: &Path) -> Option<String> {
    Url::from_directory_path(path)
        .ok()
        .map(|url| url.to_string())
}

async fn run_with_timeout<F, T>(fut: F, timeout: Option<Duration>, label: &str) -> Result<T>
where
    F: std::future::Future<Output = Result<T, rmcp::service::ServiceError>>,
{
    if let Some(duration) = timeout {
        let result = time::timeout(duration, fut)
            .await
            .with_context(|| anyhow!("Timed out awaiting {label} after {duration:?}"))?;
        result.map_err(|err| anyhow!("{label} failed: {err}"))
    } else {
        fut.await.map_err(|err| anyhow!("{label} failed: {err}"))
    }
}
