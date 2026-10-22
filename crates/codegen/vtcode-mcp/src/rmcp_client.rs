use super::rmcp_transport::create_stdio_transport_with_stderr;
use super::{McpElicitationHandler, McpSandboxContext, convert_to_rmcp, create_env_for_mcp_server};
use anyhow::{Context, Result, anyhow};
use futures::FutureExt;
use hashbrown::HashMap;
use jsonschema::Validator;
use rmcp::handler::client::ClientHandler;
#[allow(
    deprecated,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
use rmcp::model::{
    CallToolRequestParams, CallToolResult, CancelledNotificationParam, ClientResult, CustomResult, ElicitRequestParams,
    ElicitationAction, GetPromptRequestParams, GetPromptResult, InitializeRequestParams, ListRootsResult, LoggingLevel,
    LoggingMessageNotificationParam, MetaObject, ProgressNotificationParam, Prompt, ReadResourceRequestParams,
    ReadResourceResult, RequestMetaObject, Resource, ResourceTemplate, ResourceUpdatedNotificationParam, Root,
    ServerNotification, ServerPeerInfo, ServerRequest, Tool,
};
use rmcp::service::{
    self, ClientCacheConfig, ClientLifecycleMode, NotificationContext, RequestContext, RoleClient, RunningService,
    Service,
};
use rmcp::transport::child_process::TokioChildProcess;
use rmcp::transport::streamable_http_client::{StreamableHttpClientTransport, StreamableHttpClientTransportConfig};
use rmcp_reqwest::header::HeaderMap;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

const DISCOVER_PREFERRED_VERSIONS: &[rmcp::model::ProtocolVersion] = &[
    rmcp::model::ProtocolVersion::V_2025_11_25,
    rmcp::model::ProtocolVersion::V_2025_06_18,
    rmcp::model::ProtocolVersion::V_2025_03_26,
    rmcp::model::ProtocolVersion::V_2024_11_05,
];
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;
use tokio::time;
use tracing::{debug, error, info, warn};
use url::Url;
use vtcode_commons::sanitizer::sanitize_provider_diagnostic;

/// Response-cache policy for the rmcp peer (SEP-2549 `server/discover` TTL
/// framework: `tools/list` and friends are cached per peer and refreshed on
/// `list_changed`).
///
/// Bounds entries, keeps stale-serving off so transport errors surface instead
/// of resurrecting pruned tool catalogs (fail-closed: our registry already
/// prunes proxies on disconnect), and partitions by provider so credential
/// changes cannot cross-contaminate cached private responses.
pub(crate) fn peer_cache_config(provider_name: &str) -> ClientCacheConfig {
    ClientCacheConfig::default()
        .with_max_entries(128)
        .with_serve_stale_on_error(false)
        .with_private_partition(provider_name)
}

/// Central pinned MCP protocol versions (typed rmcp counterparts).
///
/// The string counterparts live in `vtcode-config::mcp`
/// (`MCP_STABLE_PROTOCOL_VERSION`, `MCP_LEGACY_PROTOCOL_VERSION`); keep both
/// sides aligned when the spec publishes a new stable revision.
/// (`SUPPORTED_PROTOCOL_VERSIONS` in `lib.rs` instead tracks rmcp's known
/// versions and is pinned to them by test.)
pub(crate) fn stable_protocol_version() -> rmcp::model::ProtocolVersion {
    rmcp::model::ProtocolVersion::V_2025_11_25
}

/// Last-resort version for legacy fallbacks (stdio `Auto` mode and HTTP
/// providers that opt in to discover-then-fallback).
pub(crate) fn legacy_fallback_protocol_version() -> rmcp::model::ProtocolVersion {
    rmcp::model::ProtocolVersion::V_2024_11_05
}

/// Newest known version, used as the opening offer before negotiation or
/// clamping settles it down.
pub(crate) fn latest_protocol_version() -> rmcp::model::ProtocolVersion {
    rmcp::model::ProtocolVersion::V_2026_07_28
}

/// Discover-then-legacy lifecycle shared by stdio transports and HTTP
/// providers that opt in via `handshake = "auto"`.
pub(crate) fn auto_lifecycle_mode() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto {
        preferred_versions: DISCOVER_PREFERRED_VERSIONS.to_vec(),
        legacy_version: Some(legacy_fallback_protocol_version()),
    }
}

