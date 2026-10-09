use crate::error::{Result, WebmcpError};
use crate::event_hub::{EventHubConfig, EventHubSubscription, MAX_EVENT_BYTES, WebmcpEventHub};
use crate::pairing::{PairingDisplay, PairingManager};
use crate::protocol::{
    BridgeEventMessage, BridgeRequest, BridgeResponse, BridgeSettings, PROTOCOL_VERSION, PairPayload, StatusPayload,
    is_valid_request_id, response_request_id,
};
use crate::remote_mcp::{RemoteMcpEndpoint, RemoteMcpServerConfig};
use crate::runtime::RuntimeAdapter;
use axum::Router;
use axum::extract::{State, WebSocketUpgrade, ws};
use axum::http::{HeaderMap, StatusCode, header::ORIGIN};
use axum::response::{IntoResponse, Response};
use futures::{StreamExt, future::BoxFuture};
use serde_json::Value;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use vtcode_commons::validation::is_valid_origin;

const MAX_PAIRED_CONNECTIONS: usize = 64;
const MAX_CONNECTIONS: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
const EVENT_ENVELOPE_OVERHEAD: usize = 256;
const MIN_FRAME_BYTES: usize = 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MUTATION_QUEUE: usize = 64;
const SESSION_LEASE_CHECK_INTERVAL: Duration = Duration::from_millis(250);

/// Configuration for the WebMCP listener.
#[derive(Debug, Clone)]
pub struct WebmcpServerConfig {
    /// Bind host, normally `127.0.0.1`.
    pub host: String,
    /// Bind port, with zero selecting an available port.
    pub port: u16,
    /// Explicit browser origin allowlist.
    pub allowed_origins: Vec<String>,
    /// Pairing lifetime and authenticated-session inactivity lease in seconds.
    pub pairing_ttl_secs: u64,
    /// Maximum WebSocket message size.
    pub max_frame_bytes: usize,
    /// Maximum concurrent adapter operations.
    pub max_in_flight_requests: usize,
    /// Whether remote reverse-proxy mode is explicitly enabled.
    pub allow_remote: bool,
    /// Public WSS URL required for remote mode.
    pub public_url: Option<String>,
    /// Optional read-only OpenAI-compatible MCP transport.
    pub remote_mcp: Option<RemoteMcpServerConfig>,
    /// Event replay and subscriber queue limits.
    pub event_hub: EventHubConfig,
    /// Per-operation timeout.
    pub request_timeout: Duration,
}

impl Default for WebmcpServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 0,
            allowed_origins: Vec::new(),
            pairing_ttl_secs: 300,
            max_frame_bytes: 1024 * 1024,
            max_in_flight_requests: 8,
            allow_remote: false,
            public_url: None,
            remote_mcp: None,
            event_hub: EventHubConfig::default(),
            request_timeout: Duration::from_secs(30),
        }
    }
}

struct ServerState {
    adapter: Arc<dyn RuntimeAdapter>,
    pairing: PairingManager,
    event_hub: WebmcpEventHub,
    dispatch: Arc<DispatchState>,
    mutation_supervisor: MutationSupervisor,
    paired_connections: Arc<Semaphore>,
    connections: Arc<Semaphore>,
    max_frame_bytes: usize,
    request_timeout: Duration,
    remote_mcp: Option<Arc<RemoteMcpEndpoint>>,
}

struct DispatchState {
    adapter: Arc<dyn RuntimeAdapter>,
    pairing: PairingManager,
    event_hub: WebmcpEventHub,
    settings: BridgeSettings,
    in_flight: Arc<Semaphore>,
    request_timeout: Duration,
}

struct MutationJob {
    dispatch: Arc<DispatchState>,
    origin: String,
    token: String,
    request: BridgeRequest,
    result: oneshot::Sender<Result<Value>>,
}

#[derive(Clone, Default)]
struct MutationSupervisor {
    sender: Arc<tokio::sync::Mutex<Option<mpsc::Sender<MutationJob>>>>,
}

impl MutationSupervisor {
    async fn submit(
        &self,
        dispatch: Arc<DispatchState>,
        origin: String,
        token: String,
        request: BridgeRequest,
    ) -> Result<Value> {
        let sender = {
            let mut sender_slot = self.sender.lock().await;
            if let Some(sender) = sender_slot.as_ref() {
                sender.clone()
            } else {
                let (sender, receiver) = mpsc::channel(MAX_MUTATION_QUEUE);
                drop(tokio::spawn(run_mutation_supervisor(receiver)));
                *sender_slot = Some(sender.clone());
                sender
            }
        };
        let (result, receiver) = oneshot::channel();
        sender
            .try_send(MutationJob { dispatch, origin, token, request, result })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => WebmcpError::LimitExceeded,
                mpsc::error::TrySendError::Closed(_) => {
                    WebmcpError::Adapter("WebMCP mutation supervisor is closed".to_string())
                }
            })?;
        receiver
            .await
            .map_err(|_error| WebmcpError::Adapter("WebMCP mutation supervisor stopped".to_string()))?
    }
}

