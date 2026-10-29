//! Shared MCP tools and local stdio/authenticated HTTP transports, read-only by default.
//!
//! The remote surface deliberately shares the [`RuntimeAdapter`] read boundary
//! with the browser bridge, but it has an independent authentication and Origin
//! policy. A TLS-terminating reverse proxy is expected to validate the external
//! OAuth bearer token and inject the configured internal bearer token.

use crate::companion::{CompanionTasks, StartTaskInput, TaskInput, TaskSnapshot};
use crate::error::{Result as WebmcpResult, WebmcpError};
use crate::runtime::RuntimeAdapter;
use axum::Router;
use axum::body::{self, Body};
use axum::extract::{Path, Request, State};
use axum::http::header::{
    ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN, PROXY_AUTHORIZATION, WWW_AUTHENTICATE,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Json as AxumJson, Response};
use axum::routing::{get, post};
use futures::stream;
use futures::{Sink, Stream, StreamExt};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ClientJsonRpcMessage, Implementation, ServerCapabilities, ServerConfig, ServerJsonRpcMessage, Tool};
use rmcp::transport::sink_stream::SinkStreamTransport;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{Json, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, mpsc};
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::{CancellationToken, DropGuard};
use url::Url;
use uuid::Uuid;
use vtcode_commons::validation::is_valid_origin;

const DEFAULT_MAX_RESULTS: usize = 20;
const DEFAULT_MAX_SCAN_FILES: usize = 256;
const DEFAULT_MAX_SCAN_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(300);
const MAX_LEGACY_SESSIONS: usize = 64;
const LEGACY_INPUT_QUEUE_CAPACITY: usize = 16;
const MAX_QUERY_BYTES: usize = 4096;
const MAX_FILE_ID_BYTES: usize = 4096;
const LEGACY_KEEP_ALIVE: Duration = Duration::from_secs(15);
const PROTECTED_RESOURCE_METADATA_PATH: &str = "/.well-known/oauth-protected-resource";

mod types;

pub use types::{FetchInput, FetchOutput, SearchInput, SearchOutput, SearchResult};

/// Runtime configuration for the remote MCP transport.
///
/// The proxy bearer token is intentionally private and is omitted from the
/// [`Debug`] implementation. It must never be copied into a URL or persisted.
#[derive(Clone)]
pub struct RemoteMcpServerConfig {
    /// Canonical external HTTPS URL, normally the legacy `/sse/` endpoint.
    pub public_url: Url,
    /// External authorization server URL advertised in protected-resource metadata.
    pub authorization_server: Url,
    proxy_bearer_token: Arc<str>,
    /// Optional URL prefix for escaped file citations.
    pub citation_url_prefix: Option<Url>,
    /// Separate allowlist for supplied MCP Origin headers.
    pub allowed_origins: Vec<String>,
    /// Maximum number of search results.
    pub max_results: usize,
    /// Maximum files inspected by search.
    pub max_scan_files: usize,
    /// Maximum UTF-8 content bytes inspected by search.
    pub max_scan_bytes: usize,
    /// Maximum legacy MCP request body size.
    pub max_request_body_bytes: usize,
    /// Legacy SSE session inactivity lifetime.
    pub session_ttl: Duration,
    /// Optional binary-owned execution service. Absent keeps the server read-only.
    pub companion: Option<Arc<dyn CompanionTasks>>,
}

impl std::fmt::Debug for RemoteMcpServerConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteMcpServerConfig")
            .field("public_url", &self.public_url)
            .field("authorization_server", &self.authorization_server)
            .field("proxy_bearer_token", &"<redacted>")
            .field("citation_url_prefix", &self.citation_url_prefix)
            .field("allowed_origins", &self.allowed_origins)
            .field("max_results", &self.max_results)
            .field("max_scan_files", &self.max_scan_files)
            .field("max_scan_bytes", &self.max_scan_bytes)
            .field("max_request_body_bytes", &self.max_request_body_bytes)
            .field("session_ttl", &self.session_ttl)
            .finish()
    }
}

