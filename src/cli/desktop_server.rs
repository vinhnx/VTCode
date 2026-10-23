use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, Notify, mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio::time::timeout;
use vtcode_core::core::threads::ArchivedSessionIntent;
use vtcode_core::utils::session_archive::{
    SessionListing, find_session_by_identifier, generate_session_archive_identifier, history_persistence_enabled,
    list_recent_sessions, reserve_session_archive_identifier, session_workspace_path,
};
use vtcode_desktop_protocol as protocol;
use vtcode_desktop_protocol::{
    InteractionChoice, InteractionKind, InteractionRegistry, InteractionRequest, InteractionStep, Request, RequestId,
    SessionSummary, ThreadEventNotification,
};
use vtcode_ui::tui::app::{
    HostCommandReceiver, HostInteraction, InlineCommand, InlineEvent, SubmittedInput, host_interaction_from_command,
    host_interaction_response, new_host_session,
};

use crate::agent::agents::{ResumeSession, load_resume_session};
use crate::agent::runloop::unified::turn::run_single_agent_loop_unified_with_host;
use crate::startup::StartupContext;

const MAX_SESSIONS: usize = 200;
const MAX_RESTORED_MESSAGES: usize = 160;
const MAX_RESTORED_MESSAGE_BYTES: usize = 4 * 1024;
const MAX_RESTORED_HISTORY_BYTES: usize = 700 * 1024;
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
        method::CONFIG_DISCOVER => {
            let agent = &context.startup.agent_config;
            // Tool-capable startup already ran the shared provider-auth preflight.
            // Reaching this method means required credentials are available, or
            // the selected provider does not require a credential (for example,
            // local Ollama).
            let authenticated = true;
            let setup = if agent.provider.trim().is_empty() {
                "vtcode auth".to_string()
            } else {
                format!("vtcode login {}", agent.provider)
            };
            let snapshot = protocol::ConfigurationSnapshot {
                provider: agent.provider.clone(),
                model: agent.model.clone(),
                authenticated,
                credential_setup_command: setup,
            };
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
            start_session(context, params.session_id.clone(), Some(resume)).await?;
            context.state.lock().await.session_id = Some(params.session_id.clone());
            let result = protocol::SessionLoadResult {
                session: session_summary(&listing),
                messages: restored_history(&listing),
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
            if params.prompt.trim().is_empty() {
                bail!("prompt must not be empty");
            }
            let turn_id = uuid::Uuid::new_v4().to_string();
            let input = {
                let mut state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str()) {
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
                .send(InlineEvent::Submit(SubmittedInput::new(params.prompt, Vec::new())))
                .is_err()
            {
                let mut state = context.state.lock().await;
                if state.active_turn_id.as_deref() == Some(turn_id.as_str()) {
                    state.active_turn_id = None;
                    state.cancel_requested_turn = None;
                }
                bail!("selected session runtime has stopped; reload the session to continue");
            }
            send_notification(
                &context.notifications,
                method::TURN_STATUS,
                json!({ "sessionId": params.session_id, "turnId": turn_id, "status": "running" }),
            )
            .await?;
            Ok((serde_json::to_value(protocol::TurnAccepted { session_id: params.session_id, turn_id })?, false))
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

async fn start_session(context: &ServiceContext, session_id: String, resume: Option<ResumeSession>) -> Result<()> {
    crate::main_helpers::set_runtime_archive_session_id(Some(session_id.clone()));
    let vtcode_ui::tui::app::HostSession { session, input, commands } = new_host_session();
    let (command_sender, command_receiver) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let command_forwarder = std::thread::Builder::new()
        .name("vtcode-desktop-host-commands".to_string())
        .spawn(move || forward_host_commands(commands, command_sender))
        .context("failed to start desktop host command bridge")?;
    let (event_sender, event_receiver) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let config = context.startup.agent_config.clone();
    let vt_config = context.startup.config.clone();
    let skip_confirmations = context.startup.skip_confirmations;
    let primary_agent_explicitly_configured = context.startup.primary_agent_explicitly_configured;
    let planning_entry_source = context.startup.planning_entry_source;
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
    mut events: mpsc::Receiver<vtcode_core::exec::events::VersionedThreadEvent>,
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
                event = events.recv(), if events_open => match event {
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

async fn publish_thread_event(
    session_id: &str,
    event: vtcode_core::exec::events::VersionedThreadEvent,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    turn_finished: &Notify,
) -> Result<()> {
    let event_value = serde_json::to_value(&event)?;
    let event_type = event_value
        .get("event")
        .and_then(|event| event.get("type"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let terminal_status = match event_type {
        "turn.completed" => Some("completed"),
        "turn.failed" => Some("failed"),
        _ => None,
    };
    let turn_id = {
        let mut state = state.lock().await;
        let turn_id = state
            .session_id
            .as_deref()
            .filter(|selected| *selected == session_id)
            .and(state.active_turn_id.clone());
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
        turn_id
    };

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
    let (input, event) = {
        let mut state = context.state.lock().await;
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
        let event = host_interaction_response(&pending.view, &choice_ids, &answer_texts, response.cancelled)
            .ok_or_else(|| anyhow!("interaction response is incomplete"))?;
        state.interaction_registry.resolve(&response)?;
        state.pending_interactions.remove(&response.interaction_id);
        let runtime = state
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow!("selected session is not running"))?;
        (runtime.input.clone(), event)
    };
    input
        .send(event)
        .map_err(|_error| anyhow!("selected session runtime has stopped"))
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
}