/// Highest protocol version sent on the wire for the legacy `initialize`
/// handshake.
///
/// The `2026-07-28` draft is excluded: legacy streamable-HTTP servers (e.g.
/// DeepWiki, max `2025-11-25`) reject unknown versions with HTTP 400 whose
/// JSON-RPC error carries the non-correlated id `"server-error"`, surfacing in
/// rmcp as `UncorrelatedErrorResponse { expected: 0, received: "server-error" }`
/// with the real validation payload discarded. Capping here keeps every
/// handshake path (`connect_server`, pool startup, `reconnect`) on a version
/// legacy servers accept; negotiation can still settle lower.
fn clamp_initialize_protocol_version(version: rmcp::model::ProtocolVersion) -> rmcp::model::ProtocolVersion {
    if version > stable_protocol_version() {
        stable_protocol_version()
    } else {
        version
    }
}

const MCP_PROGRESS_TOKEN_META_KEY: &str = "progressToken";
const MCP_STDERR_MAX_BYTES: usize = 8 * 1024;
const LIST_CHANGED_BUCKET_CAPACITY: u8 = 4;
const LIST_CHANGED_REFILL_INTERVAL: Duration = Duration::from_secs(1);

/// High level MCP client responsible for managing multiple providers and
/// enforcing VT Code specific policies like tool allow lists.
pub(crate) struct RmcpClient {
    provider_name: String,
    state: Mutex<ClientState>,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    list_changed_state: Arc<ListChangedState>,
    /// Handle for the background stderr reader task (stdio transports only).
    /// Stored so we can abort it when the client is shut down or replaced.
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

enum ClientState {
    Connecting {
        transport: Option<PendingTransport>,
    },
    Ready {
        service: Arc<RunningService<RoleClient, ElicitationClientService>>,
    },
    /// The underlying transport has disconnected (server crash, network loss).
    /// The client can potentially be replaced by a new one via `McpProvider::reconnect()`.
    Disconnected,
    Stopped,
}

enum PendingTransport {
    ChildProcess(TokioChildProcess),
    StreamableHttp(StreamableHttpClientTransport<rmcp_reqwest::Client>),
}

struct ListChangedState {
    tools: AtomicBool,
    resources: AtomicBool,
    prompts: AtomicBool,
    tools_dirty: AtomicBool,
    resources_dirty: AtomicBool,
    prompts_dirty: AtomicBool,
    limiter: StdMutex<ListChangedLimiter>,
}

#[derive(Clone, Copy)]
enum ListChangedNamespace {
    Tools,
    Resources,
    Prompts,
}

#[derive(Default)]
struct ListChangedLimiter {
    tools: TokenBucket,
    resources: TokenBucket,
    prompts: TokenBucket,
}

struct TokenBucket {
    tokens: u8,
    last_refill: std::time::Instant,
}

impl Default for ListChangedState {
    fn default() -> Self {
        Self {
            tools: AtomicBool::new(false),
            resources: AtomicBool::new(false),
            prompts: AtomicBool::new(false),
            tools_dirty: AtomicBool::new(false),
            resources_dirty: AtomicBool::new(false),
            prompts_dirty: AtomicBool::new(false),
            limiter: StdMutex::new(ListChangedLimiter::default()),
        }
    }
}

impl Default for TokenBucket {
    fn default() -> Self {
        Self {
            tokens: LIST_CHANGED_BUCKET_CAPACITY,
            last_refill: std::time::Instant::now(),
        }
    }
}

impl TokenBucket {
    fn try_take_at(&mut self, now: std::time::Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_refill);
        let refill_count = elapsed.as_secs() / LIST_CHANGED_REFILL_INTERVAL.as_secs();
        if refill_count > 0 {
            self.tokens = self
                .tokens
                .saturating_add(u8::try_from(refill_count).unwrap_or(u8::MAX))
                .min(LIST_CHANGED_BUCKET_CAPACITY);
            self.last_refill +=
                LIST_CHANGED_REFILL_INTERVAL.saturating_mul(u32::try_from(refill_count).unwrap_or(u32::MAX));
        }

        if self.tokens == 0 {
            return false;
        }
        self.tokens -= 1;
        true
    }
}

impl ListChangedState {
    fn mark_tools_changed(&self) {
        self.mark_changed(ListChangedNamespace::Tools, std::time::Instant::now());
    }

    fn mark_resources_changed(&self) {
        self.mark_changed(ListChangedNamespace::Resources, std::time::Instant::now());
    }

    fn mark_prompts_changed(&self) {
        self.mark_changed(ListChangedNamespace::Prompts, std::time::Instant::now());
    }

    fn mark_changed(&self, namespace: ListChangedNamespace, now: std::time::Instant) {
        // Preserve burst coalescing: multiple notifications before the cache
        // consumer observes one pending signal do not consume bucket tokens.
        let (flag, dirty) = self.state_for(namespace);
        dirty.store(true, Ordering::Relaxed);
        if flag.load(Ordering::Relaxed) {
            return;
        }

        self.schedule_changed(namespace, now);
    }

