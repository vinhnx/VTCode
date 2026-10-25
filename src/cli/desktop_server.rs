mod composer_resources;
mod config_settings;
mod session_options;
mod shared_preferences;
mod theme_preferences;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio::time::timeout;
use vtcode_config::models::{ModelId, model_catalog_entry};
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::tools::SpooledOutputReference;
use vtcode_core::utils::session_archive::{
    SessionListing, find_session_by_identifier, generate_session_archive_identifier, history_persistence_enabled,
    list_recent_sessions, reserve_session_archive_identifier, session_workspace_path,
};
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionKind, InteractionRegistry, InteractionRequest,
    InteractionStep, ModelInfo, ModelSelectParams, Request, RequestId, SessionSummary, ThreadEventNotification,
};
use vtcode_ui::tui::app::{
    ContentPart, HostCommandReceiver, HostInteraction, InlineCommand, InlineEvent, SubmittedInput,
    host_interaction_from_command, host_interaction_response, new_host_session,
};
use vtcode_webmcp::runtime::{
    AppliedChange, CheckResult, FileSnapshot, PatchProposal, RuntimeAdapter, RuntimeStatus, TurnResult, WorkspaceFile,
};
use vtcode_webmcp::{FileChange, FilesystemWorkspace, WebmcpServer, WebmcpServerConfig};

use crate::agent::agents::{ResumeSession, load_resume_session};
use crate::agent::runloop::unified::turn::run_single_agent_loop_unified_with_host;
use crate::startup::StartupContext;

const MAX_SESSIONS: usize = 200;
const MAX_RESTORED_MESSAGES: usize = 160;
const MAX_RESTORED_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_RESTORED_HISTORY_BYTES: usize = 700 * 1024;
const MAX_RESTORED_SPOOLED_OUTPUT_BYTES: usize = 32 * 1024;
const RESTORED_OUTPUT_TRUNCATION_SUFFIX: &str = "\n[Output truncated in restored history.]";
const SESSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const RUNLOOP_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct ServerState {
    initialized: bool,
    session_id: Option<String>,
    runtime: Option<RuntimeSession>,
    active_turn_id: Option<String>,
    cancel_requested_turn: Option<String>,
    pending_interactions: std::collections::HashMap<String, PendingInteraction>,
    interaction_registry: InteractionRegistry,
    /// Provider/model override applied to every session started after a
    /// `config/select`; the session restarts so the runtime rebuilds cleanly.
    model_override: Option<ModelSelectParams>,
    session_options: protocol::SessionOptionsSelectParams,
    theme_override: Option<String>,
    webmcp_event_hub: Option<vtcode_webmcp::WebmcpEventHub>,
}

struct RuntimeSession {
    session_id: String,
    input: mpsc::UnboundedSender<InlineEvent>,
    exit: oneshot::Receiver<std::result::Result<(), String>>,
    abort: AbortHandle,
    command_forwarder: std::thread::JoinHandle<()>,
}

#[derive(Clone)]
struct PendingInteraction {
    session_id: String,
    turn_id: String,
    view: HostInteraction,
}

struct ServiceContext {
    startup: StartupContext,
    workspace: PathBuf,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
    webmcp: Mutex<Option<DesktopWebmcpBridge>>,
}

struct DesktopWebmcpBridge {
    server: WebmcpServer,
    task: tokio::task::JoinHandle<()>,
    endpoint: String,
}

impl Drop for DesktopWebmcpBridge {
    fn drop(&mut self) {
        self.server.revoke_all_pairings();
        self.task.abort();
    }
}

pub(crate) async fn handle_desktop_server_command(startup: &StartupContext) -> Result<()> {
    let workspace = vtcode_commons::paths::canonicalize(&startup.workspace)
        .with_context(|| format!("failed to resolve workspace {}", startup.workspace.display()))?;
    let (notifications, mut notification_rx) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let context = ServiceContext {
        startup: startup.clone(),
        workspace,
        state: std::sync::Arc::new(Mutex::new(ServerState::default())),
        notifications,
        turn_finished: std::sync::Arc::new(Notify::new()),
        webmcp: Mutex::new(None),
    };
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut keep_running = true;

    while keep_running {
        tokio::select! {
            frame = protocol::read_frame(&mut reader) => {
                match frame {
                    Ok(Some(frame)) => {
                        keep_running = process_frame(&context, &mut stdout, &frame).await?;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::warn!(target: "vtcode.desktop", error = %error, "rejected desktop protocol frame");
                        let response = protocol::error_response(None, -32700, "invalid or oversized JSON-RPC frame");
                        protocol::write_frame(&mut stdout, &response).await?;
                    }
                }
            }
            notification = notification_rx.recv() => {
                if let Some(notification) = notification {
                    protocol::write_frame(&mut stdout, &notification).await?;
                }
            }
        }
    }

    shutdown_current_session(&context, true).await;
    stop_webmcp_bridge(&context).await;
    let _ = stdout.flush().await;
    Ok(())
}

async fn process_frame<W>(context: &ServiceContext, stdout: &mut W, frame: &[u8]) -> Result<bool>
where
    W: AsyncWrite + Unpin,
{
    let value = match serde_json::from_slice::<Value>(frame) {
        Ok(value) => value,
        Err(_) => {
            protocol::write_frame(stdout, &protocol::error_response(None, -32700, "invalid JSON-RPC message")).await?;
            return Ok(true);
        }
    };

    let Some(raw_id) = value.get("id") else {
        // Notifications carry no response id. The v1 desktop client uses
        // requests for state-changing commands so they can observe failures.
        return Ok(true);
    };
    let request_id = serde_json::from_value::<RequestId>(raw_id.clone()).ok();
    let request = match serde_json::from_value::<Request>(value) {
        Ok(request) if request.jsonrpc == "2.0" && !request.method.trim().is_empty() => request,
        _ => {
            let response = protocol::error_response(request_id, -32600, "invalid JSON-RPC request");
            protocol::write_frame(stdout, &response).await?;
            return Ok(true);
        }
    };

    match handle_request(context, &request).await {
        Ok((result, should_stop)) => {
            let response = protocol::success_response(request.id, result)?;
            protocol::write_frame(stdout, &response).await?;
            Ok(!should_stop)
        }
        Err(error) => {
            tracing::warn!(target: "vtcode.desktop", method = %request.method, error = %error, "desktop request failed");
            let response =
                protocol::error_response(Some(request.id), -32000, sanitize_public_error(&error.to_string()));
            protocol::write_frame(stdout, &response).await?;
            Ok(true)
        }
    }
}