async fn run_mutation_supervisor(mut receiver: mpsc::Receiver<MutationJob>) {
    while let Some(job) = receiver.recv().await {
        let result = dispatch_request(job.dispatch, job.origin, job.token, job.request).await;
        drop(job.result.send(result));
    }
}

/// Authenticated WebSocket WebMCP server.
#[derive(Clone)]
pub struct WebmcpServer {
    state: Arc<ServerState>,
    config: WebmcpServerConfig,
}

impl std::fmt::Debug for WebmcpServer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WebmcpServer")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("allowed_origins", &self.config.allowed_origins)
            .finish_non_exhaustive()
    }
}

impl WebmcpServer {
    /// Create a server around an active or headless runtime adapter.
    pub fn new(adapter: Arc<dyn RuntimeAdapter>, config: WebmcpServerConfig) -> Result<Self> {
        validate_config(&config)?;
        let pairing = PairingManager::new(&config.allowed_origins, Duration::from_secs(config.pairing_ttl_secs))?;
        let remote_mcp = config
            .remote_mcp
            .clone()
            .map(|remote_config| RemoteMcpEndpoint::new(Arc::clone(&adapter), remote_config).map(Arc::new))
            .transpose()?;
        let event_limit = config
            .max_frame_bytes
            .saturating_sub(EVENT_ENVELOPE_OVERHEAD)
            .min(MAX_EVENT_BYTES);
        let event_hub = WebmcpEventHub::new_with_max_event_bytes(config.event_hub, event_limit)?;
        let in_flight = Arc::new(Semaphore::new(config.max_in_flight_requests));
        let settings = BridgeSettings {
            host: config.host.clone(),
            port: config.port,
            pairing_ttl_secs: config.pairing_ttl_secs,
            max_frame_bytes: config.max_frame_bytes,
            max_in_flight_requests: config.max_in_flight_requests,
            remote_enabled: config.allow_remote,
        };
        let dispatch = Arc::new(DispatchState {
            adapter: Arc::clone(&adapter),
            pairing: pairing.clone(),
            event_hub: event_hub.clone(),
            settings,
            in_flight: Arc::clone(&in_flight),
            request_timeout: config.request_timeout,
        });
        Ok(Self {
            state: Arc::new(ServerState {
                adapter,
                pairing,
                event_hub,
                dispatch,
                mutation_supervisor: MutationSupervisor::default(),
                paired_connections: Arc::new(Semaphore::new(MAX_PAIRED_CONNECTIONS)),
                connections: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
                max_frame_bytes: config.max_frame_bytes,
                request_timeout: config.request_timeout,
                remote_mcp,
            }),
            config,
        })
    }

    /// Start a new one-time terminal pairing code.
    pub fn begin_pairing(&self) -> PairingDisplay {
        self.state.pairing.begin_pairing()
    }

    /// Start a one-time code bound to one exact allowed browser origin.
    pub fn begin_pairing_for_origin(&self, origin: impl Into<String>) -> Result<PairingDisplay> {
        self.state.pairing.begin_pairing_for_origin(origin)
    }

    /// Revoke browser sessions and issue a fresh code for one exact origin.
    pub fn replace_pairing_for_origin(&self, origin: impl Into<String>) -> Result<PairingDisplay> {
        self.state.pairing.replace_pairing_for_origin(origin)
    }

    /// Revoke all browser sessions and pending pairing codes.
    pub fn revoke_all_pairings(&self) {
        self.state.pairing.revoke_all();
    }

    /// Access the canonical runtime event hub used by this server.
    pub fn event_hub(&self) -> WebmcpEventHub {
        self.state.event_hub.clone()
    }

    /// Build the WebSocket router. No listener is opened by this method.
    pub fn router(&self) -> Router {
        let mut router = Router::new().route("/webmcp", axum::routing::get(websocket_handler));
        if let Some(remote_mcp) = self.state.remote_mcp.as_ref() {
            let remote_routes = remote_mcp
                .routes()
                .with_state(Arc::clone(remote_mcp))
                .with_state(self.state.clone());
            router = router.merge(remote_routes);
        }
        router.with_state(self.state.clone())
    }

    /// Bind the configured address. Call [`Self::serve_listener`] afterwards.
    pub async fn bind(&self) -> Result<TcpListener> {
        TcpListener::bind((self.config.host.as_str(), self.config.port))
            .await
            .map_err(WebmcpError::Io)
    }