impl RemoteMcpServerConfig {
    /// Create a validated remote MCP configuration with the default bounds.
    pub fn new(
        public_url: impl AsRef<str>,
        authorization_server: impl AsRef<str>,
        proxy_bearer_token: impl Into<String>,
    ) -> WebmcpResult<Self> {
        let public_url = Url::parse(public_url.as_ref())
            .map_err(|_error| WebmcpError::InvalidRequest("remote MCP public URL is invalid".to_string()))?;
        let authorization_server = Url::parse(authorization_server.as_ref()).map_err(|_error| {
            WebmcpError::InvalidRequest("remote MCP authorization-server URL is invalid".to_string())
        })?;
        let proxy_bearer_token: Arc<str> = proxy_bearer_token.into().into();
        let config = Self {
            public_url,
            authorization_server,
            proxy_bearer_token,
            citation_url_prefix: None,
            allowed_origins: Vec::new(),
            max_results: DEFAULT_MAX_RESULTS,
            max_scan_files: DEFAULT_MAX_SCAN_FILES,
            max_scan_bytes: DEFAULT_MAX_SCAN_BYTES,
            max_request_body_bytes: DEFAULT_MAX_REQUEST_BODY_BYTES,
            session_ttl: DEFAULT_SESSION_TTL,
            companion: None,
        };
        config.validate()?;
        Ok(config)
    }

    /// Validate URL, authentication, and resource bounds.
    pub fn validate(&self) -> WebmcpResult<()> {
        if !is_valid_https_url(&self.public_url) {
            return Err(WebmcpError::InvalidRequest(
                "remote MCP public URL must be absolute HTTPS without credentials, query, or fragment".to_string(),
            ));
        }
        if !is_valid_https_url(&self.authorization_server) {
            return Err(WebmcpError::InvalidRequest(
                "remote MCP authorization-server URL must be absolute HTTPS without credentials, query, or fragment"
                    .to_string(),
            ));
        }
        if self.proxy_bearer_token.is_empty()
            || !self.proxy_bearer_token.is_ascii()
            || self.proxy_bearer_token.chars().any(|character| character.is_ascii_whitespace())
        {
            return Err(WebmcpError::InvalidRequest(
                "remote MCP proxy bearer token must be a non-empty ASCII token".to_string(),
            ));
        }
        if let Some(prefix) = self.citation_url_prefix.as_ref()
            && !is_valid_citation_url_prefix(prefix)
        {
            return Err(WebmcpError::InvalidRequest(
                "remote MCP citation URL prefix must be an absolute HTTP(S) URL without credentials, query, or fragment"
                    .to_string(),
            ));
        }
        if self.allowed_origins.iter().any(|origin| !is_valid_origin(origin)) {
            return Err(WebmcpError::InvalidRequest("remote MCP origins must be explicit HTTP(S) origins".to_string()));
        }
        if self.max_results == 0 || self.max_results > 100 {
            return Err(WebmcpError::LimitExceeded);
        }
        if self.max_scan_files == 0 || self.max_scan_files > 4096 {
            return Err(WebmcpError::LimitExceeded);
        }
        if self.max_scan_bytes == 0 || self.max_scan_bytes > 64 * 1024 * 1024 {
            return Err(WebmcpError::LimitExceeded);
        }
        if self.max_request_body_bytes == 0 || self.max_request_body_bytes > 16 * 1024 * 1024 {
            return Err(WebmcpError::LimitExceeded);
        }
        if self.session_ttl.is_zero() || self.session_ttl > Duration::from_secs(3600) {
            return Err(WebmcpError::InvalidRequest(
                "remote MCP session TTL must be between 1 and 3600 seconds".to_string(),
            ));
        }
        Ok(())
    }

    /// Set the citation prefix after parsing it as a URL.
    pub fn with_citation_url_prefix(mut self, prefix: Option<Url>) -> WebmcpResult<Self> {
        self.citation_url_prefix = prefix;
        self.validate()?;
        Ok(self)
    }

    /// Set the separate MCP Origin allowlist.
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> WebmcpResult<Self> {
        self.allowed_origins = origins;
        self.validate()?;
        Ok(self)
    }