async fn handle_request(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    use protocol::method;

    if request.method == method::INITIALIZE {
        let params: protocol::InitializeParams = parse_params(&request.params)?;
        let selected = params.protocol.negotiate(protocol::PROTOCOL_VERSION)?;
        let result = protocol::InitializeResult {
            protocol: selected,
            workspace: context.workspace.to_string_lossy().into_owned(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        context.state.lock().await.initialized = true;
        return Ok((serde_json::to_value(result)?, false));
    }

    if request.method == method::SHUTDOWN {
        shutdown_current_session(context, true).await;
        return Ok((json!({ "shutdown": true }), true));
    }

    if !context.state.lock().await.initialized {
        bail!("desktop/initialize must be the first request");
    }

    match request.method.as_str() {
        method::WORKSPACE_REVIEW => {
            let params: protocol::WorkspaceReviewParams = parse_params(&request.params)?;
            if params.request_id.is_empty() || params.request_id.len() > 128 {
                bail!("invalid workspace review request identity");
            }
            let mut result = protocol::WorkspaceReviewResult {
                include_untracked: params.include_untracked,
                scope: String::new(),
                request_id: params.request_id,
                workspace: context.workspace.to_string_lossy().into_owned(),
                captured_at: chrono::Utc::now().to_rfc3339(),
                patch: None,
                truncated: false,
                note: String::new(),
            };
            match resolve_review_scope(&context.workspace, params.scope.as_deref()) {
                Ok(scope_path) => {
                    result.scope = scope_path
                        .strip_prefix(&context.workspace)
                        .context("Git review scope escaped workspace")?
                        .to_string_lossy()
                        .into_owned();
                    let snapshot =
                        vtcode_core::git::capture_workspace_review_diff(&scope_path, params.include_untracked).await;
                    result.captured_at = snapshot.captured_at;
                    result.patch = snapshot.text;
                    result.truncated = snapshot.truncated;
                    result.note = snapshot.note;
                }
                Err(error) => {
                    result.note = format!("Git review unavailable: {}", sanitize_public_error(&error.to_string()))
                }
            }
            Ok((serde_json::to_value(result)?, false))
        }
        method::CONFIG_DISCOVER => {
            let agent = active_agent_config(context).await;
            let snapshot = configuration_snapshot(context, &agent).await;
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::CONFIG_SETTINGS_READ => {
            let snapshot = config_settings::read(context.workspace.clone()).await?;
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::CONFIG_SETTINGS_WRITE => {
            let params: protocol::ConfigSettingsWriteParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            if !context.state.lock().await.pending_interactions.is_empty() {
                bail!("Resolve the pending CLI interaction before changing configuration");
            }
            let result = config_settings::write(context.workspace.clone(), params).await?;
            shutdown_current_session(context, false).await;
            stop_webmcp_bridge(context).await;
            context.state.lock().await.session_id = None;
            Ok((serde_json::to_value(result)?, false))
        }
        method::WEBMCP_STATUS => {
            let status = webmcp_status(context).await;
            Ok((serde_json::to_value(status)?, false))
        }
        method::WEBMCP_PAIR => {
            let params: protocol::WebmcpPairParams = parse_params(&request.params)?;
            let result = pair_webmcp(context, params).await?;
            Ok((serde_json::to_value(result)?, false))
        }
        method::WEBMCP_STOP => {
            stop_webmcp_bridge(context).await;
            Ok((json!({ "stopped": true }), false))
        }
        method::CONFIG_MODELS => {
            let (provider, model) = {
                let agent = active_agent_config(context).await;
                (agent.provider.clone(), agent.model.clone())
            };
            let models = catalog_models(&provider, &model);
            Ok((serde_json::to_value(models)?, false))
        }
        method::CONFIG_SELECT => {
            let params: ModelSelectParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            if !catalog_models(&params.provider, &params.model)
                .iter()
                .any(|model| model.provider == params.provider && model.id == params.model)
            {
                bail!("unknown provider/model selection");
            }
            shutdown_current_session(context, false).await;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                state.model_override = Some(params);
                state.session_options.reasoning_effort = None;
            }
            let agent = active_agent_config(context).await;
            let snapshot = configuration_snapshot(context, &agent).await;
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::COMPOSER_RESOURCES => {
            ensure_no_active_turn(context).await?;
            let resources = timeout(Duration::from_secs(3), composer_resources::discover(context.workspace.clone()))
                .await
                .context("CLI resource discovery timed out")??;
            Ok((serde_json::to_value(resources)?, false))
        }
        method::CONFIG_OPTIONS => {
            let params: protocol::SessionOptionsSelectParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let agent = active_agent_config(context).await;
            session_options::validate(&params, &agent.provider, &agent.model)?;
            shutdown_current_session(context, false).await;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                session_options::merge(&mut state.session_options, params);
            }
            let agent = active_agent_config(context).await;
            Ok((serde_json::to_value(configuration_snapshot(context, &agent).await)?, false))
        }
        method::CONFIG_THEME => {
            let params: protocol::ThemePreferenceSelectParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let theme = theme_preferences::persist(context.workspace.clone(), params.theme).await?;
            shutdown_current_session(context, false).await;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                state.theme_override = Some(theme.clone());
            }
            let agent = active_agent_config(context).await;
            let mut snapshot = configuration_snapshot(context, &agent).await;
            if let Some(shared) = snapshot.shared_ui.as_mut() {
                shared.theme = theme;
            }
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::SESSION_LIST => {
            let sessions = list_recent_sessions(MAX_SESSIONS).await?;
            let sessions = sessions
                .iter()
                .filter(|listing| listing_matches_workspace(listing, &context.workspace))
                .map(session_summary)
                .collect::<Vec<_>>();
            Ok((serde_json::to_value(sessions)?, false))
        }
        method::SESSION_CREATE => {
            let params: protocol::SessionCreateParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            shutdown_current_session(context, false).await;
            let workspace_label = workspace_label(&context.workspace);
            let session_id = if history_persistence_enabled() {
                reserve_session_archive_identifier(&workspace_label, None).await?
            } else {
                generate_session_archive_identifier(&workspace_label, None)
            };
            start_session(context, session_id.clone(), None).await?;
            context.state.lock().await.session_id = Some(session_id.clone());
            Ok((
                serde_json::to_value(SessionSummary {
                    session_id,
                    title: params.title,
                    workspace: context.workspace.to_string_lossy().into_owned(),
                    completed: false,
                    resumable: history_persistence_enabled(),
                })?,
                false,
            ))
        }
        method::SESSION_LOAD => {
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let listing = find_session_by_identifier(&params.session_id)
                .await?
                .ok_or_else(|| anyhow!("saved session was not found"))?;
            if !listing_matches_workspace(&listing, &context.workspace) {
                bail!("saved session belongs to a different workspace");
            }
            let resume = load_resume_session(&params.session_id, ArchivedSessionIntent::ResumeInPlace)
                .await?
                .ok_or_else(|| anyhow!("saved session could not be loaded"))?;
            shutdown_current_session(context, false).await;
            let workspace = context.workspace.clone();
            let history_session_id = params.session_id.clone();
            let inspector_history =
                tokio::task::spawn_blocking(move || restored_inspector_history(&workspace, &history_session_id))
                    .await
                    .context("inspector history task failed")??;
            start_session(context, params.session_id.clone(), Some(resume)).await?;
            context.state.lock().await.session_id = Some(params.session_id.clone());
            let result = protocol::SessionLoadResult {
                session: session_summary(&listing),
                messages: restored_history(&listing),
                review_items: inspector_history.items,
                review_history_incomplete: inspector_history.incomplete,
                activity_items: inspector_history.activity_items,
                activity_history_incomplete: inspector_history.activity_incomplete,
            };
            Ok((serde_json::to_value(result)?, false))
        }
        method::SESSION_CLOSE => {
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            let selected_id = context.state.lock().await.session_id.clone();
            if selected_id.as_deref() != Some(params.session_id.as_str()) {
                bail!("session is not selected");
            }
            ensure_no_active_turn(context).await?;
            shutdown_current_session(context, false).await;
            context.state.lock().await.session_id = None;
            Ok((json!({ "closed": true }), false))
        }
        method::TURN_SUBMIT => {
            let params: protocol::TurnSubmitParams = parse_params(&request.params)?;
            // Client-read attachments become inline text parts, framed with
            // their paths so the model can cite them.
            let attachment_parts = params
                .attachments
                .iter()
                .map(|attachment| ContentPart::Text {
                    text: format!("--- {} ---\n{}", attachment.path, attachment.text),
                })
                .collect::<Vec<_>>();
            let accepted = enqueue_turn(
                &context.state,
                &context.notifications,
                &params.session_id,
                params.prompt,
                attachment_parts,
            )
            .await?;
            Ok((serde_json::to_value(accepted)?, false))
        }
        method::TURN_CANCEL => {
            let params: protocol::TurnCancelParams = parse_params(&request.params)?;
            let (input, should_send) = {
                let mut state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str())
                    || state.active_turn_id.as_deref() != Some(params.turn_id.as_str())
                {
                    bail!("turn is no longer active");
                }
                let input = state
                    .runtime
                    .as_ref()
                    .map(|runtime| runtime.input.clone())
                    .ok_or_else(|| anyhow!("selected session is not running"))?;
                let should_send = state.cancel_requested_turn.as_deref() != Some(params.turn_id.as_str());
                if should_send {
                    state.cancel_requested_turn = Some(params.turn_id.clone());
                }
                (input, should_send)
            };
            if should_send && input.send(InlineEvent::Cancel).is_err() {
                let mut state = context.state.lock().await;
                if state.cancel_requested_turn.as_deref() == Some(params.turn_id.as_str()) {
                    state.cancel_requested_turn = None;
                }
                bail!("selected session runtime has stopped");
            }
            Ok((json!({ "cancelRequested": true }), false))
        }
        method::INTERACTION_RESPOND => {
            let response: protocol::InteractionResponse = parse_params(&request.params)?;
            respond_to_interaction(context, response).await?;
            Ok((json!({ "accepted": true }), false))
        }
        _ => bail!("unknown desktop method"),
    }
}

async fn enqueue_turn(
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    session_id: &str,
    prompt: String,
    attachment_parts: Vec<ContentPart>,
) -> Result<protocol::TurnAccepted> {
    if prompt.trim().is_empty() {
        bail!("prompt must not be empty");
    }
    let turn_id = uuid::Uuid::new_v4().to_string();
    let input = {
        let mut state = state.lock().await;
        if state.session_id.as_deref() != Some(session_id) {
            bail!("session is not selected");
        }
        if state.active_turn_id.is_some() {
            bail!("another turn is already running");
        }
        let runtime = state
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow!("selected session is not running"))?;
        if runtime.input.is_closed() {
            bail!("selected session runtime has stopped; reload the session to continue");
        }
        let input = runtime.input.clone();
        state.cancel_requested_turn = None;
        state.active_turn_id = Some(turn_id.clone());
        input
    };
    if input
        .send(InlineEvent::Submit(SubmittedInput::new(prompt, attachment_parts)))
        .is_err()
    {
        let mut state = state.lock().await;
        if state.active_turn_id.as_deref() == Some(turn_id.as_str()) {
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
        }
        bail!("selected session runtime has stopped; reload the session to continue");
    }
    send_notification(
        notifications,
        protocol::method::TURN_STATUS,
        json!({ "sessionId": session_id, "turnId": turn_id, "status": "running" }),
    )
    .await?;
    Ok(protocol::TurnAccepted { session_id: session_id.to_string(), turn_id })
}

async fn webmcp_status(context: &ServiceContext) -> protocol::WebmcpBridgeStatus {
    let (session_id, turns_available) = {
        let state = context.state.lock().await;
        let session_id = state.session_id.clone();
        let turns_available = state.active_turn_id.is_none()
            && state.runtime.as_ref().is_some_and(|runtime| {
                Some(runtime.session_id.as_str()) == session_id.as_deref() && !runtime.input.is_closed()
            });
        (session_id, turns_available)
    };
    let endpoint = context.webmcp.lock().await.as_ref().map(|bridge| bridge.endpoint.clone());
    protocol::WebmcpBridgeStatus {
        running: endpoint.is_some(),
        endpoint,
        allowed_origins: context.startup.config.webmcp.allowed_origins.clone(),
        session_id,
        turns_available,
        workspace: context.workspace.to_string_lossy().into_owned(),
    }
}

async fn pair_webmcp(
    context: &ServiceContext,
    params: protocol::WebmcpPairParams,
) -> Result<protocol::WebmcpPairResult> {
    let settings = &context.startup.config.webmcp;
    let origin = params.origin.trim();
    if origin != params.origin || !vtcode_commons::validation::is_valid_origin(origin) {
        bail!("enter an exact browser origin, such as https://example.test");
    }
    let allowed_origins = crate::agent::runloop::unified::webmcp::configured_origins(settings, origin)?;

    let mut bridge_slot = context.webmcp.lock().await;
    if let Some(bridge) = bridge_slot.as_ref() {
        let pairing = if params.replace {
            bridge.server.replace_pairing_for_origin(origin.to_string())?
        } else {
            bridge.server.begin_pairing_for_origin(origin.to_string())?
        };
        return Ok(protocol::WebmcpPairResult {
            endpoint: bridge.endpoint.clone(),
            origin: origin.to_string(),
            pairing_code: pairing.code().to_string(),
            expires_in_secs: pairing.expires_in().as_secs(),
        });
    }

    let workspace = FilesystemWorkspace::new(&context.workspace, [context.workspace.clone()], false)
        .await
        .context("failed to initialize the desktop WebMCP workspace")?;
    let adapter = DesktopWebmcpRuntime {
        workspace,
        state: std::sync::Arc::clone(&context.state),
        notifications: context.notifications.clone(),
    };
    let server = WebmcpServer::new(
        std::sync::Arc::new(adapter),
        WebmcpServerConfig {
            host: settings.host.clone(),
            port: settings.port,
            allowed_origins,
            pairing_ttl_secs: settings.pairing_ttl_secs,
            max_frame_bytes: settings.max_frame_bytes,
            max_in_flight_requests: settings.max_in_flight_requests,
            ..Default::default()
        },
    )?;
    let pairing = if params.replace {
        server.replace_pairing_for_origin(origin.to_string())?
    } else {
        server.begin_pairing_for_origin(origin.to_string())?
    };
    let listener = server.bind().await.context("failed to bind the desktop WebMCP listener")?;
    let address = listener
        .local_addr()
        .context("failed to determine the desktop WebMCP listener address")?;
    let endpoint = format!("ws://{address}/webmcp");
    let task_server = server.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = task_server.serve_listener(listener).await {
            tracing::error!(error = %error, "desktop WebMCP listener stopped unexpectedly");
        }
    });
    context.state.lock().await.webmcp_event_hub = Some(server.event_hub());
    *bridge_slot = Some(DesktopWebmcpBridge { server, task, endpoint: endpoint.clone() });
    Ok(protocol::WebmcpPairResult {
        endpoint,
        origin: origin.to_string(),
        pairing_code: pairing.code().to_string(),
        expires_in_secs: pairing.expires_in().as_secs(),
    })
}