    /// Serve on a caller-provided listener until it fails or is stopped.
    pub async fn serve_listener(&self, listener: TcpListener) -> Result<()> {
        axum::serve(listener, self.router())
            .await
            .map_err(|error| WebmcpError::Adapter(format!("WebMCP listener failed: {error}")))
    }

    /// Bind and serve the configured listener.
    pub async fn serve(&self) -> Result<()> {
        let listener = self.bind().await?;
        self.serve_listener(listener).await
    }
}

async fn websocket_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(origin) = origin_from_headers(&headers) else {
        return (StatusCode::FORBIDDEN, "WebMCP requires an Origin header").into_response();
    };
    if !state.pairing.is_origin_allowed(origin) {
        return (StatusCode::FORBIDDEN, "WebMCP origin is not allowed").into_response();
    }
    let connection_permit = match state.connections.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_error) => return (StatusCode::TOO_MANY_REQUESTS, "WebMCP connection limit reached").into_response(),
    };
    let origin = origin.to_string();
    upgrade
        .max_message_size(state.max_frame_bytes)
        .max_frame_size(state.max_frame_bytes)
        .on_upgrade(move |socket| run_socket(socket, state, origin, connection_permit))
        .into_response()
}

fn origin_from_headers(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(ORIGIN)
        .and_then(|value| value.to_str().ok())
        .filter(|origin| !origin.is_empty())
}

async fn run_socket(
    mut socket: ws::WebSocket,
    state: Arc<ServerState>,
    origin: String,
    _connection_permit: OwnedSemaphorePermit,
) {
    let handshake_deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    loop {
        let remaining = handshake_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        let message = match tokio::time::timeout(remaining, socket.next()).await {
            Ok(Some(message)) => message,
            Ok(None) | Err(_) => return,
        };
        let Ok(message) = message else { return };
        match message {
            ws::Message::Text(text) => {
                if text.len() > state.max_frame_bytes {
                    drop(
                        send_response(
                            &mut socket,
                            BridgeResponse::failure("unknown", "frame_too_large", "request exceeds the frame limit"),
                            state.max_frame_bytes,
                        )
                        .await,
                    );
                    return;
                }
                match serde_json::from_slice::<BridgeRequest>(text.as_bytes()) {
                    Ok(BridgeRequest::Pair {
                        request_id,
                        code,
                        resume_token,
                        origin: claimed_origin,
                        after_sequence,
                    }) => {
                        if !is_valid_request_id(&request_id) {
                            drop(
                                send_response(
                                    &mut socket,
                                    invalid_request_id_response(&request_id),
                                    state.max_frame_bytes,
                                )
                                .await,
                            );
                            continue;
                        }
                        if claimed_origin.as_deref().is_some_and(|claimed| claimed != origin) {
                            drop(
                                send_response(
                                    &mut socket,
                                    BridgeResponse::failure(
                                        &request_id,
                                        "origin_mismatch",
                                        "request origin does not match the WebSocket origin",
                                    ),
                                    state.max_frame_bytes,
                                )
                                .await,
                            );
                            return;
                        }
                        let mut subscription = match state.event_hub.subscribe(after_sequence) {
                            Ok(subscription) => subscription,
                            Err(error) => {
                                drop(
                                    send_response(
                                        &mut socket,
                                        response_for_error(&request_id, error),
                                        state.max_frame_bytes,
                                    )
                                    .await,
                                );
                                return;
                            }
                        };
                        let connection_permit = match state.paired_connections.clone().try_acquire_owned() {
                            Ok(permit) => permit,
                            Err(_error) => {
                                drop(
                                    send_response(
                                        &mut socket,
                                        BridgeResponse::failure(
                                            &request_id,
                                            "connection_limit",
                                            "the WebMCP server has reached its paired connection limit",
                                        ),
                                        state.max_frame_bytes,
                                    )
                                    .await,
                                );
                                return;
                            }
                        };
                        let session = match resume_token {
                            Some(token) => state.pairing.resume(&token, &origin),
                            None => state.pairing.pair(&code, &origin),
                        };
                        match session {
                            Ok(session) => {
                                let response = BridgeResponse::success(
                                    request_id,
                                    PairPayload {
                                        token: session.token().to_string(),
                                        protocol_version: PROTOCOL_VERSION,
                                        expires_in_secs: session.expires_in().as_secs().max(1),
                                    },
                                );
                                if send_response(&mut socket, response, state.max_frame_bytes).await.is_err() {
                                    return;
                                }
                                for event in subscription.replay() {
                                    if send_event(&mut socket, event.sequence, &event.event, state.max_frame_bytes)
                                        .await
                                        .is_err()
                                    {
                                        return;
                                    }
                                }
                                run_paired_socket(
                                    socket,
                                    state,
                                    origin,
                                    session.token().to_string(),
                                    &mut subscription,
                                    connection_permit,
                                    _connection_permit,
                                )
                                .await;
                                return;
                            }
                            Err(error) => {
                                drop(
                                    send_response(
                                        &mut socket,
                                        response_for_error(&request_id, error),
                                        state.max_frame_bytes,
                                    )
                                    .await,
                                );
                            }
                        }
                    }
                    Ok(request) => {
                        let request_id = request.request_id().to_string();
                        let response = if is_valid_request_id(&request_id) {
                            response_for_error(&request_id, WebmcpError::Unauthorized)
                        } else {
                            invalid_request_id_response(&request_id)
                        };
                        drop(send_response(&mut socket, response, state.max_frame_bytes).await);
                    }
                    Err(error) => {
                        drop(
                            send_response(
                                &mut socket,
                                BridgeResponse::failure("unknown", "malformed_request", error.to_string()),
                                state.max_frame_bytes,
                            )
                            .await,
                        );
                    }
                }
            }
            ws::Message::Binary(_) => {
                drop(
                    send_response(
                        &mut socket,
                        BridgeResponse::failure(
                            "unknown",
                            "binary_not_supported",
                            "WebMCP accepts JSON text frames only",
                        ),
                        state.max_frame_bytes,
                    )
                    .await,
                );
            }
            ws::Message::Ping(payload) => {
                if send_pong(&mut socket, payload).await.is_err() {
                    return;
                }
            }
            ws::Message::Close(_) => return,
            ws::Message::Pong(_) => {}
        }
    }
}

