mod composer_resources;
mod config_settings;
mod mcp_app;
mod session_options;
mod session_preferences;
mod shared_preferences;
mod theme_preferences;

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    list_recent_session_summaries_in_workspace, reserve_session_archive_identifier, session_workspace_path,
};
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    ConfigurationSnapshot, InteractionChoice, InteractionKind, InteractionRegistry, InteractionRequest,
    InteractionStep, ModelCapabilities, ModelInfo, ModelSelectParams, Request, RequestId, SessionSummary,
    ThreadEventNotification,
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

mod catalog;
mod dispatch;
use dispatch::{
    dispatch_interaction_response, ensure_no_active_turn, handle_request, parse_params, protocol_interaction_kind,
    resolve_review_scope, send_notification, session_summary, shutdown_current_session,
};
mod pump;
#[cfg(test)]
use pump::{publish_interaction, sanitize_primary_agent_display};
#[cfg(test)]
use pump::{publish_primary_agent_changed, publish_thread_event};
use pump::{respond_to_interaction, spawn_session_pump};
mod webmcp;
#[cfg(test)]
use webmcp::install_session_runtime;
use webmcp::{DesktopWebmcpRuntime, start_session};
mod restore;
mod secret_store;
mod storage;
use catalog::{
    active_agent_config, catalog_models, configuration_snapshot, effective_speed_config, listing_matches_workspace,
    sanitize_public_error, update_status_snapshot, workspace_label,
};
use restore::{RestoredInspectorHistory, read_restored_spool_page, restored_history_page, restored_inspector_history};
#[cfg(test)]
use restore::{read_restored_spool_text, restore_spooled_tool_output};

const MAX_SESSIONS: usize = 200;
const MAX_RESTORED_MESSAGES: usize = protocol::MAX_SESSION_HISTORY_PAGE_MESSAGES;
const MAX_RESTORED_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_RESTORED_HISTORY_BYTES: usize = 700 * 1024;
const MAX_RESTORED_SPOOLED_OUTPUT_BYTES: usize = protocol::MAX_SESSION_OUTPUT_PAGE_BYTES;
const RESTORED_OUTPUT_TRUNCATION_SUFFIX: &str = protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
const SESSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);
const RUNLOOP_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
#[error("resolve the pending startup interaction before submitting a turn")]
struct StartupInteractionPending;

#[derive(Default)]
struct ServerState {
    initialized: bool,
    session_id: Option<String>,
    runtime: Option<RuntimeSession>,
    active_turn_id: Option<String>,
    session_transitioning: bool,
    cancel_requested_turn: Option<String>,
    pending_interactions: std::collections::HashMap<String, PendingInteraction>,
    interaction_registry: InteractionRegistry,
    /// Provider/model override applied to every session started after a
    /// `config/select`; the session restarts so the runtime rebuilds cleanly.
    model_override: Option<ModelSelectParams>,
    /// Credentials resolved for `model_override` at select time. Always `Some`
    /// once an override is active; `OverrideAuth::error` records a failed
    /// resolution so the snapshot reports `authenticated: false`.
    model_override_auth: Option<catalog::OverrideAuth>,
    /// Credentials saved via `config/secret/store` for the boot provider,
    /// re-resolved at save time so in-app key saves apply without a restart.
    secret_store_auth: Option<catalog::StoredSecretAuth>,
    session_options: protocol::SessionOptionsSelectParams,
    permission_before_full_access: Option<protocol::PermissionMode>,
    /// Recent Desktop changes layered over the process-start CLI config until restart.
    fast_mode_overrides: std::collections::HashMap<String, bool>,
    /// Canonical spool paths for the selected session's paged historical outputs.
    /// Paths stay inside the service and are revalidated before each bounded read.
    restored_output_spools: std::collections::HashMap<String, String>,
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

impl DesktopWebmcpBridge {
    async fn stop(&mut self) {
        if self.task.is_finished() {
            return;
        }
        self.task.abort();
        let _ = (&mut self.task).await;
    }
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
                        keep_running = process_frame(&context, &mut stdout, &mut notification_rx, &frame).await?;
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

    shutdown_current_session(&context, true).await?;
    stop_webmcp_bridge(&context).await;
    let _ = stdout.flush().await;
    Ok(())
}

async fn process_frame<W>(
    context: &ServiceContext,
    stdout: &mut W,
    notifications: &mut mpsc::Receiver<protocol::Notification>,
    frame: &[u8],
) -> Result<bool>
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