    /// Set the search and legacy transport bounds.
    pub fn with_limits(
        mut self,
        max_results: usize,
        max_scan_files: usize,
        max_scan_bytes: usize,
        max_request_body_bytes: usize,
        session_ttl: Duration,
    ) -> WebmcpResult<Self> {
        self.max_results = max_results;
        self.max_scan_files = max_scan_files;
        self.max_scan_bytes = max_scan_bytes;
        self.max_request_body_bytes = max_request_body_bytes;
        self.session_ttl = session_ttl;
        self.validate()?;
        Ok(self)
    }

    fn proxy_bearer_token(&self) -> &str {
        &self.proxy_bearer_token
    }

    fn metadata_url(&self) -> Url {
        let mut metadata_url = self.public_url.clone();
        metadata_url.set_path(PROTECTED_RESOURCE_METADATA_PATH);
        metadata_url.set_query(None);
        metadata_url.set_fragment(None);
        metadata_url
    }
}

fn is_valid_https_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn is_valid_citation_url_prefix(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

/// MCP handler backed by workspace reads and an optional binary-owned task service.
#[derive(Clone)]
pub struct RemoteMcpHandler {
    adapter: Arc<dyn RuntimeAdapter>,
    max_results: usize,
    max_scan_files: usize,
    max_scan_bytes: usize,
    citation_url_prefix: Option<Url>,
    companion: Option<Arc<dyn CompanionTasks>>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for RemoteMcpHandler {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteMcpHandler")
            .field("max_results", &self.max_results)
            .field("companion_enabled", &self.companion.is_some())
            .finish_non_exhaustive()
    }
}

#[tool_router]
impl RemoteMcpHandler {
    /// Create a handler around the shared read-only runtime boundary.
    pub fn new(adapter: Arc<dyn RuntimeAdapter>, config: Arc<RemoteMcpServerConfig>) -> Self {
        let mut handler = Self::local(adapter, config.companion.clone());
        handler.max_results = config.max_results;
        handler.max_scan_files = config.max_scan_files;
        handler.max_scan_bytes = config.max_scan_bytes;
        handler.citation_url_prefix = config.citation_url_prefix.clone();
        handler
    }

    /// Create a local handler with default read bounds and no HTTP authentication state.
    /// Access is controlled by the owning process and the workspace execution policy.
    pub fn local(adapter: Arc<dyn RuntimeAdapter>, companion: Option<Arc<dyn CompanionTasks>>) -> Self {
        let mut tool_router = Self::tool_router();
        if companion.is_none() {
            for name in ["start_task", "get_task", "cancel_task"] {
                tool_router.remove_route(name);
            }
        }
        Self {
            adapter,
            max_results: DEFAULT_MAX_RESULTS,
            max_scan_files: DEFAULT_MAX_SCAN_FILES,
            max_scan_bytes: DEFAULT_MAX_SCAN_BYTES,
            citation_url_prefix: None,
            companion,
            tool_router,
        }
    }

    /// Serve local MCP messages until EOF or cancellation. Stdout is protocol-only.
    pub async fn serve_stdio(self, cancellation: CancellationToken) -> WebmcpResult<()> {
        let service = self
            .serve_with_ct(rmcp::transport::stdio(), cancellation)
            .await
            .map_err(|error| WebmcpError::Adapter(format!("local MCP initialization failed: {error}")))?;
        let reason = service
            .waiting()
            .await
            .map_err(|error| WebmcpError::Adapter(format!("local MCP service stopped: {error}")))?;
        if let rmcp::service::QuitReason::JoinError(error) = reason {
            return Err(WebmcpError::Adapter(format!("local MCP worker failed: {error}")));
        }
        Ok(())
    }

    /// Return the exact tool definitions advertised by this handler.
    pub fn tool_definitions(&self) -> Vec<Tool> {
        self.tool_router.list_all()
    }

    /// Search visible workspace files by deterministic case-insensitive substring.
    #[tool(
        name = "search",
        description = "Search visible UTF-8 workspace files by case-insensitive substring.",
        annotations(
            title = "Search workspace files",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn search(&self, Parameters(input): Parameters<SearchInput>) -> Result<Json<SearchOutput>, String> {
        let query = input.query.trim();
        if query.is_empty() {
            return Ok(Json(SearchOutput { results: Vec::new() }));
        }
        if query.len() > MAX_QUERY_BYTES {
            return Err("search query exceeds the configured input limit".to_string());
        }
        let query = query.to_lowercase();
        let mut files = self
            .adapter
            .list_files()
            .await
            .map_err(|_error| "workspace search is unavailable".to_string())?;
        files.sort_unstable_by(|left, right| left.path.cmp(&right.path));

        let mut results = Vec::with_capacity(self.max_results.min(DEFAULT_MAX_RESULTS));
        let mut scanned_bytes = 0usize;
        for file in files.into_iter().take(self.max_scan_files) {
            let path_lower = file.path.to_lowercase();
            let path_matches = path_lower.contains(&query);
            if path_matches {
                results.push(self.search_result(&file.path));
                if results.len() >= self.max_results {
                    break;
                }
                continue;
            }

            let declared_size = match usize::try_from(file.size_bytes) {
                Ok(size) => size,
                Err(_) => continue,
            };
            let remaining_bytes = self.max_scan_bytes.saturating_sub(scanned_bytes);
            if declared_size > remaining_bytes {
                continue;
            }
            let snapshot = match self.adapter.read_file(&file.path).await {
                Ok(snapshot) if snapshot.path == file.path => snapshot,
                Ok(_) | Err(_) => continue,
            };
            if snapshot.content.len() > remaining_bytes {
                continue;
            }
            scanned_bytes = scanned_bytes.saturating_add(snapshot.content.len());
            if snapshot.content.to_lowercase().contains(&query) {
                results.push(self.search_result(&file.path));
                if results.len() >= self.max_results {
                    break;
                }
            }
        }
        Ok(Json(SearchOutput { results }))
    }

    /// Fetch one visible workspace file by the ID returned from `search`.
    #[tool(
        name = "fetch",
        description = "Fetch the UTF-8 text of one visible workspace file by ID.",
        annotations(
            title = "Fetch workspace file",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn fetch(&self, Parameters(input): Parameters<FetchInput>) -> Result<Json<FetchOutput>, String> {
        let id = input.id.as_str();
        if id.trim().is_empty() {
            return Err("file id is required".to_string());
        }
        if id.len() > MAX_FILE_ID_BYTES {
            return Err("file id exceeds the configured input limit".to_string());
        }
        let visible = self
            .adapter
            .list_files()
            .await
            .map_err(|_error| "workspace file listing is unavailable".to_string())?;
        if !visible.iter().any(|file| file.path == id) {
            return Err("requested workspace file is unavailable".to_string());
        }
        let snapshot = self
            .adapter
            .read_file(id)
            .await
            .map_err(|_error| "requested workspace file is unavailable".to_string())?;
        if snapshot.path != id {
            return Err("requested workspace file is unavailable".to_string());
        }
        let canonical_id = snapshot.path;
        Ok(Json(FetchOutput {
            id: canonical_id.clone(),
            title: canonical_id.clone(),
            text: snapshot.content,
            url: self.citation_url(&canonical_id),
            metadata: None,
        }))
    }

    /// Start a plan or explicitly requested implementation within the configured workspace.
    #[tool(
        name = "start_task",
        description = "Start a VT Code task. Defaults to read-only planning. Implementation requires a completed plan_task_id and an explicit user request.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = true
        )
    )]
    pub async fn start_task(
        &self,
        Parameters(input): Parameters<StartTaskInput>,
    ) -> Result<Json<TaskSnapshot>, String> {
        self.companion()?.start(input).await.map(Json)
    }

    /// Return bounded, public task evidence.
    #[tool(
        name = "get_task",
        description = "Inspect task state, attributed changes, scoped runtime verification, and bounded historical command_results. Command exit zero does not establish unrelated task assertions. Missing proof remains unverified.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn get_task(&self, Parameters(input): Parameters<TaskInput>) -> Result<Json<TaskSnapshot>, String> {
        self.companion()?.get(&input.task_id).await.map(Json)
    }

    /// Cancel only a task owned by this service instance.
    #[tool(
        name = "cancel_task",
        description = "Request cancellation of an owned VT Code task. Poll get_task until terminal cleanup completes.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub async fn cancel_task(&self, Parameters(input): Parameters<TaskInput>) -> Result<Json<bool>, String> {
        self.companion()?.cancel(&input.task_id).await.map(Json)
    }

    fn companion(&self) -> Result<&dyn CompanionTasks, String> {
        self.companion
            .as_deref()
            .ok_or_else(|| "companion execution is disabled".into())
    }

    fn search_result(&self, path: &str) -> SearchResult {
        SearchResult {
            id: path.to_string(),
            title: path.to_string(),
            url: self.citation_url(path),
        }
    }

    fn citation_url(&self, path: &str) -> String {
        let Some(prefix) = self.citation_url_prefix.as_ref() else {
            return String::new();
        };
        let mut url = prefix.clone();
        if let Ok(mut segments) = url.path_segments_mut() {
            let _ = segments.pop_if_empty();
            for segment in path.split('/') {
                let _ = segments.push(segment);
            }
        }
        url.to_string()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RemoteMcpHandler {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vtcode-webmcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                if self.companion.is_some() {
                    "This server exposes workspace search/fetch and opt-in VT Code tasks. Plan first; implement only on explicit user request. Poll task results and report verification evidence accurately."
                } else {
                    "This server exposes only read-only search and fetch tools over the configured workspace."
                },
            )
    }
}

