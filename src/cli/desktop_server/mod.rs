mod composer_resources;
mod config_settings;
mod mcp_app;
mod session_options;
mod session_preferences;
mod shared_preferences;
mod theme_preferences;

use std::collections::VecDeque;
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
    ContentPart, HostCommandReceiver, HostInteraction, InlineCommand, InlineEvent, TransientHotkeyAction,
    host_interaction_from_command, host_interaction_hotkey_response, host_interaction_response, new_host_session,
};
use vtcode_webmcp::runtime::{
    AppliedChange, CheckResult, FileSnapshot, PatchProposal, RuntimeAdapter, RuntimeStatus, TurnResult, WorkspaceFile,
};
use vtcode_webmcp::{FileChange, FilesystemWorkspace, WebmcpServer, WebmcpServerConfig};

use crate::agent::agents::{ResumeSession, load_resume_session};
use crate::agent::runloop::unified::QueuedInput;
use crate::agent::runloop::unified::stop_requests::StopHandleSlot;
use crate::agent::runloop::unified::turn::run_single_agent_loop_unified_with_host;
use crate::startup::StartupContext;

mod cancel;
#[cfg(test)]
use cancel::{CANCEL_ESCALATION, EscalationWaits};
use cancel::{escalation_waits_for, spawn_cancel_refire, spawn_cancel_watchdog, spawn_late_stop_signal};
mod catalog;
mod dispatch;
use dispatch::{
    ensure_no_active_turn, handle_request, join_host_command_bridge, parse_params, protocol_interaction_kind,
    send_notification, shutdown_current_session,
};
mod interaction_respond;
use interaction_respond::dispatch_interaction_response;
mod pump;
mod recent_prompts;
#[cfg(test)]
use pump::{publish_interaction, sanitize_primary_agent_display};
#[cfg(test)]
use pump::{publish_primary_agent_changed, publish_thread_event};
use pump::{respond_to_interaction, send_runtime_notification, spawn_session_pump};
mod turn_queue;
use turn_queue::{EnqueueMode, dispatch_drained_turn, enqueue_turn, publish_queue_status_sync};
mod turn_undo;
mod update_install;
use update_install::install_state;
mod webmcp;
#[cfg(test)]
use webmcp::install_session_runtime;
use webmcp::{DesktopWebmcpRuntime, ensure_session_not_held_elsewhere, start_session};
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

const MAX_RESTORED_MESSAGES: usize = protocol::MAX_SESSION_HISTORY_PAGE_MESSAGES;
const MAX_RESTORED_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_RESTORED_HISTORY_BYTES: usize = 700 * 1024;
const MAX_RESTORED_SPOOLED_OUTPUT_BYTES: usize = protocol::MAX_SESSION_OUTPUT_PAGE_BYTES;
const RESTORED_OUTPUT_TRUNCATION_SUFFIX: &str = protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
const INTERNAL_RUNTIME_GENERATION: &str = "_vtcodeRuntimeGeneration";
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
    /// Identity of the installed runtime, distinct from the durable session id.
    runtime_generation: Option<uuid::Uuid>,
    active_turn_id: Option<String>,
    session_transitioning: bool,
    cancel_requested_turn: Option<String>,
    /// Test-only escalation bounds so cancel-watchdog regressions run with
    /// short deterministic waits instead of the production grace periods.
    #[cfg(test)]
    escalation_waits: Option<EscalationWaits>,
    /// Prompts queued behind the active turn; drained FIFO on terminal events
    /// with CLI batching semantics. Desktop agent switches require an idle
    /// session, so every entry carries `primary_agent: None`.
    queue: VecDeque<QueuedInput>,
    /// Total oldest queued prompts discarded by the soft cap.
    queue_dropped_total: usize,
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
    /// Out-of-band stop handles published by the runloop after shell setup.
    /// Firing these is what actually stops a running turn; the queued
    /// `InlineEvent::Cancel` only dismisses overlay waits.
    stop: StopHandleSlot,
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
                    write_runtime_checked_notification(&context, &mut stdout, notification).await?;
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
                    write_runtime_checked_notification(context, stdout, notification).await?;
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
            write_runtime_checked_notification(context, stdout, notification).await?;
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

/// Runtime-originated notifications can remain buffered across a restart.
/// The tag is internal to this service and is removed before protocol output.
async fn write_runtime_checked_notification<W>(
    context: &ServiceContext,
    stdout: &mut W,
    mut notification: protocol::Notification,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if let Some(tag) = notification
        .params
        .as_object_mut()
        .and_then(|params| params.remove(INTERNAL_RUNTIME_GENERATION))
    {
        let generation = tag.as_str().and_then(|value| uuid::Uuid::parse_str(value).ok());
        let state = context.state.lock().await;
        if generation.is_none() || generation != state.runtime_generation {
            return Ok(());
        }
    }
    protocol::write_frame(stdout, &notification).await?;
    Ok(())
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