    let response = if request.method == protocol::method::SHUTDOWN {
        // Shutdown can wait for the event pump, which itself may be blocked on
        // this bounded queue. Keep draining until the request is complete.
        let request_future = handle_request(context, &request);
        tokio::pin!(request_future);
        loop {
            tokio::select! {
                result = &mut request_future => break result,
                Some(notification) = notifications.recv() => {
                    protocol::write_frame(stdout, &notification).await?;
                }
            }
        }
    } else {
        handle_request(context, &request).await
    };
    if request.method == protocol::method::SHUTDOWN {
        // The pump can finish just after queueing its final event. Flush the
        // bounded backlog before sending the shutdown response and exiting.
        for _ in 0..protocol::DEFAULT_QUEUE_CAPACITY {
            let Ok(notification) = notifications.try_recv() else {
                break;
            };
            protocol::write_frame(stdout, &notification).await?;
        }
    }
    match response {
        Ok((result, should_stop)) => {
            let response = protocol::success_response(request.id, result)?;
            protocol::write_frame(stdout, &response).await?;
            Ok(!should_stop)
        }
        Err(error) => {
            tracing::warn!(target: "vtcode.desktop", method = %request.method, error = %error, "desktop request failed");
            let code = if error.downcast_ref::<StartupInteractionPending>().is_some() {
                protocol::STARTUP_INTERACTION_PENDING_ERROR
            } else {
                -32000
            };
            let response = protocol::error_response(Some(request.id), code, sanitize_public_error(&error.to_string()));
            protocol::write_frame(stdout, &response).await?;
            Ok(true)
        }
    }
}