    fn schedule_changed(&self, namespace: ListChangedNamespace, now: std::time::Instant) {
        let (flag, dirty) = self.state_for(namespace);
        if flag.load(Ordering::Relaxed) || !dirty.load(Ordering::Relaxed) {
            return;
        }

        let Ok(mut limiter) = self.limiter.lock() else {
            // A poisoned limiter must not turn an unbounded notification
            // stream into repeated refresh work.
            return;
        };
        let bucket = match namespace {
            ListChangedNamespace::Tools => &mut limiter.tools,
            ListChangedNamespace::Resources => &mut limiter.resources,
            ListChangedNamespace::Prompts => &mut limiter.prompts,
        };
        if bucket.try_take_at(now) {
            dirty.store(false, Ordering::Relaxed);
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn state_for(&self, namespace: ListChangedNamespace) -> (&AtomicBool, &AtomicBool) {
        match namespace {
            ListChangedNamespace::Tools => (&self.tools, &self.tools_dirty),
            ListChangedNamespace::Resources => (&self.resources, &self.resources_dirty),
            ListChangedNamespace::Prompts => (&self.prompts, &self.prompts_dirty),
        }
    }

    fn take_tools_changed(&self) -> bool {
        self.take_changed(ListChangedNamespace::Tools, std::time::Instant::now())
    }

    fn take_resources_changed(&self) -> bool {
        self.take_changed(ListChangedNamespace::Resources, std::time::Instant::now())
    }

    fn take_prompts_changed(&self) -> bool {
        self.take_changed(ListChangedNamespace::Prompts, std::time::Instant::now())
    }

    fn take_changed(&self, namespace: ListChangedNamespace, now: std::time::Instant) -> bool {
        let (flag, _) = self.state_for(namespace);
        let notified = flag.swap(false, Ordering::Relaxed);
        self.schedule_changed(namespace, now);
        notified || flag.swap(false, Ordering::Relaxed)
    }
}

impl RmcpClient {
    pub(super) async fn new_stdio_client(
        provider_name: String,
        program: OsString,
        args: Vec<OsString>,
        working_dir: Option<PathBuf>,
        env: Option<HashMap<OsString, OsString>>,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        sandbox_context: Option<McpSandboxContext>,
    ) -> Result<Self> {
        let env = create_env_for_mcp_server(env);
        let (program, args, working_dir, env) = if let Some(context) = sandbox_context {
            let transformed = context.transform_stdio(program, args, working_dir.as_deref(), env)?;
            (transformed.program, transformed.args, Some(transformed.working_dir), transformed.env)
        } else {
            (program, args, working_dir, env)
        };

        // Use rmcp_transport helper to create transport with stderr capture
        let (transport, stderr) = create_stdio_transport_with_stderr(&program, &args, working_dir.as_deref(), &env)?;

        // Spawn async task to log MCP server stderr
        let stderr_task = if let Some(stderr) = stderr {
            let program_name = program.to_string_lossy().into_owned();
            let provider_label = provider_name.clone();
            Some(tokio::spawn(async move {
                let mut reader = stderr;
                let mut chunk = [0_u8; 1024];
                let mut line = Vec::with_capacity(MCP_STDERR_MAX_BYTES);
                let mut truncated = false;

                let log_line = |line: &[u8], truncated: bool| {
                    let message = sanitize_provider_diagnostic(line);
                    info!(
                        provider = provider_label.as_str(),
                        program = program_name.as_str(),
                        message = message.as_str(),
                        truncated,
                        "MCP server stderr"
                    );
                };

                loop {
                    match reader.read(&mut chunk).await {
                        Ok(0) => break,
                        Ok(bytes_read) => {
                            // `.get(..bytes_read)` runs once per 1 KiB chunk to build the
                            // slice; the hot per-byte scan below is already bounds-check
                            // free, so keep the safe accessor here.
                            for byte in chunk.get(..bytes_read).unwrap_or_default() {
                                if *byte == b'\n' {
                                    if !line.is_empty() || truncated {
                                        log_line(&line, truncated);
                                    }
                                    line.clear();
                                    truncated = false;
                                } else if line.len() < MCP_STDERR_MAX_BYTES {
                                    line.push(*byte);
                                } else {
                                    truncated = true;
                                }
                            }
                        }
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
                if !line.is_empty() || truncated {
                    log_line(&line, truncated);
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
            list_changed_state: Arc::new(ListChangedState::default()),
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

        info!("Connecting to MCP HTTP provider '{}' at {}", provider_name, url);

        let mut client_builder = rmcp_reqwest::Client::builder();
        if !headers.is_empty() {
            client_builder = client_builder.default_headers(headers);
        }
        client_builder = client_builder
            .pool_max_idle_per_host(2)
            .pool_idle_timeout(Duration::from_secs(300))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            // Security (CVE-2026-64684 / GHSA-9g45-5xwm-f3wc): never follow
            // redirects automatically. reqwest's default `limited(10)` policy
            // forwards caller-supplied custom headers (API keys via
            // `http_headers` / `env_http_headers`) to cross-origin redirect
            // targets, stripping only `Authorization`/`Cookie`. A compromised
            // MCP server answering `307` to an attacker origin would capture
            // those secrets. Surface `3xx` as a transport error instead; MCP
            // has no legitimate redirect flow on this path.
            .redirect(rmcp_reqwest::redirect::Policy::none());

        let http_client = client_builder
            .build()
            .with_context(|| format!("failed to construct reqwest client for MCP provider '{provider_name}'"))?;

        let transport = StreamableHttpClientTransport::with_client(http_client, config);
        Ok(Self {
            provider_name,
            state: Mutex::new(ClientState::Connecting {
                transport: Some(PendingTransport::StreamableHttp(transport)),
            }),
            elicitation_handler,
            list_changed_state: Arc::new(ListChangedState::default()),
            stderr_task: None,
        })
    }

    pub(super) async fn initialize(
        &self,
        params: InitializeRequestParams,
        timeout: Option<Duration>,
        lifecycle: ClientLifecycleMode,
    ) -> Result<ServerPeerInfo> {
        let mut params = params;
        params.protocol_version = clamp_initialize_protocol_version(params.protocol_version);
        let handler = LoggingClientHandler::new(
            self.provider_name.clone(),
            params,
            self.elicitation_handler.clone(),
            Arc::clone(&self.list_changed_state),
        );
        let service_handler = ElicitationClientService::new(handler.clone());

        let (transport_future, service_label) = {
            let mut guard = self.state.lock().await;
            match &mut *guard {
                ClientState::Connecting { transport } => match transport.take() {
                    Some(PendingTransport::ChildProcess(transport)) => (
                        service::serve_client_with_lifecycle(service_handler.clone(), transport, lifecycle).boxed(),
                        "stdio",
                    ),
                    Some(PendingTransport::StreamableHttp(transport)) => (
                        service::serve_client_with_lifecycle(service_handler.clone(), transport, lifecycle).boxed(),
                        "http",
                    ),
                    None => {
                        return Err(anyhow!("MCP client for {} already initializing", handler.provider_name()));
                    }
                },
                ClientState::Ready { .. } => {
                    return Err(anyhow!("MCP client for {} already initialized", handler.provider_name()));
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
                .with_context(|| format!("Timed out establishing {service_label} MCP transport"))??,
            None => transport_future.await?,
        };

        let initialize_result = service
            .peer()
            .peer_info()
            .ok_or_else(|| anyhow!("Handshake succeeded but server info missing"))?
            .as_ref()
            .clone();

        service
            .peer()
            .set_response_cache_config(peer_cache_config(&self.provider_name))
            .await;

        let mut guard = self.state.lock().await;
        *guard = ClientState::Ready { service: Arc::new(service) };

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

    pub(super) async fn list_all_resources(&self, timeout: Option<Duration>) -> Result<Vec<Resource>> {
        let service = self.service().await?;
        let rmcp_future = service.peer().list_all_resources();
        let resources = run_with_timeout(rmcp_future, timeout, "resources/list").await?;
        Ok(resources)
    }

    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    async fn list_all_resource_templates(&self, timeout: Option<Duration>) -> Result<Vec<ResourceTemplate>> {
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
        let result = run_with_timeout(service.peer().read_resource(params), timeout, "resources/read").await?;
        Ok(result)
    }

    pub(super) async fn get_prompt(
        &self,
        params: GetPromptRequestParams,
        timeout: Option<Duration>,
    ) -> Result<GetPromptResult> {
        let service = self.service().await?;
        let result = run_with_timeout(service.peer().get_prompt(params), timeout, "prompts/get").await?;
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

    async fn service(&self) -> Result<Arc<RunningService<RoleClient, ElicitationClientService>>> {
        let mut guard = self.state.lock().await;
        match &*guard {
            ClientState::Ready { service } => {
                // Detect if the underlying transport has died (server crash / network loss).
                if service.is_closed() {
                    warn!(provider = self.provider_name.as_str(), "MCP service closed — marking disconnected");
                    *guard = ClientState::Disconnected;
                    return Err(anyhow!("MCP client for '{}' has disconnected", self.provider_name));
                }
                Ok(service.clone())
            }
            ClientState::Connecting { .. } => Err(anyhow!("MCP client not initialized")),
            ClientState::Disconnected => Err(anyhow!("MCP client for '{}' has disconnected", self.provider_name)),
            ClientState::Stopped => Err(anyhow!("MCP client has been shut down")),
        }
    }

    /// Returns `true` when the client is in the `Ready` state and the
    /// underlying transport has not been closed.
    pub(super) async fn is_healthy(&self) -> bool {
        let guard = self.state.lock().await;
        matches!(&*guard, ClientState::Ready { service } if !service.is_closed())
    }

    pub(super) fn take_tool_list_changed(&self) -> bool {
        self.list_changed_state.take_tools_changed()
    }

    pub(super) fn take_resource_list_changed(&self) -> bool {
        self.list_changed_state.take_resources_changed()
    }

    pub(super) fn take_prompt_list_changed(&self) -> bool {
        self.list_changed_state.take_prompts_changed()
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
struct ElicitationClientService {
    handler: LoggingClientHandler,
}

impl ElicitationClientService {
    fn new(handler: LoggingClientHandler) -> Self {
        Self { handler }
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> Result<super::McpElicitationResponse, rmcp::ErrorData> {
        let request = restore_context_meta(request, context.meta);
        self.handler.process_elicitation_request(request).await
    }
}

impl Service<RoleClient> for ElicitationClientService {
    async fn handle_request(
        &self,
        request: ServerRequest,
        context: RequestContext<RoleClient>,
    ) -> Result<ClientResult, rmcp::ErrorData> {
        match request {
            ServerRequest::ElicitRequest(request) => {
                let response = self.create_elicitation(request.params, context).await?;
                Ok(ClientResult::CustomResult(elicitation_response_result(response)?))
            }
            request => {
                <LoggingClientHandler as Service<RoleClient>>::handle_request(&self.handler, request, context).await
            }
        }
    }

    async fn handle_notification(
        &self,
        notification: ServerNotification,
        context: NotificationContext<RoleClient>,
    ) -> Result<(), rmcp::ErrorData> {
        <LoggingClientHandler as Service<RoleClient>>::handle_notification(&self.handler, notification, context).await
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        <LoggingClientHandler as Service<RoleClient>>::get_info(&self.handler)
    }
}

#[derive(Clone)]
struct LoggingClientHandler {
    provider: String,
    initialize_params: InitializeRequestParams,
    elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
    list_changed_state: Arc<ListChangedState>,
}

impl LoggingClientHandler {
    fn new(
        provider_name: String,
        params: InitializeRequestParams,
        elicitation_handler: Option<Arc<dyn McpElicitationHandler>>,
        list_changed_state: Arc<ListChangedState>,
    ) -> Self {
        Self {
            provider: provider_name,
            initialize_params: params,
            elicitation_handler,
            list_changed_state,
        }
    }

    fn provider_name(&self) -> &str {
        &self.provider
    }

    async fn process_elicitation_request(
        &self,
        request: ElicitRequestParams,
    ) -> Result<super::McpElicitationResponse, rmcp::ErrorData> {
        let provider = self.provider.clone();

        let default_response = super::McpElicitationResponse {
            action: ElicitationAction::Decline,
            content: None,
            meta: None,
        };

        if let Some(handler) = &self.elicitation_handler {
            let (message, schema_value, request_meta) = match &request {
                ElicitRequestParams::FormElicitationParams { meta, message, requested_schema } => {
                    let schema_value = match serde_json::to_value(requested_schema) {
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
                    (message.clone(), schema_value, serialize_elicitation_meta(provider.as_str(), meta.as_ref()))
                }
                ElicitRequestParams::UrlElicitationParams { meta, message, url, .. } => {
                    let schema_value = json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "const": url
                            }
                        }
                    });
                    (message.clone(), schema_value, serialize_elicitation_meta(provider.as_str(), meta.as_ref()))
                }
                _ => {
                    warn!(
                        provider = provider.as_str(),
                        "Unknown elicitation request type; using default empty response"
                    );
                    ("".to_owned(), Value::Null, serialize_elicitation_meta(provider.as_str(), None))
                }
            };

            let validator = build_elicitation_validator(provider.as_str(), &schema_value);
            let payload = super::McpElicitationRequest {
                message: message.clone(),
                requested_schema: schema_value.clone(),
                meta: request_meta,
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
                    return Ok(response);
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
            let message_str = match &request {
                ElicitRequestParams::FormElicitationParams { message, .. } => message.as_str(),
                ElicitRequestParams::UrlElicitationParams { message, .. } => message.as_str(),
                _ => "unknown",
            };
            info!(
                provider = provider.as_str(),
                message = message_str,
                "MCP provider requested elicitation but no handler configured; declining"
            );
        }

        Ok(default_response)
    }

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
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
            LoggingLevel::Error | LoggingLevel::Critical | LoggingLevel::Alert | LoggingLevel::Emergency => error!(
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
        request: ElicitRequestParams,
        context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<rmcp::model::ElicitResult, rmcp::ErrorData>> + Send + '_ {
        let request = restore_context_meta(request, context.meta);
        async move {
            self.process_elicitation_request(request).await.map(|response| {
                let meta = response.meta.and_then(|value| {
                    value.as_object().cloned().map(MetaObject).or_else(|| {
                        warn!(
                            provider = self.provider.as_str(),
                            "Elicitation response meta is not an object; dropping _meta"
                        );
                        None
                    })
                });
                {
                    let mut result = rmcp::model::ElicitResult::new(response.action);
                    result.content = response.content;
                    result.meta = meta;
                    result
                }
            })
        }
    }

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn list_roots(
        &self,
        _context: RequestContext<RoleClient>,
    ) -> impl Future<Output = Result<ListRootsResult, rmcp::ErrorData>> + Send + '_ {
        let provider = self.provider.clone();
        async move {
            let mut roots = Vec::new();
            match std::env::current_dir() {
                Ok(dir) => {
                    if let Some(uri) = directory_to_file_uri(&dir) {
                        roots.push(Root::new(uri).with_name("workspace"));
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

            Ok(ListRootsResult::new(roots))
        }
    }

    fn on_cancelled(
        &self,
        params: CancelledNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        debug!(
            provider = self.provider.as_str(),
            request_id = ?params.request_id,
            reason = ?params.reason,
            "MCP provider cancelled request"
        );
        async move {}
    }

    fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
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

    #[allow(
        deprecated,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn on_logging_message(
        &self,
        params: LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.handle_logging(params);
        async move {}
    }

    fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        info!(provider = self.provider.as_str(), uri = params.uri.as_str(), "MCP resource updated");
        async move {}
    }

    fn on_resource_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_resources_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported resource list change");
        async move {}
    }

    fn on_tool_list_changed(&self, _context: NotificationContext<RoleClient>) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_tools_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported tool list change");
        async move {}
    }

    fn on_prompt_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.list_changed_state.mark_prompts_changed();
        info!(provider = self.provider.as_str(), "MCP provider reported prompt list change");
        async move {}
    }

    fn get_info(&self) -> rmcp::model::ClientConfig {
        convert_to_rmcp(self.initialize_params.clone()).unwrap_or_else(|error| {
            warn!(
                provider = self.provider.as_str(),
                error = %error,
                "Failed to convert MCP initialize params; using fallback client info"
            );
            rmcp::model::ClientConfig::new(Default::default(), super::utils::build_client_implementation())
        })
    }
}

fn restore_context_meta(mut request: ElicitRequestParams, mut context_meta: RequestMetaObject) -> ElicitRequestParams {
    drop(context_meta.remove(MCP_PROGRESS_TOKEN_META_KEY));
    if context_meta.is_empty() {
        return request;
    }

    match &mut request {
        ElicitRequestParams::FormElicitationParams { meta, .. }
        | ElicitRequestParams::UrlElicitationParams { meta, .. } => {
            meta.get_or_insert_with(RequestMetaObject::new).extend(context_meta);
        }
        _ => {}
    }

    request
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateElicitationResultWithMeta {
    action: ElicitationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    meta: Option<Value>,
}

fn elicitation_response_result(response: super::McpElicitationResponse) -> Result<CustomResult, rmcp::ErrorData> {
    let result = CreateElicitationResultWithMeta {
        action: response.action,
        content: response.content,
        meta: response.meta,
    };

    serde_json::to_value(result)
        .map(CustomResult)
        .map_err(|err| rmcp::ErrorData::internal_error(err.to_string(), None))
}

fn serialize_elicitation_meta(provider: &str, meta: Option<&RequestMetaObject>) -> Option<Value> {
    meta.and_then(|meta| match serde_json::to_value(meta) {
        Ok(value) => Some(value),
        Err(err) => {
            warn!(
                provider = provider,
                error = %err,
                "Failed to serialize MCP elicitation metadata; dropping _meta"
            );
            None
        }
    })
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
        warn!(provider = provider, "MCP elicitation accept action missing response content");
        return Err(rmcp::ErrorData::invalid_params("Elicitation response missing content for accept action", None));
    };

    if !validator.is_valid(payload) {
        let messages: Vec<String> = validator.iter_errors(payload).map(|err| err.to_string()).collect();
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
    Url::from_directory_path(path).ok().map(|url| url.to_string())
}

async fn run_with_timeout<F, T>(fut: F, timeout: Option<Duration>, label: &str) -> Result<T>
where
    F: Future<Output = Result<T, service::ServiceError>>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{BooleanSchema, ElicitationSchema, PrimitiveSchemaDefinition};

    #[test]
    fn peer_cache_config_is_bounded_fail_closed_and_partitioned() {
        let config = peer_cache_config("deepwiki");
        assert!(config.enabled);
        assert_eq!(config.max_entries, 128);
        assert!(!config.serve_stale_on_error);
        assert_eq!(config.private_partition.as_deref(), Some("deepwiki"));
    }

    #[test]
    fn discover_preferred_versions_exclude_unsupported_draft_and_remain_newest_first() {
        let versions: Vec<&str> = DISCOVER_PREFERRED_VERSIONS
            .iter()
            .map(rmcp::model::ProtocolVersion::as_str)
            .collect();

        assert_eq!(versions, vec!["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05",]);
        assert!(!versions.contains(&"2026-07-28"));
    }

    #[test]
    fn clamp_initialize_protocol_version_caps_draft_at_last_stable() {
        assert_eq!(
            clamp_initialize_protocol_version(rmcp::model::ProtocolVersion::V_2026_07_28),
            rmcp::model::ProtocolVersion::V_2025_11_25
        );
    }

    #[test]
    fn clamp_initialize_protocol_version_caps_unknown_future_versions() {
        let future: rmcp::model::ProtocolVersion = serde_json::from_value(Value::String("2099-01-01".to_string()))
            .expect("custom protocol versions must deserialize");
        assert_eq!(clamp_initialize_protocol_version(future), rmcp::model::ProtocolVersion::V_2025_11_25);
    }

    #[test]
    fn clamp_initialize_protocol_version_keeps_supported_versions() {
        for version in [
            rmcp::model::ProtocolVersion::V_2025_11_25,
            rmcp::model::ProtocolVersion::V_2025_06_18,
            rmcp::model::ProtocolVersion::V_2025_03_26,
            rmcp::model::ProtocolVersion::V_2024_11_05,
        ] {
            assert_eq!(clamp_initialize_protocol_version(version.clone()), version);
        }
    }

    #[test]
    fn restore_context_meta_adds_request_meta_and_removes_progress_token() {
        let request = form_request(Some(meta(json!({ "existing": true }))));
        let restored = restore_context_meta(
            request,
            meta(json!({
                "persist": "always",
                "progressToken": "token-1"
            })),
        );

        let ElicitRequestParams::FormElicitationParams { meta, .. } = restored else {
            panic!("expected form elicitation request");
        };

        assert_eq!(
            serde_json::to_value(meta.expect("meta should be present")).expect("meta should serialize"),
            json!({
                "existing": true,
                "persist": "always"
            })
        );
    }

    #[test]
    fn elicitation_response_result_serializes_response_meta() {
        let result = ClientResult::CustomResult(
            elicitation_response_result(super::super::McpElicitationResponse {
                action: ElicitationAction::Accept,
                content: Some(json!({ "confirmed": true })),
                meta: Some(json!({ "persist": "always" })),
            })
            .expect("elicitation response should serialize"),
        );

        assert_eq!(
            serde_json::to_value(result).expect("client result should serialize"),
            json!({
                "action": "accept",
                "content": { "confirmed": true },
                "_meta": { "persist": "always" }
            })
        );
    }

    #[test]
    fn list_changed_state_consumes_signals_once() {
        let state = ListChangedState::default();

        assert!(!state.take_tools_changed());
        assert!(!state.take_resources_changed());
        assert!(!state.take_prompts_changed());

        state.mark_tools_changed();
        state.mark_resources_changed();
        state.mark_prompts_changed();

        assert!(state.take_tools_changed());
        assert!(state.take_resources_changed());
        assert!(state.take_prompts_changed());

        assert!(!state.take_tools_changed());
        assert!(!state.take_resources_changed());
        assert!(!state.take_prompts_changed());
    }

    #[test]
    fn list_changed_state_coalesces_bursts_and_bounds_repeated_refreshes() {
        let state = ListChangedState::default();
        let now = std::time::Instant::now();

        for _ in 0..LIST_CHANGED_BUCKET_CAPACITY {
            state.mark_changed(ListChangedNamespace::Tools, now);
            assert!(state.take_changed(ListChangedNamespace::Tools, now));
        }

        // The fifth notification in the same refill interval is suppressed.
        state.mark_changed(ListChangedNamespace::Tools, now);
        assert!(!state.take_changed(ListChangedNamespace::Tools, now));

        // Buckets are independent per notification namespace.
        state.mark_changed(ListChangedNamespace::Resources, now);
        assert!(state.take_changed(ListChangedNamespace::Resources, now));
    }

    #[test]
    fn list_changed_state_refills_after_cooldown() {
        let state = ListChangedState::default();
        let now = std::time::Instant::now();

        for _ in 0..LIST_CHANGED_BUCKET_CAPACITY {
            state.mark_changed(ListChangedNamespace::Prompts, now);
            assert!(state.take_changed(ListChangedNamespace::Prompts, now));
        }
        state.mark_changed(ListChangedNamespace::Prompts, now);
        assert!(!state.take_changed(ListChangedNamespace::Prompts, now));
        assert!(state.take_changed(ListChangedNamespace::Prompts, now + LIST_CHANGED_REFILL_INTERVAL));
    }

    fn form_request(meta: Option<RequestMetaObject>) -> ElicitRequestParams {
        ElicitRequestParams::FormElicitationParams {
            meta,
            message: "Confirm?".to_string(),
            requested_schema: ElicitationSchema::builder()
                .required_property("confirmed", PrimitiveSchemaDefinition::Boolean(BooleanSchema::new()))
                .build()
                .expect("schema should build"),
        }
    }

    fn meta(value: Value) -> RequestMetaObject {
        let Value::Object(map) = value else {
            panic!("meta must be an object");
        };
        RequestMetaObject(MetaObject(map))
    }

    /// Regression test for CVE-2026-64684 (GHSA-9g45-5xwm-f3wc): the MCP
    /// streamable-HTTP client must not follow redirects, otherwise
    /// caller-supplied custom headers (API keys) leak to the redirect target.
    #[tokio::test]
    async fn streamable_http_client_does_not_follow_redirects_with_custom_headers() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        use rmcp_reqwest::header::{HeaderMap, HeaderValue};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
            let mut buf = Vec::new();
            let mut chunk = [0_u8; 1024];
            while let Ok(n) = stream.read(&mut chunk).await {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > 65536 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf);
                let Some(header_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let Some(header_text) = text.get(..header_end) else {
                    continue;
                };
                let content_length = header_text
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= header_end + 4 + content_length {
                    break;
                }
            }
            buf
        }

        // Arrange: attacker-controlled capture endpoint. Records hits and
        // whether the custom auth header arrived.
        let hits = Arc::new(AtomicUsize::new(0));
        let leaked = Arc::new(AtomicUsize::new(0));
        let capture_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind capture listener");
        let capture_addr = capture_listener.local_addr().expect("capture addr");
        let capture_task = tokio::spawn({
            let hits = Arc::clone(&hits);
            let leaked = Arc::clone(&leaked);
            async move {
                if let Ok((mut stream, _)) = capture_listener.accept().await {
                    let raw = read_http_request(&mut stream).await;
                    let _ = hits.fetch_add(1, Ordering::SeqCst);
                    if String::from_utf8_lossy(&raw).contains("x-api-key:") {
                        let _ = leaked.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });

        // Arrange: compromised MCP endpoint answering 307 to the capture host.
        let redirect_listener = TcpListener::bind("127.0.0.1:0").await.expect("bind redirect listener");
        let redirect_addr = redirect_listener.local_addr().expect("redirect addr");
        let redirect_task = tokio::spawn(async move {
            if let Ok((mut stream, _)) = redirect_listener.accept().await {
                let _ = read_http_request(&mut stream).await;
                let response = format!(
                    "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://{capture_addr}/mcp\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                drop(stream.write_all(response.as_bytes()).await);
            }
        });

        // Act: initialize against the redirecting endpoint with a secret header.
        let mut headers = HeaderMap::new();
        drop(headers.insert("x-api-key", HeaderValue::from_static("super-secret")));
        let client = RmcpClient::new_streamable_http_client(
            "redirect-probe".to_string(),
            &format!("http://{redirect_addr}/mcp"),
            None,
            headers,
            None,
        )
        .await
        .expect("client builds");
        let params = InitializeRequestParams::new(
            rmcp::model::ClientCapabilities::default(),
            super::super::utils::build_client_implementation(),
        );
        let result = client
            .initialize(params, Some(Duration::from_secs(10)), ClientLifecycleMode::Initialize)
            .await;

        redirect_task.await.expect("redirect server completes");
        capture_task.abort();

        // Assert: handshake surfaces the 307 as a transport error and the
        // redirect target is never contacted, so the secret cannot leak.
        assert!(result.is_err(), "307 must surface as an error, got {result:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 0, "redirect target must never be contacted");
        assert_eq!(leaked.load(Ordering::SeqCst), 0, "custom auth header must never reach redirect target");
    }
}