enum PairedFrameAction {
    Request(BridgeRequest),
    Continue,
    Close,
}

async fn handle_paired_frame(
    socket: &mut ws::WebSocket,
    message: ws::Message,
    max_frame_bytes: usize,
) -> PairedFrameAction {
    match message {
        ws::Message::Text(text) => {
            if text.len() > max_frame_bytes {
                drop(
                    send_response(
                        socket,
                        BridgeResponse::failure("unknown", "frame_too_large", "request exceeds the frame limit"),
                        max_frame_bytes,
                    )
                    .await,
                );
                return PairedFrameAction::Close;
            }
            let request = match serde_json::from_slice::<BridgeRequest>(text.as_bytes()) {
                Ok(request) => request,
                Err(error) => {
                    if send_response(
                        socket,
                        BridgeResponse::failure("unknown", "malformed_request", error.to_string()),
                        max_frame_bytes,
                    )
                    .await
                    .is_err()
                    {
                        return PairedFrameAction::Close;
                    }
                    return PairedFrameAction::Continue;
                }
            };
            if !is_valid_request_id(request.request_id()) {
                if send_response(socket, invalid_request_id_response(request.request_id()), max_frame_bytes)
                    .await
                    .is_err()
                {
                    return PairedFrameAction::Close;
                }
                return PairedFrameAction::Continue;
            }
            PairedFrameAction::Request(request)
        }
        ws::Message::Binary(_) => {
            if send_response(
                socket,
                BridgeResponse::failure("unknown", "binary_not_supported", "WebMCP accepts JSON text frames only"),
                max_frame_bytes,
            )
            .await
            .is_err()
            {
                PairedFrameAction::Close
            } else {
                PairedFrameAction::Continue
            }
        }
        ws::Message::Ping(payload) => {
            if send_pong(socket, payload).await.is_err() {
                PairedFrameAction::Close
            } else {
                PairedFrameAction::Continue
            }
        }
        ws::Message::Close(_) => PairedFrameAction::Close,
        ws::Message::Pong(_) => PairedFrameAction::Continue,
    }
}