/// Remote MCP HTTP endpoint collection mounted by [`crate::WebmcpServer`].
pub struct RemoteMcpEndpoint {
    config: Arc<RemoteMcpServerConfig>,
    handler: RemoteMcpHandler,
    streamable: StreamableHttpService<RemoteMcpHandler, LocalSessionManager>,
    legacy_sessions: LegacySessionStore,
}

impl std::fmt::Debug for RemoteMcpEndpoint {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RemoteMcpEndpoint")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl RemoteMcpEndpoint {
    /// Construct the modern Streamable HTTP and legacy HTTP+SSE services.
    pub fn new(adapter: Arc<dyn RuntimeAdapter>, config: RemoteMcpServerConfig) -> WebmcpResult<Self> {
        config.validate()?;
        let config = Arc::new(config);
        let service_config = StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_legacy_session_mode(false)
            .with_allowed_hosts(allowed_hosts(&config.public_url))
            .with_allowed_origins(config.allowed_origins.clone())
            .with_max_request_body_bytes(config.max_request_body_bytes);
        let handler = RemoteMcpHandler::new(Arc::clone(&adapter), Arc::clone(&config));
        let handler_adapter = Arc::clone(&adapter);
        let handler_config = Arc::clone(&config);
        let streamable = StreamableHttpService::new(
            move || Ok(RemoteMcpHandler::new(Arc::clone(&handler_adapter), Arc::clone(&handler_config))),
            Arc::new(LocalSessionManager::default()),
            service_config,
        );
        Ok(Self {
            legacy_sessions: LegacySessionStore::new(config.session_ttl),
            config,
            handler,
            streamable,
        })
    }