async fn stop_webmcp_bridge(context: &ServiceContext) {
    let bridge = context.webmcp.lock().await.take();
    context.state.lock().await.webmcp_event_hub = None;
    drop(bridge);
}

#[derive(Clone)]
struct DesktopWebmcpRuntime {
    workspace: FilesystemWorkspace,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
}

#[async_trait]
impl RuntimeAdapter for DesktopWebmcpRuntime {
    async fn status(&self) -> vtcode_webmcp::Result<RuntimeStatus> {
        let mut status = self.workspace.status().await?;
        let state = self.state.lock().await;
        status.turns_available = state.active_turn_id.is_none()
            && state.session_id.as_deref().is_some_and(|session_id| {
                state
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.session_id == session_id && !runtime.input.is_closed())
            });
        status.approval_authority = "active VT Code Desktop session".into();
        Ok(status)
    }

    async fn list_files(&self) -> vtcode_webmcp::Result<Vec<WorkspaceFile>> {
        self.workspace.list_files().await
    }

    async fn read_file(&self, path: &str) -> vtcode_webmcp::Result<FileSnapshot> {
        self.workspace.read_file(path).await
    }

    async fn propose_changes(&self, changes: Vec<FileChange>) -> vtcode_webmcp::Result<PatchProposal> {
        self.workspace.propose_changes(changes).await
    }

    async fn apply_proposal(&self, _proposal_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn run_checks(&self, _command: &str) -> vtcode_webmcp::Result<CheckResult> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn revert_last_change(&self, _change_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn request_turn(&self, prompt: &str, proposal_id: Option<&str>) -> vtcode_webmcp::Result<TurnResult> {
        let prompt =
            crate::agent::runloop::unified::webmcp::build_turn_prompt(&self.workspace, prompt, proposal_id).await?;
        let session_id = {
            let state = self.state.lock().await;
            state.session_id.clone().ok_or_else(|| {
                vtcode_webmcp::WebmcpError::Unsupported("select a VT Code Desktop session first".into())
            })?
        };
        let accepted = enqueue_turn(&self.state, &self.notifications, &session_id, prompt, Vec::new())
            .await
            .map_err(|error| vtcode_webmcp::WebmcpError::Adapter(error.to_string()))?;
        Ok(TurnResult { turn_id: accepted.turn_id, accepted: true })
    }
}

async fn start_session(context: &ServiceContext, session_id: String, resume: Option<ResumeSession>) -> Result<()> {
    crate::main_helpers::set_runtime_archive_session_id(Some(session_id.clone()));
    let vtcode_ui::tui::app::HostSession { session, input, commands } = new_host_session();
    let (command_sender, command_receiver) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let command_forwarder = std::thread::Builder::new()
        .name("vtcode-desktop-host-commands".to_string())
        .spawn(move || forward_host_commands(commands, command_sender))
        .context("failed to start desktop host command bridge")?;
    let (event_sender, event_receiver) = protocol::live_event_channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let config = active_agent_config(context).await;
    let mut vt_config = context.startup.config.clone();
    vt_config.agent.theme.clone_from(&config.theme);
    let options = context.state.lock().await.session_options.clone();
    let skip_confirmations = session_options::skip_confirmations(&options, context.startup.skip_confirmations);
    if let Some(mode) = options.mode.as_ref() {
        vt_config.default_primary_agent.clone_from(mode);
    }
    let primary_agent_explicitly_configured =
        options.mode.is_some() || context.startup.primary_agent_explicitly_configured;
    let planning_entry_source = session_options::planning_source(&options, context.startup.planning_entry_source);
    let runtime_task = tokio::spawn(async move {
        run_single_agent_loop_unified_with_host(
            &config,
            Some(vt_config),
            skip_confirmations,
            false,
            primary_agent_explicitly_configured,
            planning_entry_source,
            resume,
            None,
            session,
            event_sender,
        )
        .await
    });
    let abort = runtime_task.abort_handle();
    let (exit_sender, exit) = oneshot::channel();
    tokio::spawn(async move {
        let result = match runtime_task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(sanitize_public_error(&error.to_string())),
            Err(error) => Err(sanitize_public_error(&error.to_string())),
        };
        let _ = exit_sender.send(result);
    });
    spawn_session_pump(
        session_id.clone(),
        command_receiver,
        event_receiver,
        std::sync::Arc::clone(&context.state),
        context.notifications.clone(),
        std::sync::Arc::clone(&context.turn_finished),
    );
    context.state.lock().await.runtime = Some(RuntimeSession { session_id, input, exit, abort, command_forwarder });
    Ok(())
}

fn forward_host_commands(commands: HostCommandReceiver, sender: mpsc::Sender<InlineCommand>) {
    while let Ok(command) = commands.recv() {
        if sender.blocking_send(command).is_err() {
            break;
        }
    }
}

fn spawn_session_pump(
    session_id: String,
    mut commands: mpsc::Receiver<InlineCommand>,
    events: protocol::LiveEventReceiver,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
) {
    tokio::spawn(async move {
        let mut commands_open = true;
        let mut events_open = true;
        while commands_open || events_open {
            tokio::select! {
                command = commands.recv(), if commands_open => match command {
                    Some(command) => {
                        if let InlineCommand::SetPrimaryAgent { name, .. } = &command
                            && let Err(error) = publish_primary_agent_changed(
                                &session_id,
                                name.as_deref(),
                                &state,
                                &notifications,
                            )
                            .await
                        {
                            tracing::warn!(target: "vtcode.desktop", error = %error, "failed to publish active primary agent");
                        }
                        if let Some(view) = host_interaction_from_command(&command) {
                            if let Err(error) = publish_interaction(
                                &session_id,
                                view,
                                &state,
                                &notifications,
                            ).await {
                                tracing::warn!(target: "vtcode.desktop", error = %error, "failed to publish runtime interaction");
                            }
                        }
                    }
                    None => commands_open = false,
                },
                event = events.recv(), if events_open => {
                    report_dropped_events(&events);
                    match event {
                        Some(event) => {
                            if let Err(error) = publish_thread_event(
                                &session_id,
                                event,
                                &state,
                                &notifications,
                                &turn_finished,
                            ).await {
                                tracing::warn!(target: "vtcode.desktop", error = %error, "failed to publish runtime event");
                            }
                        }
                        None => events_open = false,
                    }
                },
            }
        }

        let completed_turn = {
            let mut state = state.lock().await;
            if state.session_id.as_deref() == Some(session_id.as_str()) {
                let turn_id = state.active_turn_id.take();
                if let Some(turn_id) = turn_id.as_deref() {
                    state.cancel_requested_turn = None;
                    state.interaction_registry.finish_turn(&session_id, turn_id);
                    state.pending_interactions.retain(|_, interaction| {
                        interaction.session_id != session_id || interaction.turn_id != turn_id
                    });
                }
                turn_id
            } else {
                None
            }
        };
        if let Some(turn_id) = completed_turn {
            let _ = send_notification(
                &notifications,
                protocol::method::TURN_STATUS,
                json!({ "sessionId": session_id, "turnId": turn_id, "status": "failed" }),
            )
            .await;
            turn_finished.notify_waiters();
        }
    });
}