async fn run_paired_socket(
    mut socket: ws::WebSocket,
    state: Arc<ServerState>,
    origin: String,
    token: String,
    subscription: &mut EventHubSubscription,
    _connection_permit: OwnedSemaphorePermit,
    _all_connections_permit: OwnedSemaphorePermit,
) {
    let mut expiry_check = tokio::time::interval(SESSION_LEASE_CHECK_INTERVAL);
    expiry_check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            message = socket.next() => {
                let Some(Ok(message)) = message else { return };
                match handle_paired_frame(&mut socket, message, state.max_frame_bytes).await {
                    PairedFrameAction::Close => return,
                    PairedFrameAction::Continue => {}
                    PairedFrameAction::Request(request) => {
                        let request_id = request.request_id().to_string();
                        let mutation_request = matches!(
                            &request,
                            BridgeRequest::ApplyProposal { .. } | BridgeRequest::RevertLastChange { .. }
                        );
                        let operation: BoxFuture<'static, Result<Value>> = if mutation_request {
                            let supervisor = state.mutation_supervisor.clone();
                            let dispatch = Arc::clone(&state.dispatch);
                            let origin = origin.clone();
                            let token = token.clone();
                            Box::pin(async move { supervisor.submit(dispatch, origin, token, request).await })
                        } else {
                            Box::pin(dispatch_request(
                                Arc::clone(&state.dispatch),
                                origin.clone(),
                                token.clone(),
                                request,
                            ))
                        };
                        let mut operation = operation;
                        loop {
                            tokio::select! {
                                result = &mut operation => {
                                    let response = match result {
                                        Ok(payload) => BridgeResponse::success(request_id.clone(), payload),
                                        Err(error) => response_for_error(&request_id, error),
                                    };
                                    if send_response(&mut socket, response, state.max_frame_bytes).await.is_err() { return; }
                                    break;
                                }
                                message = socket.next() => {
                                    let Some(Ok(message)) = message else { return };
                                    match handle_paired_frame(&mut socket, message, state.max_frame_bytes).await {
                                        PairedFrameAction::Close => return,
                                        PairedFrameAction::Continue => {}
                                        PairedFrameAction::Request(request) => {
                                            let request_id = request.request_id().to_string();
                                            let response = if request.token() != Some(token.as_str()) {
                                                response_for_error(&request_id, WebmcpError::Unauthorized)
                                            } else if matches!(&request, BridgeRequest::Cancel { .. }) {
                                                match dispatch_cancel_request(&state, &origin, &token, request).await {
                                                    Ok(payload) => BridgeResponse::success(request_id.clone(), payload),
                                                    Err(error) => response_for_error(&request_id, error),
                                                }
                                            } else {
                                                if state.pairing.refresh(&token, &origin).is_err() { return; }
                                                BridgeResponse::failure(&request_id, "request_in_progress", "wait for the active request or cancel it")
                                            };
                                            if send_response(&mut socket, response, state.max_frame_bytes).await.is_err() { return; }
                                        }
                                    }
                                }
                                event = subscription.recv() => {
                                    let Some(event) = event else {
                                        drop(send_response(&mut socket, BridgeResponse::failure("event", "slow_client", "client could not keep up with runtime events"), state.max_frame_bytes).await);
                                        return;
                                    };
                                    if state.pairing.validate(&token, &origin).is_err() { return; }
                                    if send_event(&mut socket, event.sequence, &event.event, state.max_frame_bytes).await.is_err() { return; }
                                }
                                _ = expiry_check.tick() => {
                                    // Keep the lease pinned while the authenticated operation is
                                    // still executing. Mutations intentionally remain pending
                                    // until the supervisor knows their final result.
                                    if state.pairing.refresh(&token, &origin).is_err() { return; }
                                }
                            }
                        }
                    }
                }
            }
            event = subscription.recv() => {
                let Some(event) = event else {
                    drop(send_response(&mut socket, BridgeResponse::failure("event", "slow_client", "client could not keep up with runtime events"), state.max_frame_bytes).await);
                    return;
                };
                if state.pairing.validate(&token, &origin).is_err() { return; }
                if send_event(&mut socket, event.sequence, &event.event, state.max_frame_bytes).await.is_err() { return; }
            }
            _ = expiry_check.tick() => {
                if state.pairing.validate(&token, &origin).is_err() { return; }
            }
        }
    }
}