    /// Return the public configuration without exposing the proxy token.
    pub fn config(&self) -> &RemoteMcpServerConfig {
        &self.config
    }

    /// Return the advertised tools (read-only unless companion is explicitly configured).
    pub fn tool_definitions(&self) -> Vec<Tool> {
        self.handler.tool_definitions()
    }

    /// Build routes before the containing WebMCP server supplies its state.
    pub fn routes(self: &Arc<Self>) -> Router<Arc<Self>> {
        Router::new()
            .nest_service("/mcp", self.streamable.clone())
            .route("/sse/", get(legacy_sse_handler))
            .route("/sse", get(legacy_sse_handler))
            .route("/messages/{session_id}", post(legacy_message_handler))
            .route(PROTECTED_RESOURCE_METADATA_PATH, get(protected_resource_metadata))
            .layer(middleware::from_fn_with_state(self.clone(), authenticate_request))
    }

    async fn create_legacy_session(
        &self,
    ) -> WebmcpResult<(
        String,
        mpsc::Receiver<ClientJsonRpcMessage>,
        mpsc::Receiver<ServerJsonRpcMessage>,
        mpsc::Sender<ServerJsonRpcMessage>,
        CancellationToken,
    )> {
        self.legacy_sessions.create().await
    }

    async fn legacy_sender(&self, session_id: &str) -> Option<mpsc::Sender<ClientJsonRpcMessage>> {
        self.legacy_sessions.sender(session_id).await
    }