async fn publish_primary_agent_changed(
    session_id: &str,
    display_name: Option<&str>,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
) -> Result<()> {
    if state.lock().await.session_id.as_deref() != Some(session_id) {
        return Ok(());
    }

    let notification = protocol::PrimaryAgentChanged {
        session_id: session_id.to_string(),
        display_name: sanitize_primary_agent_display(display_name),
    };
    send_notification(notifications, protocol::method::PRIMARY_AGENT_CHANGED, serde_json::to_value(notification)?).await
}

fn sanitize_primary_agent_display(display_name: Option<&str>) -> Option<String> {
    let sanitized = display_name?
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect::<String>();
    (!sanitized.trim().is_empty()).then_some(sanitized)
}

/// Surface live-stream degradation from a saturated coalescing queue.
///
/// Displaced snapshots are best-effort previews; dropped ordered events are a
/// transcript gap the client can heal by reloading the session, so both are
/// logged rather than failing the producing turn.
fn report_dropped_events(events: &protocol::LiveEventReceiver) {
    let stale = events.take_dropped_stale();
    if stale > 0 {
        tracing::debug!(target: "vtcode.desktop", stale, "displaced stale streaming snapshots under saturation");
    }
    let ordered = events.take_dropped_ordered();
    if ordered > 0 {
        tracing::warn!(target: "vtcode.desktop", ordered, "dropped ordered runtime events under saturation; reload the session to resync");
    }
}

async fn publish_thread_event(
    session_id: &str,
    event: vtcode_core::exec::events::VersionedThreadEvent,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    turn_finished: &Notify,
) -> Result<()> {
    use vtcode_core::exec::events::ThreadEvent;

    let terminal_status = match event.event() {
        ThreadEvent::TurnCompleted(_) => Some("completed"),
        ThreadEvent::TurnFailed(_) => Some("failed"),
        ThreadEvent::TurnBlocked(_) => Some("blocked"),
        _ => None,
    };
    let (turn_id, webmcp_event_hub) = {
        let mut state = state.lock().await;
        if state.session_id.as_deref().is_some_and(|selected| selected != session_id) {
            return Ok(());
        }
        let turn_id = state.active_turn_id.clone();
        if terminal_status.is_some() && turn_id.is_some() && state.session_id.as_deref() == Some(session_id) {
            if let Some(turn_id) = turn_id.as_deref() {
                state.interaction_registry.finish_turn(session_id, turn_id);
                state
                    .pending_interactions
                    .retain(|_, interaction| interaction.session_id != session_id || interaction.turn_id != turn_id);
            }
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
        }
        (turn_id, state.webmcp_event_hub.clone())
    };

    if let Some(event_hub) = webmcp_event_hub
        && let Err(error) = event_hub.publish(event.clone())
    {
        tracing::warn!(target: "vtcode.desktop", error = %error, "failed to publish canonical runtime event to WebMCP");
    }

    let notification = ThreadEventNotification {
        session_id: session_id.to_string(),
        turn_id: turn_id.clone(),
        event,
    };
    send_notification(notifications, protocol::method::THREAD_EVENT, serde_json::to_value(notification)?).await?;
    if let (Some(status), Some(turn_id)) = (terminal_status, turn_id) {
        send_notification(
            notifications,
            protocol::method::TURN_STATUS,
            json!({ "sessionId": session_id, "turnId": turn_id, "status": status }),
        )
        .await?;
        turn_finished.notify_waiters();
    }
    Ok(())
}

async fn publish_interaction(
    session_id: &str,
    view: HostInteraction,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
) -> Result<()> {
    let interaction_id = uuid::Uuid::new_v4().to_string();
    let mut state = state.lock().await;
    if state.session_id.as_deref() != Some(session_id) {
        return Ok(());
    }
    let turn_id = state.active_turn_id.clone().unwrap_or_else(|| "session-start".to_string());
    let steps = view
        .steps
        .iter()
        .map(|step| InteractionStep {
            title: step.title.clone(),
            question: step.question.clone(),
            choices: step
                .choices
                .iter()
                .map(|choice| InteractionChoice {
                    id: choice.id.clone(),
                    label: choice.label.clone(),
                    description: choice.description.clone(),
                    recommended: choice.label.to_ascii_lowercase().contains("recommended"),
                })
                .collect(),
            selected_choice_id: step.selected_choice_id.clone(),
            allow_freeform: step.allow_freeform,
        })
        .collect::<Vec<_>>();
    let choices = steps.first().map(|step| step.choices.clone()).unwrap_or_default();
    let request = InteractionRequest {
        session_id: session_id.to_string(),
        turn_id: turn_id.clone(),
        interaction_id: interaction_id.clone(),
        kind: protocol_interaction_kind(view.kind),
        wizard: view.wizard,
        title: view.title.clone(),
        details: view.details.clone(),
        choices,
        steps,
        selected_choice_id: view.selected_choice_ids.first().cloned(),
        selected_choice_ids: view.selected_choice_ids.clone(),
    };
    state.interaction_registry.insert(&request)?;
    state
        .pending_interactions
        .insert(interaction_id, PendingInteraction { session_id: session_id.to_string(), turn_id, view });
    drop(state);
    send_notification(notifications, protocol::method::INTERACTION_REQUESTED, serde_json::to_value(request)?).await
}

async fn respond_to_interaction(context: &ServiceContext, response: protocol::InteractionResponse) -> Result<()> {
    let mut choice_ids = response.choice_ids.clone();
    if choice_ids.is_empty()
        && let Some(choice_id) = response.choice_id.as_ref()
    {
        choice_ids.push(choice_id.clone());
    }
    let mut answer_texts = response.answer_texts.clone();
    if answer_texts.is_empty() && response.text.is_some() {
        answer_texts.push(response.text.clone());
    }
    let mut state = context.state.lock().await;
    let runtime = state
        .runtime
        .as_ref()
        .ok_or_else(|| anyhow!("selected session is not running"))?;
    if runtime.session_id != response.session_id {
        bail!("interaction response is stale");
    }
    let input = runtime.input.clone();
    dispatch_interaction_response(&mut state, &response, &choice_ids, &answer_texts, &input)
}

fn dispatch_interaction_response(
    state: &mut ServerState,
    response: &protocol::InteractionResponse,
    choice_ids: &[String],
    answer_texts: &[Option<String>],
    input: &mpsc::UnboundedSender<InlineEvent>,
) -> Result<()> {
    let pending = state
        .pending_interactions
        .get(&response.interaction_id)
        .cloned()
        .ok_or_else(|| anyhow!("interaction is no longer pending"))?;
    if pending.session_id != response.session_id || pending.turn_id != response.turn_id {
        bail!("interaction response is stale");
    }
    if choice_ids.iter().any(|choice_id| {
        !pending
            .view
            .steps
            .iter()
            .any(|step| step.choices.iter().any(|choice| &choice.id == choice_id))
    }) {
        bail!("interaction choice is invalid");
    }
    if pending.view.steps.len() > 1 && !response.cancelled && choice_ids.len() != pending.view.steps.len() {
        bail!("every interaction step requires a response");
    }
    let event = host_interaction_response(&pending.view, choice_ids, answer_texts, response.cancelled)
        .ok_or_else(|| anyhow!("interaction response is incomplete"))?;

    // Keep the interaction retryable unless the runtime accepted the event.
    state.interaction_registry.validate(response)?;
    input
        .send(event)
        .map_err(|_error| anyhow!("selected session runtime has stopped"))?;
    state.interaction_registry.resolve(response)?;
    state.pending_interactions.remove(&response.interaction_id);
    Ok(())
}

async fn shutdown_current_session(context: &ServiceContext, cancel_active: bool) {
    let (session_id, input, active_turn, should_send_cancel, mut runtime) = {
        let mut state = context.state.lock().await;
        let active_turn = state.active_turn_id.clone();
        if active_turn.is_some() && !cancel_active {
            return;
        }
        let should_send_cancel = active_turn
            .as_ref()
            .is_some_and(|turn_id| state.cancel_requested_turn.as_deref() != Some(turn_id.as_str()));
        if should_send_cancel {
            state.cancel_requested_turn = active_turn.clone();
        }
        let runtime = state.runtime.take();
        let session_id = runtime.as_ref().map(|session| session.session_id.clone());
        let input = runtime.as_ref().map(|session| session.input.clone());
        (session_id, input, active_turn, should_send_cancel, runtime)
    };

    if let Some(input) = input {
        if active_turn.is_some() {
            if should_send_cancel {
                let _ = input.send(InlineEvent::Cancel);
            }
            let _ = timeout(SESSION_SHUTDOWN_TIMEOUT, wait_for_active_turn_to_finish(context)).await;
        }
        let _ = input.send(InlineEvent::Exit);
    }

    if let Some(mut runtime) = runtime.take() {
        match timeout(RUNLOOP_EXIT_TIMEOUT, &mut runtime.exit).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => {
                tracing::debug!(target: "vtcode.desktop", session_id = %runtime.session_id, error, "desktop runloop exited with an error")
            }
            Ok(Err(_)) => {}
            Err(_) => {
                runtime.abort.abort();
                tracing::warn!(target: "vtcode.desktop", session_id = %runtime.session_id, "desktop runloop did not stop before timeout");
            }
        }
        match tokio::task::spawn_blocking(move || runtime.command_forwarder.join()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) => tracing::warn!(target: "vtcode.desktop", "desktop host command bridge panicked"),
            Err(error) => {
                tracing::warn!(target: "vtcode.desktop", %error, "failed to join desktop host command bridge")
            }
        }
    }

    if let Some(session_id) = session_id {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() == Some(session_id.as_str()) {
            state.session_id = None;
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
            state.interaction_registry.close_session(&session_id);
            state.pending_interactions.retain(|_, pending| pending.session_id != session_id);
        }
        crate::main_helpers::set_runtime_archive_session_id(None);
    }
    context.turn_finished.notify_waiters();
}

