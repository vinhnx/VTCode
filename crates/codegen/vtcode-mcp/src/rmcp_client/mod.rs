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
mod elicitation;

use elicitation::{ElicitationClientService, LoggingClientHandler};

pub(crate) use elicitation::{build_elicitation_validator, directory_to_file_uri, validate_elicitation_payload};

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
        // Bound the TCP/TLS handshake from the installed `[timeouts]` budget.
        // No transport-level total timeout by design: every operation is
        // wrapped in `run_with_timeout` with its tool-appropriate budget, and
        // a total here would abort legitimately long SSE-streamed tool calls.
        if let Some(connect) = vtcode_commons::http::http_timeouts().connect {
            client_builder = client_builder.connect_timeout(connect);
        }

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
mod tests;