    fn authentication_failure(&self, request: &Request<Body>, metadata: bool) -> Option<Response> {
        if !host_is_allowed(request.headers(), &self.config.public_url) {
            return Some((StatusCode::FORBIDDEN, "MCP Host header is not allowed").into_response());
        }
        match request.headers().get(ORIGIN) {
            None => {}
            Some(origin) => {
                let Ok(origin) = origin.to_str() else {
                    return Some((StatusCode::BAD_REQUEST, "invalid MCP Origin header").into_response());
                };
                if !self.config.allowed_origins.iter().any(|allowed| allowed == origin) {
                    return Some((StatusCode::FORBIDDEN, "MCP Origin is not allowed").into_response());
                }
            }
        }
        if metadata {
            return None;
        }
        let authorized = request
            .headers()
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split_once(' '))
            .is_some_and(|(scheme, token)| {
                scheme.eq_ignore_ascii_case("Bearer") && token == self.config.proxy_bearer_token()
            });
        if authorized {
            None
        } else {
            Some(self.unauthorized_response())
        }
    }

    fn unauthorized_response(&self) -> Response {
        let metadata_url = self.config.metadata_url().to_string();
        let challenge = format!("Bearer resource_metadata=\"{}\"", quote_header_value(&metadata_url));
        let mut response = (StatusCode::UNAUTHORIZED, "MCP authentication required").into_response();
        if let Ok(value) = HeaderValue::from_str(&challenge) {
            let _ = response.headers_mut().insert(WWW_AUTHENTICATE, value);
        }
        response
    }
}

async fn authenticate_request(
    State(endpoint): State<Arc<RemoteMcpEndpoint>>,
    mut request: Request,
    next: Next,
) -> Response {
    let metadata = request.uri().path() == PROTECTED_RESOURCE_METADATA_PATH;
    if let Some(response) = endpoint.authentication_failure(&request, metadata) {
        return response;
    }
    let _ = request.headers_mut().remove(AUTHORIZATION);
    let _ = request.headers_mut().remove(PROXY_AUTHORIZATION);
    next.run(request).await
}

async fn protected_resource_metadata(State(endpoint): State<Arc<RemoteMcpEndpoint>>) -> Response {
    let body = serde_json::json!({
        "resource": endpoint.config.public_url.as_str(),
        "authorization_servers": [endpoint.config.authorization_server.as_str()],
        "bearer_methods_supported": ["header"],
    });
    let mut response = AxumJson(body).into_response();
    let _ = response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn legacy_sse_handler(State(endpoint): State<Arc<RemoteMcpEndpoint>>, headers: HeaderMap) -> Response {
    let accepts_sse = headers.get(ACCEPT).and_then(|value| value.to_str().ok()).is_some_and(|value| {
        value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("text/event-stream"))
    });
    if !accepts_sse {
        return (StatusCode::NOT_ACCEPTABLE, "legacy MCP SSE requires Accept: text/event-stream").into_response();
    }
    let (session_id, input_receiver, output_receiver, output_sender, cancellation) =
        match endpoint.create_legacy_session().await {
            Ok(session) => session,
            Err(WebmcpError::LimitExceeded) => {
                return (StatusCode::TOO_MANY_REQUESTS, "MCP session limit reached").into_response();
            }
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "MCP session creation failed").into_response(),
        };
    spawn_legacy_server(endpoint.handler.clone(), input_receiver, output_sender, cancellation.clone());
    let endpoint_event = Event::default().event("endpoint").data(format!("/messages/{session_id}"));
    // Cancelling the token makes the session's expiry loop remove it; no
    // async work is spawned from Drop.
    let guard = cancellation.drop_guard();
    let message_stream = legacy_event_stream(output_receiver, guard, endpoint_event);
    Sse::new(message_stream)
        .keep_alive(KeepAlive::new().interval(LEGACY_KEEP_ALIVE).text("keep-alive"))
        .into_response()
}