async fn wait_for_active_turn_to_finish(context: &ServiceContext) {
    loop {
        let notified = context.turn_finished.notified();
        if context.state.lock().await.active_turn_id.is_none() {
            return;
        }
        notified.await;
    }
}

async fn ensure_no_active_turn(context: &ServiceContext) -> Result<()> {
    if context.state.lock().await.active_turn_id.is_some() {
        bail!("cancel the active turn and wait for cleanup before switching sessions");
    }
    Ok(())
}

async fn send_notification(sender: &mpsc::Sender<protocol::Notification>, method: &str, params: Value) -> Result<()> {
    sender
        .send(protocol::Notification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        })
        .await
        .map_err(|_error| anyhow!("desktop client disconnected"))
}

fn resolve_review_scope(workspace: &Path, scope: Option<&str>) -> Result<PathBuf> {
    let scope = scope.filter(|scope| !scope.is_empty()).unwrap_or(".");
    if scope.len() > 4096 {
        bail!("Git review folder path is too long");
    }
    let resolved = vtcode_commons::resolve_workspace_path(workspace, Path::new(scope))?;
    if !resolved.starts_with(workspace) || !resolved.is_dir() {
        bail!("Git review scope must be a directory within the selected workspace");
    }
    Ok(resolved)
}

fn parse_params<T: DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).context("invalid request parameters")
}

fn protocol_interaction_kind(kind: vtcode_ui::tui::app::HostInteractionKind) -> InteractionKind {
    match kind {
        vtcode_ui::tui::app::HostInteractionKind::Approval => InteractionKind::Approval,
        vtcode_ui::tui::app::HostInteractionKind::PlanApproval => InteractionKind::PlanApproval,
        vtcode_ui::tui::app::HostInteractionKind::Question => InteractionKind::Question,
        vtcode_ui::tui::app::HostInteractionKind::Choice => InteractionKind::Choice,
        vtcode_ui::tui::app::HostInteractionKind::Other => InteractionKind::Other,
    }
}

fn session_summary(listing: &SessionListing) -> SessionSummary {
    SessionSummary {
        session_id: listing.identifier(),
        title: listing.first_prompt_preview(),
        workspace: listing.snapshot.metadata.workspace_path.clone(),
        completed: listing.snapshot.progress.is_none(),
        resumable: !listing.snapshot.messages.is_empty(),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RestoredInspectorHistory {
    items: Vec<vtcode_core::exec::events::ThreadItem>,
    incomplete: bool,
    activity_items: Vec<vtcode_core::exec::events::ThreadItem>,
    activity_incomplete: bool,
}

#[derive(Default)]
struct BoundedHistoryItems {
    items: Vec<vtcode_core::exec::events::ThreadItem>,
    seen: std::collections::HashSet<String>,
    encoded_bytes: usize,
    incomplete: bool,
    full: bool,
}

impl BoundedHistoryItems {
    fn retain_latest(&mut self, item: vtcode_core::exec::events::ThreadItem, byte_limit: usize) -> Result<()> {
        if self.full || !self.seen.insert(item.id.clone()) {
            return Ok(());
        }
        let size = serde_json::to_vec(&item)?.len();
        if self.items.len() >= 80 || self.encoded_bytes.saturating_add(size) > byte_limit {
            self.incomplete = true;
            self.full = true;
        } else {
            self.encoded_bytes += size;
            self.items.push(item);
        }
        Ok(())
    }
}

struct RestoredSpoolText {
    text: String,
    truncated: bool,
}

/// Read a historical tool-output spool only from the workspace's canonical
/// tool-output directory. Event logs are durable, but spool files are bounded,
/// optional evidence and may have been removed since the session completed.
fn read_restored_spool_text(workspace: &Path, spool_path: &str, max_bytes: usize) -> Option<RestoredSpoolText> {
    if max_bytes == 0 {
        return None;
    }

    let normalized = spool_path.replace('\\', "/");
    let has_foreign_drive_prefix = normalized.as_bytes().get(1) == Some(&b':');
    let path = Path::new(&normalized);
    if !path.is_absolute() && has_foreign_drive_prefix {
        return None;
    }
    let relative = if path.is_absolute() {
        path.strip_prefix(workspace).ok()?
    } else {
        path
    };
    if !SpooledOutputReference::recognizes_file_path(relative) {
        return None;
    }

    let mut file = vtcode_commons::fs::bound_file::open_file_beneath(workspace, relative).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }

    let mut bytes = Vec::with_capacity(max_bytes.min(MAX_RESTORED_SPOOLED_OUTPUT_BYTES));
    (&mut file).take(max_bytes as u64).read_to_end(&mut bytes).ok()?;
    let final_len = file.metadata().ok()?.len();
    let truncated = final_len > bytes.len() as u64 || std::str::from_utf8(&bytes).is_err();
    Some(RestoredSpoolText {
        text: String::from_utf8_lossy(&bytes).into_owned(),
        truncated,
    })
}

/// Restore bounded spool content into a historical ToolOutput item without
/// exposing its local path. A missing, stale, or unsafe reference leaves the
/// inline preview in place and marks inspector history incomplete.
fn restore_spooled_tool_output(
    workspace: &Path,
    item: &mut vtcode_core::exec::events::ThreadItem,
    encoded_budget: usize,
) -> Result<bool> {
    use vtcode_core::exec::events::ThreadItemDetails;

    let (spool_path, preview) = {
        let ThreadItemDetails::ToolOutput(output) = &mut item.details else {
            return Ok(false);
        };
        let Some(spool_path) = output.spool_path.take() else {
            return Ok(false);
        };
        (spool_path, std::mem::take(&mut output.output))
    };
    let Some(spooled) = read_restored_spool_text(workspace, &spool_path, MAX_RESTORED_SPOOLED_OUTPUT_BYTES) else {
        if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
            output.output = preview;
        }
        return Ok(true);
    };

    let baseline_size = serde_json::to_vec(item)?.len();
    let mut prefix_bytes = spooled.text.len();
    let mut truncated = spooled.truncated;
    loop {
        let visible_truncation = truncated || prefix_bytes < spooled.text.len();
        let prefix_end = (0..=prefix_bytes)
            .rev()
            .find(|index| spooled.text.is_char_boundary(*index))
            .unwrap_or(0);
        let mut candidate = spooled.text.get(..prefix_end).unwrap_or_default().to_string();
        if visible_truncation {
            candidate.push_str(RESTORED_OUTPUT_TRUNCATION_SUFFIX);
        }
        if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
            output.output = candidate;
        }
        let encoded_size = serde_json::to_vec(item)?.len();
        if encoded_size <= encoded_budget {
            return Ok(visible_truncation);
        }
        if prefix_end == 0 {
            if let ThreadItemDetails::ToolOutput(output) = &mut item.details {
                output.output = preview;
            }
            return Ok(true);
        }

        // JSON escaping can expand tool text. Scale the prefix to the remaining
        // encoded allowance, then retry at a UTF-8 boundary.
        let payload_size = encoded_size.saturating_sub(baseline_size);
        let payload_budget = encoded_budget.saturating_sub(baseline_size);
        prefix_bytes = prefix_end.saturating_mul(payload_budget) / payload_size.max(1);
        truncated = true;
    }
}