async fn dispatch_request(
    dispatch: Arc<DispatchState>,
    origin: String,
    token: String,
    request: BridgeRequest,
) -> Result<Value> {
    if request.token() != Some(token.as_str()) {
        return Err(WebmcpError::Unauthorized);
    }
    dispatch.pairing.refresh(&token, &origin)?;
    let _permit = tokio::time::timeout(dispatch.request_timeout, dispatch.in_flight.clone().acquire_owned())
        .await
        .map_err(|_error| WebmcpError::Timeout(dispatch.request_timeout))?
        .map_err(|_error| WebmcpError::Adapter("WebMCP request capacity is closed".to_string()))?;
    // A request may have waited for capacity long enough for the session to
    // expire or be revoked. Re-check immediately before handing it to the
    // runtime adapter.
    dispatch.pairing.refresh(&token, &origin)?;
    let mutation_request =
        matches!(&request, BridgeRequest::ApplyProposal { .. } | BridgeRequest::RevertLastChange { .. });
    let operation = async {
        match request {
            BridgeRequest::Pair { .. } => Err(WebmcpError::Unauthorized),
            BridgeRequest::ExplanationGet { scope, offset, .. } => {
                serde_json::to_value(dispatch.adapter.explanation_get(scope, offset).await?).map_err(WebmcpError::from)
            }
            BridgeRequest::ExplanationEvidence { reference, offset, .. } => {
                serde_json::to_value(dispatch.adapter.explanation_evidence(reference, offset).await?)
                    .map_err(WebmcpError::from)
            }
            BridgeRequest::ExplanationNavigate { reference, .. } => {
                Ok(serde_json::json!({"focused": dispatch.adapter.explanation_navigate(reference).await?}))
            }
            BridgeRequest::Status { .. } => {
                let runtime = dispatch.adapter.status().await?;
                serde_json::to_value(StatusPayload {
                    protocol_version: PROTOCOL_VERSION,
                    connected: runtime.connected,
                    runtime,
                    authenticated_origin: origin,
                    settings: dispatch.settings.clone(),
                    latest_sequence: dispatch.event_hub.latest_sequence(),
                })
                .map_err(WebmcpError::Json)
            }
            BridgeRequest::ListFiles { .. } => {
                serde_json::to_value(dispatch.adapter.list_files().await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::ReadFile { path, .. } => {
                serde_json::to_value(dispatch.adapter.read_file(&path).await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::ProposeChanges { changes, .. } => {
                serde_json::to_value(dispatch.adapter.propose_changes(changes).await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::ApplyProposal { proposal_id, .. } => {
                serde_json::to_value(dispatch.adapter.apply_proposal(&proposal_id).await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::RunChecks { command, .. } => {
                serde_json::to_value(dispatch.adapter.run_checks(&command).await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::RevertLastChange { change_id, .. } => {
                serde_json::to_value(dispatch.adapter.revert_last_change(&change_id).await?).map_err(WebmcpError::Json)
            }
            BridgeRequest::RequestTurn { prompt, proposal_id, .. } => {
                serde_json::to_value(dispatch.adapter.request_turn(&prompt, proposal_id.as_deref()).await?)
                    .map_err(WebmcpError::Json)
            }
            BridgeRequest::Cancel { target_id, .. } => {
                let accepted = dispatch.adapter.cancel(&target_id).await?;
                Ok(serde_json::json!({ "cancelled": target_id, "accepted": accepted }))
            }
        }
    };
    if mutation_request {
        // Filesystem adapters keep multi-file mutations transactional after a
        // transport disconnect. Keep the response pending until that result is
        // known instead of returning an ambiguous timeout to the browser.
        operation.await
    } else {
        tokio::time::timeout(dispatch.request_timeout, operation)
            .await
            .map_err(|_error| WebmcpError::Timeout(dispatch.request_timeout))?
    }
}

async fn dispatch_cancel_request(
    state: &Arc<ServerState>,
    origin: &str,
    token: &str,
    request: BridgeRequest,
) -> Result<Value> {
    if request.token() != Some(token) {
        return Err(WebmcpError::Unauthorized);
    }
    state.pairing.refresh(token, origin)?;
    let BridgeRequest::Cancel { target_id, .. } = request else {
        return Err(WebmcpError::InvalidRequest(
            "only cancellation requests are accepted while a request is running".to_string(),
        ));
    };
    let accepted = tokio::time::timeout(state.request_timeout, state.adapter.cancel(&target_id))
        .await
        .map_err(|_error| WebmcpError::Timeout(state.request_timeout))??;
    Ok(serde_json::json!({ "cancelled": target_id, "accepted": accepted }))
}

async fn send_response(socket: &mut ws::WebSocket, response: BridgeResponse, max_frame_bytes: usize) -> Result<()> {
    let serialized = serde_json::to_string(&response)?;
    let serialized = if serialized.len() > max_frame_bytes {
        serde_json::to_string(&BridgeResponse::failure(
            response.request_id,
            "limit_exceeded",
            "WebMCP response exceeds the configured frame limit",
        ))?
    } else {
        serialized
    };
    if serialized.len() > max_frame_bytes {
        return Err(WebmcpError::LimitExceeded);
    }
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(ws::Message::Text(serialized.into())))
        .await
        .map_err(|_error| WebmcpError::Timeout(WRITE_TIMEOUT))?
        .map_err(|error| WebmcpError::Adapter(error.to_string()))
}

async fn send_pong(socket: &mut ws::WebSocket, payload: axum::body::Bytes) -> Result<()> {
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(ws::Message::Pong(payload)))
        .await
        .map_err(|_error| WebmcpError::Timeout(WRITE_TIMEOUT))?
        .map_err(|error| WebmcpError::Adapter(error.to_string()))
}

async fn send_event(
    socket: &mut ws::WebSocket,
    sequence: u64,
    event: &vtcode_exec_events::VersionedThreadEvent,
    max_frame_bytes: usize,
) -> Result<()> {
    let message = BridgeEventMessage { kind: "event", sequence, event: event.clone() };
    let serialized = serde_json::to_string(&message)?;
    if serialized.len() > max_frame_bytes {
        return Err(WebmcpError::LimitExceeded);
    }
    tokio::time::timeout(WRITE_TIMEOUT, socket.send(ws::Message::Text(serialized.into())))
        .await
        .map_err(|_error| WebmcpError::Timeout(WRITE_TIMEOUT))?
        .map_err(|error| WebmcpError::Adapter(error.to_string()))
}

fn response_for_error(request_id: &str, error: WebmcpError) -> BridgeResponse {
    let (code, message) = match &error {
        WebmcpError::OriginRejected(_) => ("origin_rejected", "browser origin is not allowed".to_string()),
        WebmcpError::PairingExpired => ("pairing_expired", error.to_string()),
        WebmcpError::PairingUsed => ("pairing_used", error.to_string()),
        WebmcpError::Unauthorized => ("unauthorized", error.to_string()),
        WebmcpError::LimitExceeded => ("limit_exceeded", error.to_string()),
        WebmcpError::PathRejected(_) => ("path_rejected", error.to_string()),
        WebmcpError::Conflict { .. } => ("conflict", error.to_string()),
        WebmcpError::ProposalNotFound => ("proposal_not_found", error.to_string()),
        WebmcpError::ApprovalRequired => ("approval_required", error.to_string()),
        WebmcpError::Unsupported(_) => ("unsupported", error.to_string()),
        WebmcpError::ChangeNotFound => ("change_not_found", error.to_string()),
        WebmcpError::PartialApply => ("partial_apply", error.to_string()),
        WebmcpError::SequenceGap { .. } => ("sequence_gap", error.to_string()),
        WebmcpError::SlowClient => ("slow_client", error.to_string()),
        WebmcpError::Timeout(_) => ("timeout", error.to_string()),
        WebmcpError::InvalidRequest(_) | WebmcpError::Json(_) => ("invalid_request", error.to_string()),
        WebmcpError::Io(_) | WebmcpError::Adapter(_) => {
            ("runtime_error", "WebMCP runtime operation failed".to_string())
        }
    };
    BridgeResponse::failure(response_request_id(request_id), code, message)
}

fn invalid_request_id_response(request_id: &str) -> BridgeResponse {
    BridgeResponse::failure(
        response_request_id(request_id),
        "invalid_request",
        "request_id must be between 1 and 256 UTF-8 bytes",
    )
}

fn validate_config(config: &WebmcpServerConfig) -> Result<()> {
    if config.host.trim().is_empty() || config.max_in_flight_requests == 0 {
        return Err(WebmcpError::InvalidRequest("WebMCP host and limits must be non-empty".to_string()));
    }
    if config.max_frame_bytes < MIN_FRAME_BYTES
        || config.max_frame_bytes > 16 * 1024 * 1024
        || config.max_in_flight_requests > 64
    {
        return Err(WebmcpError::LimitExceeded);
    }
    if (config.allowed_origins.is_empty() && config.remote_mcp.is_none())
        || config.allowed_origins.iter().any(|origin| !is_valid_origin(origin))
    {
        return Err(WebmcpError::InvalidRequest("WebMCP requires an explicit origin allowlist".to_string()));
    }
    let address = config
        .host
        .parse::<IpAddr>()
        .map_err(|_error| WebmcpError::InvalidRequest("WebMCP host must be a literal IP address".to_string()))?;
    if !address.is_loopback() {
        return Err(WebmcpError::InvalidRequest(
            "WebMCP only binds loopback; place a TLS-terminating reverse proxy in front of it for remote access"
                .to_string(),
        ));
    }
    match (config.allow_remote, config.public_url.as_deref()) {
        (false, Some(_)) => {
            return Err(WebmcpError::InvalidRequest("--public-url requires remote WebMCP mode".to_string()));
        }
        (true, None) => {
            return Err(WebmcpError::InvalidRequest("remote WebMCP mode requires a wss:// public URL".to_string()));
        }
        (true, Some(url)) if !is_valid_public_url(url) => {
            return Err(WebmcpError::InvalidRequest(
                "remote WebMCP mode requires a valid wss:// public URL".to_string(),
            ));
        }
        (false, None) => {}
        (true, Some(_)) => {}
    }
    if config.request_timeout.is_zero() {
        return Err(WebmcpError::InvalidRequest("WebMCP request timeout must be positive".to_string()));
    }
    if let Some(remote_mcp) = config.remote_mcp.as_ref() {
        remote_mcp.validate()?;
    }
    Ok(())
}

fn is_valid_public_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    url == url.trim()
        && !url.chars().any(char::is_whitespace)
        && parsed.scheme() == "wss"
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FilesystemWorkspace;
    use tempfile::TempDir;

    #[tokio::test]
    async fn explanation_requests_require_origin_bound_tokens_and_report_unsupported() {
        let temp = TempDir::new().expect("workspace");
        let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
        let origin = "https://example.test";
        let server = WebmcpServer::new(
            adapter,
            WebmcpServerConfig {
                allowed_origins: vec![origin.to_owned(), "https://other.test".to_owned()],
                ..Default::default()
            },
        )
        .expect("server");
        let pairing = server.begin_pairing_for_origin(origin).expect("pairing");
        let session = server
            .state
            .pairing
            .pair(pairing.code(), origin)
            .expect("authenticated session");
        let token = session.token().to_owned();
        let reference = vtcode_memory::explanation::EvidenceRef {
            session_id: "session".into(),
            offset: 0,
            length: 1,
            digest: "0".repeat(64),
            item_id: None,
        };
        let requests = [
            BridgeRequest::ExplanationGet {
                request_id: "get".into(),
                token: token.clone(),
                scope: vtcode_memory::explanation::ExplanationScope::Task,
                offset: 0,
            },
            BridgeRequest::ExplanationEvidence {
                request_id: "evidence".into(),
                token: token.clone(),
                reference: reference.clone(),
                offset: 0,
            },
            BridgeRequest::ExplanationNavigate {
                request_id: "navigate".into(),
                token: token.clone(),
                reference,
            },
        ];
        for request in &requests {
            let dispatch = || Arc::clone(&server.state.dispatch);
            assert!(matches!(
                dispatch_request(dispatch(), origin.into(), "wrong-token".into(), request.clone()).await,
                Err(WebmcpError::Unauthorized)
            ));
            let error = dispatch_request(dispatch(), origin.into(), token.clone(), request.clone())
                .await
                .expect_err("headless adapter does not provide explanations");
            assert!(matches!(error, WebmcpError::Unsupported(_)));
            assert_eq!(response_for_error("operation", error).error.expect("error payload").code, "unsupported");
        }
        assert!(matches!(
            dispatch_request(
                Arc::clone(&server.state.dispatch),
                "https://other.test".into(),
                token.clone(),
                requests[0].clone()
            )
            .await,
            Err(WebmcpError::Unauthorized)
        ));
        for request in requests {
            assert!(matches!(
                dispatch_request(Arc::clone(&server.state.dispatch), origin.into(), token.clone(), request).await,
                Err(WebmcpError::Unauthorized)
            ));
        }
        let status = server.state.adapter.status().await.expect("status");
        assert!(!status.explanations_available);
        assert!(!status.turns_available);
        assert!(std::fs::read_dir(temp.path()).expect("workspace files").next().is_none());
    }

    #[tokio::test]
    async fn server_requires_explicit_origins_and_remote_flags() {
        let temp = TempDir::new().expect("temp dir");
        let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
        assert!(matches!(
            WebmcpServer::new(adapter.clone(), WebmcpServerConfig::default()),
            Err(WebmcpError::InvalidRequest(_))
        ));
        let config = WebmcpServerConfig {
            host: "0.0.0.0".to_string(),
            allowed_origins: vec!["https://example.test".to_string()],
            ..Default::default()
        };
        assert!(matches!(WebmcpServer::new(adapter, config), Err(WebmcpError::InvalidRequest(_))));

        let remote_config = WebmcpServerConfig {
            host: "0.0.0.0".to_string(),
            allowed_origins: vec!["https://example.test".to_string()],
            allow_remote: true,
            public_url: Some("wss://bridge.example.test/webmcp".to_string()),
            ..Default::default()
        };
        let temp = TempDir::new().expect("temp dir");
        let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
        assert!(matches!(WebmcpServer::new(adapter, remote_config), Err(WebmcpError::InvalidRequest(_))));

        let temp = TempDir::new().expect("temp dir");
        let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
        let valid_proxy_config = WebmcpServerConfig {
            allowed_origins: vec!["https://example.test".to_string()],
            allow_remote: true,
            public_url: Some("wss://bridge.example.test/webmcp".to_string()),
            ..Default::default()
        };
        assert!(WebmcpServer::new(adapter.clone(), valid_proxy_config).is_ok());
        let invalid_public_url_config = WebmcpServerConfig {
            allowed_origins: vec!["https://example.test".to_string()],
            public_url: Some("ws://bridge.example.test/webmcp".to_string()),
            ..Default::default()
        };
        assert!(matches!(WebmcpServer::new(adapter, invalid_public_url_config), Err(WebmcpError::InvalidRequest(_))));

        let temp = TempDir::new().expect("temp dir");
        let adapter = Arc::new(FilesystemWorkspace::new(temp.path(), [], false).await.expect("adapter"));
        let invalid_public_url_config = WebmcpServerConfig {
            allowed_origins: vec!["https://example.test".to_string()],
            allow_remote: true,
            public_url: Some("wss://".to_string()),
            ..Default::default()
        };
        assert!(matches!(WebmcpServer::new(adapter, invalid_public_url_config), Err(WebmcpError::InvalidRequest(_))));
    }
}