async fn handle_request_inner(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    use protocol::method;

    if request.method == method::INITIALIZE {
        let params: protocol::InitializeParams = parse_params(&request.params)?;
        let selected = params.protocol.negotiate(protocol::PROTOCOL_VERSION)?;
        let result = protocol::InitializeResult {
            protocol: selected,
            workspace: context.workspace.to_string_lossy().into_owned(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            slash_commands: vtcode_core::ui::slash::visible_commands_for_desktop()
                .into_iter()
                .map(|command| protocol::SlashCommandInfo {
                    name: command.name.to_string(),
                    description: command.description.to_string(),
                })
                .collect(),
        };
        context.state.lock().await.initialized = true;
        return Ok((serde_json::to_value(result)?, false));
    }

    if request.method == method::SHUTDOWN {
        shutdown_current_session(context, true).await?;
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
            let snapshot = configuration_snapshot(context, &agent).await?;
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
            shutdown_current_session(context, false).await?;
            stop_webmcp_bridge(context).await;
            context.state.lock().await.session_id = None;
            Ok((serde_json::to_value(result)?, false))
        }
        method::CONFIG_OPEN_FILE => {
            let params: protocol::ConfigOpenFileParams = parse_params(&request.params)?;
            config_settings::open_file(context.workspace.clone(), params.target).await?;
            Ok((json!({ "opened": true }), false))
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
        method::UPDATE_STATUS => {
            let status = update_status_snapshot()?;
            Ok((serde_json::to_value(status)?, false))
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
            shutdown_current_session(context, false).await?;
            // Re-resolve credentials for the newly selected provider so later
            // turns authenticate as that provider. A failed resolution keeps
            // the selection but reports `authenticated: false`; the desktop
            // guides recovery instead of disconnecting.
            let api_key_env = if params.provider.eq_ignore_ascii_case(&context.startup.agent_config.provider) {
                context.startup.agent_config.api_key_env.clone()
            } else if let Some(custom) = context.startup.config.custom_provider(&params.provider) {
                custom.resolved_api_key_env()
            } else {
                vtcode_config::api_keys::api_key_env_var(&params.provider)
            };
            let override_auth =
                catalog::resolve_override_auth(context, &params.provider, &params.model, &api_key_env).await;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                state.model_override = Some(params);
                state.model_override_auth = Some(override_auth);
                state.session_options.reasoning_effort = None;
            }
            let agent = active_agent_config(context).await;
            let snapshot = configuration_snapshot(context, &agent).await?;
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::CONFIG_SECRET_STORE => {
            let params: protocol::SecretStoreParams = parse_params(&request.params)?;
            let result = secret_store::store(context, params).await?;
            Ok((serde_json::to_value(result)?, false))
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
            if params.permission_mode == Some(protocol::PermissionMode::FullAccess)
                && context.startup.full_auto_requested
            {
                bail!("Full access cannot be combined with Full auto");
            }
            let agent = active_agent_config(context).await;
            let speed_supported =
                crate::agent::runloop::unified::fast_mode::supports_fast_mode(&agent, &context.startup.config);
            let modes = session_options::discover_modes(context.workspace.clone()).await?;
            session_options::validate(&params, &agent.provider, &agent.model, speed_supported, &modes)?;
            if params.fast_mode.is_some() || params.mode.is_some() {
                session_preferences::persist(
                    context.workspace.clone(),
                    agent.clone(),
                    params.fast_mode,
                    params.mode.clone(),
                )
                .await?;
            }
            shutdown_current_session(context, false).await?;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                if let Some(enabled) = params.fast_mode {
                    state.fast_mode_overrides.insert(agent.provider.to_ascii_lowercase(), enabled);
                }
                if params.permission_mode == Some(protocol::PermissionMode::FullAccess)
                    && state.session_options.permission_mode != Some(protocol::PermissionMode::FullAccess)
                {
                    state.permission_before_full_access = Some(state.session_options.permission_mode.unwrap_or(
                        if context.startup.skip_confirmations {
                            protocol::PermissionMode::AutoApproveCommands
                        } else {
                            protocol::PermissionMode::Ask
                        },
                    ));
                } else if params.permission_mode.is_some()
                    && params.permission_mode != Some(protocol::PermissionMode::FullAccess)
                {
                    state.permission_before_full_access = None;
                }
                session_options::merge(&mut state.session_options, params);
            }
            let agent = active_agent_config(context).await;
            Ok((serde_json::to_value(configuration_snapshot(context, &agent).await?)?, false))
        }
        method::CONFIG_THEME => {
            let params: protocol::ThemePreferenceSelectParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let theme = theme_preferences::persist(context.workspace.clone(), params.theme).await?;
            shutdown_current_session(context, false).await?;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                state.theme_override = Some(theme.clone());
            }
            let agent = active_agent_config(context).await;
            let mut snapshot = configuration_snapshot(context, &agent).await?;
            if let Some(shared) = snapshot.shared_ui.as_mut() {
                shared.theme = theme;
            }
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::SESSION_LIST => {
            let sessions = list_recent_session_summaries_in_workspace(MAX_SESSIONS, &context.workspace).await?;
            let sessions = sessions
                .into_iter()
                .map(|session| SessionSummary {
                    session_id: session.identifier,
                    title: session.title,
                    workspace: session.workspace_path,
                    completed: session.completed,
                    resumable: session.resumable,
                    size_bytes: session.size_bytes,
                    updated_at: (session.updated_at != 0).then_some(session.updated_at),
                })
                .collect::<Vec<_>>();
            Ok((serde_json::to_value(sessions)?, false))
        }
        method::SESSION_CREATE => {
            let params: protocol::SessionCreateParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let create_started = Instant::now();
            let shutdown_started = Instant::now();
            let had_session = context.state.lock().await.session_id.is_some();
            shutdown_current_session(context, false).await?;
            if had_session {
                let mut state = context.state.lock().await;
                if state.session_options.permission_mode == Some(protocol::PermissionMode::FullAccess) {
                    state.session_options.permission_mode = state.permission_before_full_access.take();
                }
            }
            let shutdown_elapsed = shutdown_started.elapsed();
            let workspace_label = workspace_label(&context.workspace);
            let identifier_started = Instant::now();
            let session_id = if history_persistence_enabled() {
                reserve_session_archive_identifier(&workspace_label, None).await?
            } else {
                generate_session_archive_identifier(&workspace_label, None)
            };
            let identifier_elapsed = identifier_started.elapsed();
            let runtime_started = Instant::now();
            start_session(context, session_id.clone(), None).await?;
            let runtime_elapsed = runtime_started.elapsed();
            {
                let mut state = context.state.lock().await;
                state.session_id = Some(session_id.clone());
                state.restored_output_spools.clear();
            }
            tracing::debug!(
                target: "vtcode.desktop",
                shutdown_ms = shutdown_elapsed.as_millis(),
                archive_identifier_ms = identifier_elapsed.as_millis(),
                runtime_start_ms = runtime_elapsed.as_millis(),
                total_ms = create_started.elapsed().as_millis(),
                "desktop session created"
            );
            Ok((
                serde_json::to_value(SessionSummary {
                    session_id,
                    title: params.title,
                    workspace: context.workspace.to_string_lossy().into_owned(),
                    completed: false,
                    resumable: history_persistence_enabled(),
                    size_bytes: 0,
                    updated_at: None,
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
            let previous_session_id = context.state.lock().await.session_id.clone();
            shutdown_current_session(context, false).await?;
            {
                let mut state = context.state.lock().await;
                if previous_session_id.as_deref().is_some_and(|id| id != params.session_id)
                    && state.session_options.permission_mode == Some(protocol::PermissionMode::FullAccess)
                {
                    state.session_options.permission_mode = state.permission_before_full_access.take();
                }
            }
            let workspace = context.workspace.clone();
            let history_session_id = params.session_id.clone();
            let inspector_history =
                tokio::task::spawn_blocking(move || restored_inspector_history(&workspace, &history_session_id))
                    .await
                    .context("inspector history task failed")??;
            start_session(context, params.session_id.clone(), Some(resume)).await?;
            let RestoredInspectorHistory {
                items: review_items,
                incomplete: review_history_incomplete,
                activity_items,
                activity_incomplete: activity_history_incomplete,
                activity_output_cursors,
                output_spools,
            } = inspector_history;
            {
                let mut state = context.state.lock().await;
                state.session_id = Some(params.session_id.clone());
                state.restored_output_spools = output_spools;
            }
            let history = restored_history_page(&listing.snapshot.messages, None, MAX_RESTORED_MESSAGES);
            let result = protocol::SessionLoadResult {
                session: session_summary(&listing),
                messages: history.messages,
                history_start_index: history.start_index,
                history_has_earlier: history.has_earlier,
                review_items,
                review_history_incomplete,
                activity_items,
                activity_output_cursors,
                activity_history_incomplete,
            };
            Ok((serde_json::to_value(result)?, false))
        }
        method::SESSION_HISTORY => {
            let params: protocol::SessionHistoryParams = parse_params(&request.params)?;
            if params.request_id.is_empty() || params.request_id.len() > 128 {
                bail!("invalid session history request identity");
            }
            if params.limit == 0 || params.limit > protocol::MAX_SESSION_HISTORY_PAGE_MESSAGES {
                bail!("invalid session history page size");
            }
            let selected_id = context.state.lock().await.session_id.clone();
            if selected_id.as_deref() != Some(params.session_id.as_str()) {
                bail!("session is not selected");
            }
            let listing = find_session_by_identifier(&params.session_id)
                .await?
                .ok_or_else(|| anyhow!("saved session was not found"))?;
            if !listing_matches_workspace(&listing, &context.workspace) {
                bail!("saved session belongs to a different workspace");
            }
            if params.before_index > listing.snapshot.messages.len() {
                bail!("session history cursor is out of range");
            }
            let page = restored_history_page(&listing.snapshot.messages, Some(params.before_index), params.limit);
            let result = protocol::SessionHistoryPage {
                session_id: params.session_id,
                request_id: params.request_id,
                start_index: page.start_index,
                messages: page.messages,
                has_earlier: page.has_earlier,
            };
            Ok((serde_json::to_value(result)?, false))
        }
        method::SESSION_OUTPUT_READ => {
            let params: protocol::SessionOutputReadParams = parse_params(&request.params)?;
            if params.request_id.is_empty() || params.request_id.len() > 128 {
                bail!("invalid session output request identity");
            }
            if params.item_id.is_empty() || params.item_id.len() > 512 {
                bail!("invalid session output item identity");
            }
            let spool_path = {
                let state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str()) {
                    bail!("session is not selected");
                }
                state
                    .restored_output_spools
                    .get(&params.item_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("no more captured output is available"))?
            };
            let workspace = context.workspace.clone();
            let offset = params.offset;
            let text_page =
                tokio::task::spawn_blocking(move || read_restored_spool_page(&workspace, &spool_path, offset))
                    .await
                    .context("tool output page task failed")??;
            let page = protocol::SessionOutputPage {
                session_id: params.session_id,
                request_id: params.request_id,
                item_id: params.item_id,
                offset,
                next_offset: text_page.next_offset,
                total_bytes: text_page.total_bytes,
                text: text_page.text,
                has_more: text_page.next_offset < text_page.total_bytes,
            };
            Ok((serde_json::to_value(page)?, false))
        }
        method::MCP_APP_READ => {
            let params: protocol::McpAppReadParams = parse_params(&request.params)?;
            let result = mcp_app::read(context.workspace.clone(), params).await?;
            Ok((serde_json::to_value(result)?, false))
        }
        method::SESSION_CLOSE => {
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            let selected_id = context.state.lock().await.session_id.clone();
            if selected_id.as_deref() != Some(params.session_id.as_str()) {
                bail!("session is not selected");
            }
            ensure_no_active_turn(context).await?;
            shutdown_current_session(context, false).await?;
            {
                let mut state = context.state.lock().await;
                state.session_id = None;
                if state.session_options.permission_mode == Some(protocol::PermissionMode::FullAccess) {
                    state.session_options.permission_mode = state.permission_before_full_access.take();
                }
                state.restored_output_spools.clear();
            }
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
        method::STORAGE_SUMMARY | method::SESSION_DELETE | method::WORKTREE_LIST | method::WORKTREE_REMOVE => {
            storage::handle_storage_request(context, request).await
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
        if state.session_transitioning {
            bail!("a session change is in progress");
        }
        if state.session_id.as_deref() != Some(session_id) {
            bail!("session is not selected");
        }
        if state.active_turn_id.is_some() {
            bail!("another turn is already running");
        }
        if !state.pending_interactions.is_empty() {
            if state.pending_interactions.values().all(|interaction| {
                interaction.session_id == session_id && interaction.turn_id == protocol::SESSION_START_TURN_ID
            }) {
                return Err(StartupInteractionPending.into());
            }
            bail!("resolve the pending CLI interaction before submitting a turn");
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
    // The prompt has crossed the runtime boundary. Status is advisory: waiting
    // for this queue here can deadlock the request loop, and reporting failure
    // after delivery could cause a client to submit the same prompt twice.
    if let Err(error) = notifications.try_send(protocol::Notification {
        jsonrpc: "2.0".to_string(),
        method: protocol::method::TURN_STATUS.to_string(),
        params: json!({ "sessionId": session_id, "turnId": turn_id, "status": "running" }),
    }) {
        tracing::warn!(target: "vtcode.desktop", %error, "could not publish turn status");
    }
    Ok(protocol::TurnAccepted { session_id: session_id.to_string(), turn_id })
}

async fn webmcp_status(context: &ServiceContext) -> protocol::WebmcpBridgeStatus {
    reap_stopped_webmcp_bridge(context).await;
    let (session_id, turns_available) = {
        let state = context.state.lock().await;
        let session_id = state.session_id.clone();
        let turns_available = !state.session_transitioning
            && state.pending_interactions.is_empty()
            && state.active_turn_id.is_none()
            && state.runtime.as_ref().is_some_and(|runtime| {
                Some(runtime.session_id.as_str()) == session_id.as_deref() && !runtime.input.is_closed()
            });
        (session_id, turns_available)
    };
    let endpoint = context
        .webmcp
        .lock()
        .await
        .as_ref()
        .filter(|bridge| !bridge.task.is_finished())
        .map(|bridge| bridge.endpoint.clone());
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
    reap_stopped_webmcp_bridge(context).await;
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
    let mut bridge = context.webmcp.lock().await.take();
    context.state.lock().await.webmcp_event_hub = None;
    if let Some(bridge) = bridge.as_mut() {
        bridge.stop().await;
    }
}

async fn reap_stopped_webmcp_bridge(context: &ServiceContext) {
    let mut bridge = {
        let mut slot = context.webmcp.lock().await;
        if slot.as_ref().is_some_and(|bridge| bridge.task.is_finished()) {
            slot.take()
        } else {
            None
        }
    };
    if let Some(bridge) = bridge.as_mut() {
        context.state.lock().await.webmcp_event_hub = None;
        bridge.stop().await;
    }
}

#[cfg(test)]
mod tests;