/// Read-only review projection over the shared canonical store. Lifecycle events
/// remain historical evidence and must never be replayed into the live runloop.
fn restored_inspector_history(workspace: &Path, session_id: &str) -> Result<RestoredInspectorHistory> {
    use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails};
    const MAX_REVIEW_BYTES: usize = 200 * 1024;
    const MAX_ACTIVITY_BYTES: usize = 64 * 1024;
    let tail = match vtcode_memory::snapshot::read_event_tail(workspace, session_id, 4 * 1024 * 1024) {
        Ok(Some(tail)) => tail,
        Ok(None) => return Ok(RestoredInspectorHistory::default()),
        Err(vtcode_memory::SessionStoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::Unsupported =>
        {
            return Ok(RestoredInspectorHistory {
                incomplete: true,
                activity_incomplete: true,
                ..Default::default()
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut review = BoundedHistoryItems { incomplete: tail.truncated, ..Default::default() };
    let mut activity = BoundedHistoryItems { incomplete: tail.truncated, ..Default::default() };
    for event in tail.events.into_iter().rev() {
        let mut item = match event.into_event() {
            ThreadEvent::ItemStarted(event) => event.item,
            ThreadEvent::ItemUpdated(event) => event.item,
            ThreadEvent::ItemCompleted(event) => event.item,
            _ => continue,
        };

        if matches!(&item.details, ThreadItemDetails::FileChange(_)) {
            review.retain_latest(item, MAX_REVIEW_BYTES)?;
            continue;
        }
        if matches!(&item.details, ThreadItemDetails::ToolOutput(_)) {
            if !activity.full && !activity.seen.contains(&item.id) {
                let remaining = MAX_ACTIVITY_BYTES.saturating_sub(activity.encoded_bytes);
                activity.incomplete |= restore_spooled_tool_output(workspace, &mut item, remaining)?;
            }
            activity.retain_latest(item, MAX_ACTIVITY_BYTES)?;
            continue;
        }
        if matches!(
            &item.details,
            ThreadItemDetails::ToolInvocation(_)
                | ThreadItemDetails::CommandExecution(_)
                | ThreadItemDetails::McpToolCall(_)
                | ThreadItemDetails::WebSearch(_)
                | ThreadItemDetails::Harness(_)
                | ThreadItemDetails::Error(_)
        ) {
            activity.retain_latest(item, MAX_ACTIVITY_BYTES)?;
        }
    }
    review.items.reverse();
    activity.items.reverse();
    Ok(RestoredInspectorHistory {
        items: review.items,
        incomplete: review.incomplete,
        activity_items: activity.items,
        activity_incomplete: activity.incomplete,
    })
}

fn restored_history(listing: &SessionListing) -> Vec<protocol::SessionHistoryMessage> {
    let mut messages = Vec::new();
    let mut total_bytes = 0_usize;

    for message in listing.snapshot.messages.iter().rev().take(MAX_RESTORED_MESSAGES) {
        let text = bounded_text_projection(&message.content.as_text(), MAX_RESTORED_MESSAGE_BYTES);
        let restored = protocol::SessionHistoryMessage { role: message.role.to_string(), text };
        let encoded_size = serde_json::to_vec(&restored).map_or(MAX_RESTORED_MESSAGE_BYTES, |bytes| bytes.len());
        if total_bytes.saturating_add(encoded_size) > MAX_RESTORED_HISTORY_BYTES {
            break;
        }
        total_bytes = total_bytes.saturating_add(encoded_size);
        messages.push(restored);
    }

    messages.reverse();
    messages
}

fn bounded_text_projection(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut bounded = text[..end].to_string();
    bounded.push_str("\n[Earlier content in this message was omitted.]");
    bounded
}

fn listing_matches_workspace(listing: &SessionListing, workspace: &Path) -> bool {
    let Some(listing_workspace) = session_workspace_path(listing) else {
        return false;
    };
    vtcode_commons::paths::canonicalize(listing_workspace).is_ok_and(|path| path == workspace)
}

fn workspace_label(workspace: &Path) -> String {
    workspace
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("workspace")
        .to_string()
}

/// The startup agent config with any `config/select` override applied.
async fn active_agent_config(context: &ServiceContext) -> vtcode_core::config::types::AgentConfig {
    let mut config = context.startup.agent_config.clone();
    let state = context.state.lock().await;
    if let Some(selection) = state.model_override.as_ref() {
        config.provider.clone_from(&selection.provider);
        config.model.clone_from(&selection.model);
    }
    if let Some(effort) = state
        .session_options
        .reasoning_effort
        .as_deref()
        .and_then(vtcode_commons::reasoning::ReasoningEffortLevel::parse)
    {
        config.reasoning_effort = effort;
    }
    if let Some(theme) = state.theme_override.as_ref() {
        config.theme.clone_from(theme);
    }
    config
}

async fn configuration_snapshot(
    context: &ServiceContext,
    agent: &vtcode_core::config::types::AgentConfig,
) -> ConfigurationSnapshot {
    // Tool-capable startup already ran the shared provider-auth preflight.
    // Reaching the config surface means required credentials are available,
    // or the selected provider does not require a credential.
    let setup = if agent.provider.trim().is_empty() {
        "vtcode auth".to_string()
    } else {
        format!("vtcode login {}", agent.provider)
    };
    let options = context.state.lock().await.session_options.clone();
    let skip_confirmations = session_options::skip_confirmations(&options, context.startup.skip_confirmations);
    ConfigurationSnapshot {
        session_options: Some(protocol::SessionOptionsSnapshot {
            permission_mode: if skip_confirmations {
                protocol::PermissionMode::AutoApproveCommands
            } else {
                protocol::PermissionMode::Ask
            },
            mode: options
                .mode
                .unwrap_or_else(|| context.startup.config.default_primary_agent.clone()),
            reasoning_effort: agent.reasoning_effort.as_str().into(),
            supported_efforts: session_options::supported_efforts(&agent.provider, &agent.model),
            speed: crate::agent::runloop::effective_speed_label(agent, &context.startup.config).map(str::to_owned),
        }),
        provider: agent.provider.clone(),
        model: agent.model.clone(),
        authenticated: true,
        credential_setup_command: setup,
        sandbox: Some(
            if skip_confirmations {
                "Auto-approved commands"
            } else {
                "Ask before protected actions"
            }
            .to_string(),
        ),
        shared_ui: Some(shared_preferences::snapshot(&context.startup.config.ui.keybindings, &agent.theme)),
        context_window: model_catalog_entry(&agent.provider, &agent.model).map(|entry| entry.context_window as u64),
    }
}

/// The selectable model catalog, mirroring the TUI picker's source.
fn catalog_models(active_provider: &str, active_model: &str) -> Vec<ModelInfo> {
    let recommended_models = ModelId::fallback_models();
    ModelId::all_models()
        .into_iter()
        .map(|model| {
            let provider = model.provider().as_ref().to_string();
            let id = model.as_str().to_string();
            let recommended_rank = recommended_models
                .iter()
                .position(|recommended| recommended == &model)
                .and_then(|rank| u8::try_from(rank).ok());
            ModelInfo {
                context_window: model_catalog_entry(&provider, &id).map(|entry| entry.context_window as u64),
                current: provider == active_provider && id == active_model,
                recommended_rank,
                display_name: model.display_name().to_string(),
                provider,
                id,
            }
        })
        .collect()
}

fn sanitize_public_error(message: &str) -> String {
    let sanitized = vtcode_commons::sanitizer::sanitize_provider_diagnostic(message)
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "desktop service request failed".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_catalog_preserves_cli_recommended_model_order() {
        let recommended = ModelId::fallback_models();
        let models = catalog_models("custom-provider", "custom-model");

        for (rank, model) in recommended.iter().enumerate() {
            let expected_provider = model.provider().as_ref().to_string();
            let expected_id = model.as_str();
            let actual = models
                .iter()
                .find(|entry| entry.provider == expected_provider && entry.id == expected_id)
                .expect("recommended model is available in catalog");
            assert_eq!(actual.recommended_rank, Some(u8::try_from(rank).expect("small recommendation rank")));
        }
        assert!(models.iter().any(|model| model.recommended_rank.is_none()));
    }

    #[tokio::test]
    async fn webmcp_turns_use_the_selected_desktop_runtime_and_respect_busy_state() {
        let workspace_root = tempfile::tempdir().expect("workspace");
        let workspace = FilesystemWorkspace::new(workspace_root.path(), [], false)
            .await
            .expect("workspace adapter");
        let (input, mut input_receiver) = mpsc::unbounded_channel();
        let (notifications, mut notification_receiver) = mpsc::channel(2);
        let (exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        let abort = runtime_task.abort_handle();
        let command_forwarder = std::thread::spawn(|| {});
        let server_state = ServerState {
            session_id: Some("desktop-session".into()),
            runtime: Some(RuntimeSession {
                session_id: "desktop-session".into(),
                input,
                exit,
                abort,
                command_forwarder,
            }),
            ..ServerState::default()
        };
        let state = std::sync::Arc::new(Mutex::new(server_state));
        let adapter = DesktopWebmcpRuntime {
            workspace,
            state: std::sync::Arc::clone(&state),
            notifications,
        };

        assert!(adapter.status().await.expect("status").turns_available);
        let accepted = adapter
            .request_turn("  inspect the workspace  ", None)
            .await
            .expect("accepted turn");
        assert!(accepted.accepted);
        let Some(InlineEvent::Submit(submission)) = input_receiver.recv().await else {
            panic!("WebMCP must enqueue through the desktop session input");
        };
        assert_eq!(submission.text, "inspect the workspace");
        assert_eq!(notification_receiver.recv().await.expect("turn status").method, protocol::method::TURN_STATUS);
        assert!(!adapter.status().await.expect("busy status").turns_available);
        assert!(adapter.request_turn("a second turn", None).await.is_err());
        assert!(input_receiver.try_recv().is_err(), "a busy runtime must not receive another turn");

        drop(exit_sender);
        runtime_task.abort();
    }

    #[cfg(unix)]
    fn review_item(id: &str, patch: &str) -> vtcode_core::exec::events::ThreadItem {
        use vtcode_core::exec::events::*;
        ThreadItem {
            id: id.into(),
            context: None,
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![FileUpdateChange {
                    path: "src/app.rs".into(),
                    kind: PatchChangeKind::Update,
                }],
                status: PatchApplyStatus::Completed,
                unified_diff: Some(patch.into()),
                diff_incomplete: Some(false),
                additions: None,
                deletions: None,
            })),
        }
    }

    #[cfg(unix)]
    fn tool_output_item(
        id: &str,
        spool_path: Option<&str>,
        inline_preview: &str,
    ) -> vtcode_core::exec::events::ThreadItem {
        use vtcode_core::exec::events::*;
        ThreadItem {
            id: id.into(),
            context: None,
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: format!("{id}-call"),
                tool_call_id: None,
                spool_path: spool_path.map(str::to_owned),
                output: inline_preview.into(),
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
            })),
        }
    }

    #[test]
    fn failed_interaction_delivery_keeps_response_retryable() {
        let interaction = InteractionRequest {
            session_id: "session-a".into(),
            turn_id: "turn-9".into(),
            interaction_id: "interaction-x".into(),
            kind: InteractionKind::Question,
            wizard: false,
            title: "Confirm action".into(),
            details: Vec::new(),
            choices: Vec::new(),
            steps: Vec::new(),
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        };
        let response = protocol::InteractionResponse {
            session_id: interaction.session_id.clone(),
            turn_id: interaction.turn_id.clone(),
            interaction_id: interaction.interaction_id.clone(),
            choice_id: None,
            choice_ids: Vec::new(),
            text: None,
            answer_texts: Vec::new(),
            cancelled: true,
        };
        let view = HostInteraction {
            kind: vtcode_ui::tui::app::HostInteractionKind::Question,
            wizard: false,
            title: interaction.title.clone(),
            details: Vec::new(),
            steps: Vec::new(),
            selected_choice_ids: Vec::new(),
        };
        let mut state = ServerState::default();
        state.interaction_registry.insert(&interaction).expect("register interaction");
        state.pending_interactions.insert(
            interaction.interaction_id.clone(),
            PendingInteraction {
                session_id: interaction.session_id.clone(),
                turn_id: interaction.turn_id.clone(),
                view,
            },
        );

        let (closed_input, closed_receiver) = mpsc::unbounded_channel();
        drop(closed_receiver);
        let error = dispatch_interaction_response(&mut state, &response, &[], &[], &closed_input)
            .expect_err("closed runtime rejects delivery");
        assert!(error.to_string().contains("runtime has stopped"));
        assert!(state.pending_interactions.contains_key(&interaction.interaction_id));
        assert_eq!(state.interaction_registry.validate(&response), Ok(()));

        let (live_input, mut live_receiver) = mpsc::unbounded_channel();
        dispatch_interaction_response(&mut state, &response, &[], &[], &live_input).expect("retry reaches runtime");
        assert!(matches!(live_receiver.try_recv(), Ok(InlineEvent::Transient(_))));
        assert!(!state.pending_interactions.contains_key(&interaction.interaction_id));
        assert!(state.interaction_registry.validate(&response).is_err());
        assert!(dispatch_interaction_response(&mut state, &response, &[], &[], &live_input).is_err());
        assert!(live_receiver.try_recv().is_err(), "a consumed response must not be delivered twice");
    }

    #[cfg(unix)]
    #[test]
    fn restored_review_uses_latest_canonical_items_without_lifecycle_replay() {
        use vtcode_core::exec::events::*;
        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        assert_eq!(restored_inspector_history(&workspace, "legacy").unwrap(), RestoredInspectorHistory::default());
        let log = vtcode_memory::open(&workspace, "review", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        log.append(&ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "review".into() }))
            .unwrap();
        log.append(&ThreadEvent::ItemStarted(ItemStartedEvent { item: review_item("first", "old patch") }))
            .unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("other", "other patch") }))
            .unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("first", "latest patch") }))
            .unwrap();
        log.flush().unwrap();
        let RestoredInspectorHistory { items, incomplete, .. } =
            restored_inspector_history(&workspace, "review").unwrap();
        assert!(!incomplete);
        assert_eq!(
            items,
            vec![
                review_item("other", "other patch"),
                review_item("first", "latest patch")
            ]
        );
        // An oversized newest patch must not expose an older patch as current.
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: review_item("huge", &"x".repeat(210 * 1024)),
        }))
        .unwrap();
        log.flush().unwrap();
        let RestoredInspectorHistory { items, incomplete, .. } =
            restored_inspector_history(&workspace, "review").unwrap();
        assert!(items.is_empty());
        assert!(incomplete);
    }

    #[cfg(unix)]
    #[test]
    fn restored_activity_is_latest_ordered_and_cannot_displace_saved_patches() {
        use vtcode_core::exec::events::*;
        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        let log = vtcode_memory::open(&workspace, "activity", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        let command = |id: &str, output: &str, status| ThreadItem {
            id: id.into(),
            context: None,
            details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                command: "cargo check --locked".into(),
                arguments: None,
                aggregated_output: output.into(),
                exit_code: None,
                status,
            })),
        };
        log.append(&ThreadEvent::ItemStarted(ItemStartedEvent {
            item: command("checked", "starting", CommandExecutionStatus::InProgress),
        }))
        .unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: review_item("edit", "saved patch") }))
            .unwrap();
        let completed = command("checked", "check passed", CommandExecutionStatus::Completed);
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: completed.clone() }))
            .unwrap();
        let interrupted = command("unfinished", "partial output", CommandExecutionStatus::InProgress);
        log.append(&ThreadEvent::ItemUpdated(ItemUpdatedEvent { item: interrupted.clone() }))
            .unwrap();
        log.flush().unwrap();
        let restored = restored_inspector_history(&workspace, "activity").unwrap();
        assert_eq!(restored.items, vec![review_item("edit", "saved patch")]);
        assert_eq!(restored.activity_items, vec![completed, interrupted]);
        assert!(!restored.activity_incomplete);
        // Oversized activity stops only its own suffix; review remains intact.
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: command("huge", &"x".repeat(65 * 1024), CommandExecutionStatus::Completed),
        }))
        .unwrap();
        log.flush().unwrap();
        let restored = restored_inspector_history(&workspace, "activity").unwrap();
        assert_eq!(restored.items, vec![review_item("edit", "saved patch")]);
        assert!(!restored.incomplete);
        assert!(restored.activity_items.is_empty());
        assert!(restored.activity_incomplete);
        // The count cap retains a contiguous newest suffix in persisted order.
        for index in 0..81 {
            log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: command(&format!("command-{index}"), "ok", CommandExecutionStatus::Completed),
            }))
            .unwrap();
        }
        log.flush().unwrap();
        let restored = restored_inspector_history(&workspace, "activity").unwrap();
        assert_eq!(restored.activity_items.len(), 80);
        assert_eq!(restored.activity_items[0].id, "command-1");
        assert_eq!(restored.activity_items[79].id, "command-80");
        assert!(restored.activity_incomplete);
        assert_eq!(restored.items.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn restored_activity_recovers_bounded_spooled_tool_output() {
        use vtcode_core::exec::events::*;

        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        let spool_file = workspace.join(".vtcode/context/tool_outputs/call-1.txt");
        let spool_path = spool_file.to_string_lossy().into_owned();
        std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
        std::fs::write(&spool_file, "complete captured tool output\nsecond line").unwrap();

        let log = vtcode_memory::open(&workspace, "spooled-output", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: tool_output_item("tool-output-1", Some(&spool_path), "bounded preview"),
        }))
        .unwrap();
        log.flush().unwrap();

        let restored = restored_inspector_history(&workspace, "spooled-output").unwrap();
        assert!(!restored.activity_incomplete);
        let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
            panic!("expected a restored tool output item");
        };
        assert_eq!(output.output, "complete captured tool output\nsecond line");
        assert_eq!(output.spool_path, None, "local paths must not cross the protocol boundary");
    }

    #[cfg(unix)]
    #[test]
    fn restored_activity_rejects_unavailable_or_unsafe_spools_and_reports_partial_history() {
        use vtcode_core::exec::events::*;

        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_file = outside.path().join("private.txt");
        std::fs::write(&outside_file, "outside secret").unwrap();
        std::fs::write(workspace.join("private.txt"), "workspace secret").unwrap();
        let spool_dir = workspace.join(".vtcode/context/tool_outputs");
        std::fs::create_dir_all(&spool_dir).unwrap();
        std::os::unix::fs::symlink(&outside_file, spool_dir.join("linked.txt")).unwrap();

        let log = vtcode_memory::open(&workspace, "unsafe-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        for (id, path, preview) in [
            ("symlink", ".vtcode/context/tool_outputs/linked.txt", "symlink preview"),
            ("missing", ".vtcode/context/tool_outputs/missing.txt", "missing preview"),
            ("escape", ".vtcode/context/tool_outputs/../../../private.txt", "escape preview"),
        ] {
            log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: tool_output_item(id, Some(path), preview),
            }))
            .unwrap();
        }
        log.flush().unwrap();

        let restored = restored_inspector_history(&workspace, "unsafe-spool").unwrap();
        assert!(restored.activity_incomplete);
        assert_eq!(restored.activity_items.len(), 3);
        for (item, expected_preview) in
            restored
                .activity_items
                .iter()
                .zip(["symlink preview", "missing preview", "escape preview"])
        {
            let ThreadItemDetails::ToolOutput(output) = &item.details else {
                panic!("expected the inline preview to be retained");
            };
            assert_eq!(output.output, expected_preview);
            assert_eq!(output.spool_path, None);
            assert!(!output.output.contains("outside secret"));
            assert!(!output.output.contains("workspace secret"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn restored_activity_truncates_large_spooled_output_with_visible_notice() {
        use vtcode_core::exec::events::*;

        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        let spool_path = ".vtcode/context/tool_outputs/large.txt";
        let spool_file = workspace.join(spool_path);
        std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
        std::fs::write(&spool_file, "x".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES + 1)).unwrap();

        let log = vtcode_memory::open(&workspace, "large-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: tool_output_item("tool-output-1", Some(spool_path), "preview"),
        }))
        .unwrap();
        log.flush().unwrap();

        let restored = restored_inspector_history(&workspace, "large-spool").unwrap();
        assert!(restored.activity_incomplete);
        let ThreadItemDetails::ToolOutput(output) = &restored.activity_items[0].details else {
            panic!("expected a restored tool output item");
        };
        assert!(output.output.starts_with(&"x".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES - 64)));
        assert!(output.output.ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));
    }

    #[cfg(unix)]
    #[test]
    fn restored_activity_fits_json_escaped_spool_text_to_the_encoded_budget() {
        use vtcode_core::exec::events::*;

        let dir = tempfile::tempdir().unwrap();
        let workspace = vtcode_commons::canonicalize(dir.path()).unwrap();
        let spool_path = ".vtcode/context/tool_outputs/escaped.txt";
        let spool_file = workspace.join(spool_path);
        std::fs::create_dir_all(spool_file.parent().unwrap()).unwrap();
        std::fs::write(&spool_file, "\u{0001}".repeat(MAX_RESTORED_SPOOLED_OUTPUT_BYTES)).unwrap();

        let log = vtcode_memory::open(&workspace, "escaped-spool", vtcode_memory::DEFAULT_MAX_EVENTS).unwrap();
        log.append(&ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: tool_output_item("tool-output-1", Some(spool_path), "preview"),
        }))
        .unwrap();
        log.flush().unwrap();

        let restored = restored_inspector_history(&workspace, "escaped-spool").unwrap();
        assert!(restored.activity_incomplete);
        let item = restored.activity_items.first().expect("tool output should remain visible");
        let ThreadItemDetails::ToolOutput(output) = &item.details else {
            panic!("expected a restored tool output item");
        };
        assert!(output.output.starts_with('\u{0001}'));
        assert!(output.output.ends_with(RESTORED_OUTPUT_TRUNCATION_SUFFIX));
        assert!(serde_json::to_vec(item).unwrap().len() <= 64 * 1024);
    }

    #[test]
    fn review_scope_is_bounded_to_existing_workspace_directories() {
        let workspace = tempfile::tempdir().unwrap();
        let root = vtcode_commons::canonicalize(workspace.path()).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::create_dir(root.join(" spaced ")).unwrap();
        std::fs::write(root.join("file"), "not a directory").unwrap();
        assert_eq!(resolve_review_scope(&root, None).unwrap(), root);
        assert_eq!(resolve_review_scope(&root, Some("src")).unwrap(), root.join("src"));
        assert_eq!(resolve_review_scope(&root, Some(" spaced ")).unwrap(), root.join(" spaced "));
        assert!(resolve_review_scope(&root, Some("file")).is_err());
        assert!(resolve_review_scope(&root, Some(outside.path().to_str().unwrap())).is_err());
        assert!(resolve_review_scope(&root, Some(&"x".repeat(4097))).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();
            assert!(resolve_review_scope(&root, Some("escape")).is_err());
        }
    }

    #[test]
    fn public_errors_remove_control_characters_and_limit_length() {
        let sanitized = sanitize_public_error(&format!("bad\u{001b}[31m{}", "x".repeat(300)));
        assert!(!sanitized.chars().any(char::is_control));
        assert!(sanitized.chars().count() <= 240);
    }

    #[test]
    fn public_errors_redact_provider_secrets() {
        let sanitized = sanitize_public_error("request failed with api_key=supersecretvalue and sk-1234567890abcdef");
        assert!(!sanitized.contains("supersecretvalue"));
        assert!(!sanitized.contains("sk-1234567890abcdef"));
        assert!(sanitized.contains("[REDACTED_SECRET]"));
    }

    #[test]
    fn session_workspace_labels_are_file_names() {
        assert_eq!(workspace_label(Path::new("/tmp/example-workspace")), "example-workspace");
        assert_eq!(workspace_label(Path::new("/")), "workspace");
    }

    #[tokio::test]
    async fn terminal_events_release_accepted_turns_without_a_start_event() {
        use vtcode_core::exec::events::{
            ThreadEvent, TurnCompletedEvent, TurnFailedEvent, Usage, VersionedThreadEvent,
        };
        for (turn_id, event, expected_status) in [
            (
                "first-turn",
                ThreadEvent::TurnFailed(TurnFailedEvent {
                    completed_at: None,
                    message: "turn cancelled during preparation".into(),
                    usage: None,
                }),
                "failed",
            ),
            (
                "later-turn",
                ThreadEvent::TurnFailed(TurnFailedEvent {
                    completed_at: None,
                    message: "turn cancelled during preparation".into(),
                    usage: None,
                }),
                "failed",
            ),
            (
                "dropped-start",
                ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    completed_at: None,
                    usage: Usage::default(),
                    in_progress_exec_sessions: Vec::new(),
                }),
                "completed",
            ),
        ] {
            let state = Mutex::new(ServerState {
                session_id: Some("session".into()),
                active_turn_id: Some(turn_id.into()),
                cancel_requested_turn: Some(turn_id.into()),
                ..Default::default()
            });
            let (notifications, mut received) = mpsc::channel(4);
            let finished = Notify::new();
            let waiting = finished.notified();
            tokio::pin!(waiting);
            let _already_notified = waiting.as_mut().enable();
            publish_thread_event("session", VersionedThreadEvent::new(event), &state, &notifications, &finished)
                .await
                .unwrap();
            timeout(Duration::from_secs(1), waiting).await.expect("cleanup waiter notified");
            let state = state.lock().await;
            assert!(state.active_turn_id.is_none(), "{turn_id}");
            assert!(state.cancel_requested_turn.is_none(), "{turn_id}");
            let event = received.try_recv().unwrap();
            assert_eq!(event.method, protocol::method::THREAD_EVENT);
            assert_eq!(event.params["turnId"], turn_id);
            let status = received.try_recv().unwrap();
            assert_eq!(status.params["turnId"], turn_id);
            assert_eq!(status.params["status"], expected_status);
        }
    }

    #[tokio::test]
    async fn old_session_events_do_not_reach_the_selected_session() {
        use vtcode_core::exec::events::{ThreadEvent, TurnStartedEvent, VersionedThreadEvent};
        let state = Mutex::new(ServerState {
            session_id: Some("selected-session".into()),
            active_turn_id: Some("selected-turn".into()),
            ..Default::default()
        });
        let (notifications, mut received) = mpsc::channel(2);
        publish_thread_event(
            "old-session",
            VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
            &state,
            &notifications,
            &Notify::new(),
        )
        .await
        .unwrap();
        assert_eq!(state.lock().await.active_turn_id.as_deref(), Some("selected-turn"));
        assert!(received.try_recv().is_err());
    }

    #[tokio::test]
    async fn primary_agent_updates_are_bounded_sanitized_and_session_scoped() {
        let state = Mutex::new(ServerState {
            session_id: Some("selected-session".into()),
            ..Default::default()
        });
        let (notifications, mut received) = mpsc::channel(3);

        publish_primary_agent_changed("old-session", Some("Stale"), &state, &notifications)
            .await
            .expect("stale notifications are ignored");
        assert!(received.try_recv().is_err());

        publish_primary_agent_changed("selected-session", Some(" Plan \u{7}Mode "), &state, &notifications)
            .await
            .expect("current notification is published");
        let update = received.try_recv().expect("current update");
        assert_eq!(update.method, protocol::method::PRIMARY_AGENT_CHANGED);
        assert_eq!(update.params["sessionId"], "selected-session");
        assert_eq!(update.params["displayName"], "Plan Mode");

        publish_primary_agent_changed("selected-session", None, &state, &notifications)
            .await
            .expect("reset is published");
        let reset = received.try_recv().expect("reset update");
        assert!(reset.params["displayName"].is_null());

        let long_name = "a".repeat(160);
        assert_eq!(sanitize_primary_agent_display(Some(&long_name)).unwrap().len(), 128);
        assert_eq!(sanitize_primary_agent_display(Some("\u{1}\n ")), None);
    }

    #[tokio::test]
    async fn session_pump_projects_primary_agent_host_commands() {
        let state = std::sync::Arc::new(Mutex::new(ServerState {
            session_id: Some("session-a".into()),
            ..Default::default()
        }));
        let (commands, command_receiver) = mpsc::channel(1);
        let (event_sender, event_receiver) = protocol::live_event_channel(1);
        let (notifications, mut received) = mpsc::channel(1);
        spawn_session_pump(
            "session-a".into(),
            command_receiver,
            event_receiver,
            state,
            notifications,
            std::sync::Arc::new(Notify::new()),
        );

        commands
            .send(InlineCommand::SetPrimaryAgent { name: Some("Plan".into()), color: None })
            .await
            .expect("queue active-agent command");
        drop(commands);
        drop(event_sender);

        let update = timeout(Duration::from_secs(1), received.recv())
            .await
            .expect("active-agent notification deadline")
            .expect("active-agent notification");
        assert_eq!(update.method, protocol::method::PRIMARY_AGENT_CHANGED);
        assert_eq!(update.params["sessionId"], "session-a");
        assert_eq!(update.params["displayName"], "Plan");
    }
}