async fn legacy_message_handler(
    State(endpoint): State<Arc<RemoteMcpEndpoint>>,
    Path(session_id): Path<String>,
    request: Request,
) -> Response {
    if request.method() != Method::POST {
        return (StatusCode::METHOD_NOT_ALLOWED, "MCP messages require POST").into_response();
    }
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
    if !content_type {
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "MCP messages require application/json").into_response();
    }
    let body = match body::to_bytes(request.into_body(), endpoint.config.max_request_body_bytes).await {
        Ok(body) => body,
        Err(_) => return (StatusCode::PAYLOAD_TOO_LARGE, "MCP request body is too large").into_response(),
    };
    let message = match serde_json::from_slice::<ClientJsonRpcMessage>(&body) {
        Ok(message) => message,
        Err(_) => return (StatusCode::BAD_REQUEST, "invalid MCP JSON-RPC message").into_response(),
    };
    let Some(sender) = endpoint.legacy_sender(&session_id).await else {
        return (StatusCode::NOT_FOUND, "MCP session is unknown or expired").into_response();
    };
    match sender.try_send(message) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(mpsc::error::TrySendError::Full(_)) => {
            (StatusCode::TOO_MANY_REQUESTS, "MCP session input queue is full").into_response()
        }
        Err(mpsc::error::TrySendError::Closed(_)) => (StatusCode::NOT_FOUND, "MCP session is closed").into_response(),
    }
}

fn allowed_hosts(public_url: &Url) -> Vec<String> {
    let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string(), "::1".to_string()];
    if let Some(host) = public_url.host_str()
        && !hosts.iter().any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        hosts.push(host.to_string());
    }
    hosts
}

fn host_is_allowed(headers: &HeaderMap, public_url: &Url) -> bool {
    let Some(value) = headers.get(HOST).and_then(|value| value.to_str().ok()) else {
        return false;
    };
    let Ok(authority) = http::uri::Authority::try_from(value) else {
        return false;
    };
    let host = authority.host().trim_matches(['[', ']']).to_ascii_lowercase();
    let port = authority.port_u16();
    if matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1") {
        return true;
    }
    let Some(public_host) = public_url.host_str() else {
        return false;
    };
    host == public_host.to_ascii_lowercase() && public_url.port().is_none_or(|expected| port == Some(expected))
}

fn quote_header_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn legacy_event_stream(
    receiver: mpsc::Receiver<ServerJsonRpcMessage>,
    guard: DropGuard,
    endpoint_event: Event,
) -> impl Stream<Item = Result<Event, Infallible>> + Send + 'static {
    let first = stream::once(async move { Ok(endpoint_event) });
    let messages = stream::unfold((receiver, guard), |(mut receiver, guard)| async move {
        let message = receiver.recv().await?;
        let data = serde_json::to_string(&message).unwrap_or_else(|_| "{}".to_string());
        let event = Event::default().event("message").data(data);
        Some((Ok(event), (receiver, guard)))
    });
    first.chain(messages)
}

#[derive(Clone)]
struct LegacySessionStore {
    sessions: Arc<Mutex<BTreeMap<String, LegacySession>>>,
    ttl: Duration,
}

struct LegacySession {
    sender: mpsc::Sender<ClientJsonRpcMessage>,
    cancellation: CancellationToken,
    expires_at: Instant,
}

impl LegacySessionStore {
    fn new(ttl: Duration) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            ttl,
        }
    }

    async fn create(
        &self,
    ) -> WebmcpResult<(
        String,
        mpsc::Receiver<ClientJsonRpcMessage>,
        mpsc::Receiver<ServerJsonRpcMessage>,
        mpsc::Sender<ServerJsonRpcMessage>,
        CancellationToken,
    )> {
        let now = Instant::now();
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, session| session.expires_at > now && !session.cancellation.is_cancelled());
        if sessions.len() >= MAX_LEGACY_SESSIONS {
            return Err(WebmcpError::LimitExceeded);
        }
        let session_id = Uuid::new_v4().simple().to_string();
        let (input_sender, input_receiver) = mpsc::channel(LEGACY_INPUT_QUEUE_CAPACITY);
        let (output_sender, output_receiver) = mpsc::channel(LEGACY_INPUT_QUEUE_CAPACITY);
        let cancellation = CancellationToken::new();
        let _ = sessions.insert(
            session_id.clone(),
            LegacySession {
                sender: input_sender,
                cancellation: cancellation.clone(),
                expires_at: now + self.ttl,
            },
        );
        drop(sessions);

        let expiration_store = self.clone();
        let expiration_id = session_id.clone();
        let expiration_token = cancellation.clone();
        // Deliberately detached: bounded by the session TTL and terminated by
        // `cancellation` (dropped with the session); it owns removing the
        // session from the store, so its outcome cannot be silently lost.
        drop(tokio::spawn(async move {
            expiration_store.expire_when_idle(expiration_id, expiration_token).await;
        }));

        Ok((session_id, input_receiver, output_receiver, output_sender, cancellation))
    }

    async fn sender(&self, session_id: &str) -> Option<mpsc::Sender<ClientJsonRpcMessage>> {
        let now = Instant::now();
        let mut sessions = self.sessions.lock().await;
        let session = sessions.get_mut(session_id)?;
        if session.expires_at <= now || session.cancellation.is_cancelled() {
            let session = sessions.remove(session_id)?;
            session.cancellation.cancel();
            return None;
        }
        session.expires_at = now + self.ttl;
        Some(session.sender.clone())
    }

    async fn remove(&self, session_id: &str) {
        if let Some(session) = self.sessions.lock().await.remove(session_id) {
            session.cancellation.cancel();
        }
    }

    async fn expire_when_idle(&self, session_id: String, cancellation: CancellationToken) {
        loop {
            let Some(remaining) = self.remaining(&session_id).await else {
                return;
            };
            tokio::select! {
                _ = sleep(remaining) => {}
                // The token is cancelled by the stream's DropGuard or the
                // server task ending; this loop owns removing the session.
                _ = cancellation.cancelled() => {
                    self.remove(&session_id).await;
                    return;
                }
            }
            let expired = self
                .sessions
                .lock()
                .await
                .get(&session_id)
                .is_some_and(|session| session.expires_at <= Instant::now());
            if expired {
                self.remove(&session_id).await;
                return;
            }
        }
    }

    async fn remaining(&self, session_id: &str) -> Option<Duration> {
        self.sessions
            .lock()
            .await
            .get(session_id)
            .map(|session| session.expires_at.saturating_duration_since(Instant::now()))
    }
}

#[derive(Debug)]
struct LegacyTransportError;

impl Display for LegacyTransportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("legacy MCP SSE output channel is closed")
    }
}

impl Error for LegacyTransportError {}

struct LegacyMessageSink {
    sender: mpsc::Sender<ServerJsonRpcMessage>,
}

impl Sink<ServerJsonRpcMessage> for LegacyMessageSink {
    type Error = LegacyTransportError;

    fn poll_ready(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.get_mut().sender.is_closed() {
            Poll::Ready(Err(LegacyTransportError))
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: ServerJsonRpcMessage) -> Result<(), Self::Error> {
        self.get_mut().sender.try_send(item).map_err(|_error| LegacyTransportError)
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

fn spawn_legacy_server(
    handler: RemoteMcpHandler,
    input_receiver: mpsc::Receiver<ClientJsonRpcMessage>,
    output_sender: mpsc::Sender<ServerJsonRpcMessage>,
    cancellation: CancellationToken,
) {
    drop(tokio::spawn(async move {
        let transport =
            SinkStreamTransport::new(LegacyMessageSink { sender: output_sender }, ReceiverStream::new(input_receiver));
        if let Ok(service) = handler.serve_with_ct(transport, cancellation.clone()).await {
            drop(service.waiting().await);
        }
        cancellation.cancel();
    }));
}

#[cfg(test)]
mod tests;
