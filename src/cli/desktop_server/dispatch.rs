use super::*;
use crate::agent::runloop::unified::stop_requests;

pub(super) async fn handle_request(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
    let transition = matches!(
        request.method.as_str(),
        protocol::method::SHUTDOWN
            | protocol::method::CONFIG_SETTINGS_WRITE
            | protocol::method::CONFIG_SELECT
            | protocol::method::CONFIG_OPTIONS
            | protocol::method::CONFIG_THEME
            | protocol::method::CONFIG_SECRET_STORE
            | protocol::method::SESSION_CREATE
            | protocol::method::SESSION_LOAD
            | protocol::method::SESSION_CLOSE
    );
    if !transition {
        return handle_request_inner(context, request).await;
    }
    {
        let mut state = context.state.lock().await;
        if state.session_transitioning {
            bail!("a session change is already in progress");
        }
        if request.method != protocol::method::SHUTDOWN && state.active_turn_id.is_some() {
            bail!("cancel the active turn and wait for cleanup before switching sessions");
        }
        if request.method != protocol::method::SHUTDOWN && !state.pending_interactions.is_empty() {
            bail!("resolve the pending CLI interaction before switching sessions or settings");
        }
        state.session_transitioning = true;
    }
    let result = handle_request_inner(context, request).await;
    context.state.lock().await.session_transitioning = false;
    context.turn_finished.notify_waiters();
    result
}

/// Bound the cooperative stop wait on the session shutdown path. A turn stop
/// itself waits for readiness independently of this timeout.
const STOP_HANDLE_WAIT: Duration = Duration::from_secs(2);
const STOP_HANDLE_POLL: Duration = Duration::from_millis(10);

/// Delay before re-asserting a cancel signal: long enough to land after the
/// runloop's Submit-time latch reset (in-memory work), short enough that a
/// raced single press still stops the turn promptly.
const CANCEL_REFIRE_DELAY: Duration = Duration::from_millis(100);

/// Fire the runloop's out-of-band stop signal, waiting briefly when the
/// runtime is still publishing its handles after shell setup.
async fn signal_runloop_stop(stop: &StopHandleSlot) {
    signal_runloop_stop_with_timeout(stop, STOP_HANDLE_WAIT).await;
}

pub(super) async fn signal_runloop_stop_with_timeout(stop: &StopHandleSlot, wait: Duration) {
    if stop_requests::fire_stop_signal(stop) {
        return;
    }
    let _ = timeout(wait, async {
        loop {
            tokio::time::sleep(STOP_HANDLE_POLL).await;
            if stop_requests::fire_stop_signal(stop) {
                return;
            }
        }
    })
    .await;
}

pub(super) async fn shutdown_current_session(context: &ServiceContext, cancel_active: bool) -> Result<()> {
    let (session_id, input, stop, active_turn, should_send_cancel, mut runtime) = {
        let mut state = context.state.lock().await;
        let active_turn = state.active_turn_id.clone();
        if active_turn.is_some() && !cancel_active {
            bail!("cancel the active turn and wait for cleanup before switching sessions");
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
        let stop = runtime.as_ref().map(|session| session.stop.clone());
        (session_id, input, stop, active_turn, should_send_cancel, runtime)
    };

    if let Some(input) = input {
        if active_turn.is_some() {
            if should_send_cancel {
                if !input.is_closed()
                    && let Some(stop) = stop.as_ref()
                {
                    signal_runloop_stop(stop).await;
                }
                let _ = input.send(InlineEvent::Cancel);
            }
            let _ = timeout(SESSION_SHUTDOWN_TIMEOUT, wait_for_active_turn_to_finish(context)).await;
        }
        // Dual delivery mirrors the TUI exit callback: the queued event ends
        // an idle loop, while the signal also reaches a runloop paused in
        // initialization (which never polls the queue) so shutdown stays
        // graceful instead of waiting for the abort timeout.
        if let Some(stop) = stop.as_ref() {
            stop_requests::fire_exit_signal(stop);
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
        join_host_command_bridge(runtime.command_forwarder, RUNLOOP_EXIT_TIMEOUT).await;
    }

    if let Some(session_id) = session_id {
        let mut state = context.state.lock().await;
        if state.session_id.as_deref() == Some(session_id.as_str()) {
            state.session_id = None;
            state.restored_output_spools.clear();
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
            // Ephemeral queue state dies with the session; a queued prompt is
            // only meaningful while its runtime can dispatch it.
            state.queue.clear();
            state.queue_dropped_total = 0;
            state.interaction_registry.close_session(&session_id);
            state.pending_interactions.retain(|_, pending| pending.session_id != session_id);
        }
        crate::main_helpers::set_runtime_archive_session_id(None);
    }
    context.turn_finished.notify_waiters();
    Ok(())
}

/// Restart the selected session's runtime under the same session id after an
/// idle configuration change, preserving the thread instead of discarding it.
///
/// Loads resume data before shutdown (the archive is current while idle, and a
/// load failure here leaves the live session untouched), shuts the runtime
/// down, applies `apply` to the shared state, then starts a fresh runtime
/// under the same id so the new configuration takes effect. Queued prompts and
/// restored-output spools are carried across the restart. With no session
/// selected, applies `apply` and starts nothing.
///
/// Requires an idle session: no active turn and no pending interactions. The
/// `config/*` transition guard enforces this before dispatch.
async fn restart_session_preserving_thread(
    context: &ServiceContext,
    apply: impl FnOnce(&mut ServerState),
) -> Result<()> {
    let session_id = context.state.lock().await.session_id.clone();
    let Some(session_id) = session_id else {
        let mut state = context.state.lock().await;
        apply(&mut state);
        return Ok(());
    };
    let resume = load_resume_session(&session_id, ArchivedSessionIntent::ResumeInPlace).await?;
    let (queue, queue_dropped_total, restored_output_spools) = {
        let mut state = context.state.lock().await;
        (
            std::mem::take(&mut state.queue),
            std::mem::take(&mut state.queue_dropped_total),
            std::mem::take(&mut state.restored_output_spools),
        )
    };
    shutdown_current_session(context, false).await?;
    {
        let mut state = context.state.lock().await;
        apply(&mut state);
        state.queue = queue;
        state.queue_dropped_total = queue_dropped_total;
        state.restored_output_spools = restored_output_spools;
    }
    start_session(context, session_id, resume).await?;
    Ok(())
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

/// Join the host command bridge thread within `bound`. A wedged bridge must
/// degrade to a warning — never hang shutdown or escalation — so both paths
/// share this bound. Returns whether the thread is confirmed done.
pub(super) async fn join_host_command_bridge(command_forwarder: std::thread::JoinHandle<()>, bound: Duration) -> bool {
    match timeout(bound, tokio::task::spawn_blocking(move || command_forwarder.join())).await {
        Ok(Ok(Ok(()))) => true,
        Ok(Ok(Err(_))) => {
            tracing::warn!(target: "vtcode.desktop", "desktop host command bridge panicked");
            true
        }
        Ok(Err(error)) => {
            tracing::warn!(target: "vtcode.desktop", %error, "failed to join desktop host command bridge");
            false
        }
        Err(_) => {
            tracing::warn!(target: "vtcode.desktop", "timed out joining desktop host command bridge");
            false
        }
    }
}

pub(super) async fn ensure_no_active_turn(context: &ServiceContext) -> Result<()> {
    if context.state.lock().await.active_turn_id.is_some() {
        bail!("cancel the active turn and wait for cleanup before switching sessions");
    }
    Ok(())
}

pub(super) async fn send_notification(
    sender: &mpsc::Sender<protocol::Notification>,
    method: &str,
    params: Value,
) -> Result<()> {
    sender
        .send(protocol::Notification {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        })
        .await
        .map_err(|_error| anyhow!("desktop client disconnected"))
}

pub(super) fn resolve_review_scope(workspace: &Path, scope: Option<&str>) -> Result<PathBuf> {
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

pub(super) fn parse_params<T: DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(value.clone()).context("invalid request parameters")
}

pub(super) fn protocol_interaction_kind(kind: vtcode_ui::tui::app::HostInteractionKind) -> InteractionKind {
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
        size_bytes: listing.size_bytes(),
        updated_at: Some(listing.updated_at()).filter(|updated| *updated > 0),
    }
}

pub(super) async fn handle_request_inner(context: &ServiceContext, request: &Request) -> Result<(Value, bool)> {
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
            stop_webmcp_bridge(context).await;
            // The write targets disk; keep the thread across the restart.
            restart_session_preserving_thread(context, |_| {}).await?;
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
        method::UPDATE_CHECK => {
            // Unlike `update/status`, this one does network I/O: it refreshes
            // the cache so a release published since the last check is
            // visible. A failure propagates instead of re-serving stale data.
            update_install::check_for_updates().await?;
            let status = update_status_snapshot()?;
            Ok((serde_json::to_value(status)?, false))
        }
        method::UPDATE_INSTALL => {
            let state = update_install::start_install(context).await?;
            Ok((serde_json::to_value(state)?, false))
        }
        method::UPDATE_ACKNOWLEDGE => {
            // Best-effort: the banner already rendered, so a cache write
            // failure only risks one extra reminder next launch.
            crate::updater::record_current_version_seen();
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
            // CLI parity: the inherited effort maps onto the new route
            // (nearest supported level, preferring lower) instead of
            // resetting, and the selection persists to the workspace config
            // so it survives service restarts. Persistence runs before the
            // restart so a rejection leaves the live session untouched.
            let supported = session_options::supported_efforts(&params.provider, &params.model);
            let requested = context
                .state
                .lock()
                .await
                .session_options
                .reasoning_effort
                .as_deref()
                .and_then(vtcode_config::types::ReasoningEffortLevel::parse)
                .unwrap_or(context.startup.agent_config.reasoning_effort);
            let supported_refs: Vec<&str> = supported.iter().map(String::as_str).collect();
            let mapped = crate::agent::runloop::unified::best_effort_reasoning(requested, &supported_refs);
            session_preferences::persist_model_selection(
                context.workspace.clone(),
                params.provider.clone(),
                params.model.clone(),
                mapped,
            )
            .await?;
            restart_session_preserving_thread(context, |state| {
                state.model_override = Some(params);
                state.model_override_auth = Some(override_auth);
                state.session_options.reasoning_effort = mapped.map(|level| level.as_str().to_owned());
            })
            .await?;
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
            if params.fast_mode.is_some() || params.mode.is_some() || params.reasoning_effort.is_some() {
                // Best-effort disk persistence: model-specific effort names
                // outside the CLI enum (e.g. "max") stay live-session-only.
                session_preferences::persist(
                    context.workspace.clone(),
                    agent.clone(),
                    params.fast_mode,
                    params.mode.clone(),
                    params
                        .reasoning_effort
                        .as_deref()
                        .and_then(vtcode_config::types::ReasoningEffortLevel::parse),
                )
                .await?;
            }
            restart_session_preserving_thread(context, |state| {
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
            })
            .await?;
            let agent = active_agent_config(context).await;
            Ok((serde_json::to_value(configuration_snapshot(context, &agent).await?)?, false))
        }
        method::CONFIG_THEME => {
            let params: protocol::ThemePreferenceSelectParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            let theme = theme_preferences::persist(context.workspace.clone(), params.theme).await?;
            // A theme change carries no runtime state: record the override and
            // leave the live session untouched instead of restarting it.
            context.state.lock().await.theme_override = Some(theme.clone());
            let agent = active_agent_config(context).await;
            let mut snapshot = configuration_snapshot(context, &agent).await?;
            if let Some(shared) = snapshot.shared_ui.as_mut() {
                shared.theme = theme;
            }
            Ok((serde_json::to_value(snapshot)?, false))
        }
        method::SESSION_LIST => {
            let params: protocol::SessionListParams = parse_params(&request.params)?;
            let sessions =
                list_recent_session_summaries_in_workspace(params.effective_limit(), &context.workspace).await?;
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
            let load_started = Instant::now();
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            ensure_no_active_turn(context).await?;
            ensure_session_not_held_elsewhere(&context.workspace, &params.session_id).await?;
            let archive_started = Instant::now();
            let listing = find_session_by_identifier(&params.session_id)
                .await?
                .ok_or_else(|| anyhow!("saved session was not found"))?;
            if !listing_matches_workspace(&listing, &context.workspace) {
                bail!("saved session belongs to a different workspace");
            }
            let resume = load_resume_session(&params.session_id, ArchivedSessionIntent::ResumeInPlace)
                .await?
                .ok_or_else(|| anyhow!("saved session could not be loaded"))?;
            let archive_elapsed = archive_started.elapsed();
            let previous_session_id = context.state.lock().await.session_id.clone();
            let shutdown_started = Instant::now();
            shutdown_current_session(context, false).await?;
            let shutdown_elapsed = shutdown_started.elapsed();
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
            let inspector_started = Instant::now();
            let inspector_history =
                tokio::task::spawn_blocking(move || restored_inspector_history(&workspace, &history_session_id))
                    .await
                    .context("inspector history task failed")??;
            let inspector_elapsed = inspector_started.elapsed();
            let resume_started = Instant::now();
            start_session(context, params.session_id.clone(), Some(resume)).await?;
            let resume_elapsed = resume_started.elapsed();
            let RestoredInspectorHistory {
                items: review_items,
                incomplete: review_history_incomplete,
                activity_items,
                activity_incomplete: activity_history_incomplete,
                activity_output_cursors,
                output_spools,
                mut timeline_events,
                mut timeline_incomplete,
            } = inspector_history;
            {
                let mut state = context.state.lock().await;
                state.session_id = Some(params.session_id.clone());
                state.restored_output_spools = output_spools;
            }
            let history = restored_history_page(&listing.snapshot.messages, None, MAX_RESTORED_MESSAGES);
            let mut result = protocol::SessionLoadResult {
                session: session_summary(&listing),
                messages: history.messages,
                history_start_index: history.start_index,
                history_has_earlier: history.has_earlier,
                review_items,
                review_history_incomplete,
                activity_items,
                activity_output_cursors,
                activity_history_incomplete,
                timeline_events: Vec::new(),
                timeline_incomplete,
            };
            // Session history and inspector projections share the same frame.
            // Keep a reserve for the response envelope and trim only the oldest
            // timeline events when the remaining wire budget is smaller.
            let base_bytes = serde_json::to_vec(&result)?.len();
            let budget = protocol::MAX_FRAME_BYTES.saturating_sub(base_bytes.saturating_add(8192));
            let mut used = 0_usize;
            let mut retained = 0_usize;
            for event in timeline_events.iter().rev() {
                let size = serde_json::to_vec(event)?.len().saturating_add(1);
                if used.saturating_add(size) > budget {
                    timeline_incomplete = true;
                    break;
                }
                used += size;
                retained += 1;
            }
            if retained < timeline_events.len() {
                drop(timeline_events.drain(..timeline_events.len() - retained));
            }
            result.timeline_events = timeline_events;
            result.timeline_incomplete = timeline_incomplete;
            let payload = serde_json::to_value(result)?;
            tracing::debug!(
                target: "vtcode.desktop",
                archive_ms = archive_elapsed.as_millis(),
                shutdown_ms = shutdown_elapsed.as_millis(),
                inspector_ms = inspector_elapsed.as_millis(),
                resume_ms = resume_elapsed.as_millis(),
                total_ms = load_started.elapsed().as_millis(),
                "desktop session loaded"
            );
            Ok((payload, false))
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
                EnqueueMode::Queueable { batchable: params.batchable },
            )
            .await?;
            Ok((serde_json::to_value(accepted)?, false))
        }
        method::TURN_UNDO => turn_undo::handle_undo_turn(context, request).await,
        method::TURN_CANCEL => {
            let params: protocol::TurnCancelParams = parse_params(&request.params)?;
            let queue_status = {
                let mut state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str())
                    || state.active_turn_id.as_deref() != Some(params.turn_id.as_str())
                {
                    bail!("turn is no longer active");
                }
                let runtime = state
                    .runtime
                    .as_ref()
                    .ok_or_else(|| anyhow!("selected session is not running"))?;
                let input = runtime.input.clone();
                let stop = runtime.stop.clone();
                let should_send = state.cancel_requested_turn.as_deref() != Some(params.turn_id.as_str());
                if should_send {
                    state.cancel_requested_turn = Some(params.turn_id.clone());
                    // The out-of-band signal is what stops the running turn; the
                    // queued event only dismisses overlay waits. A closed channel
                    // proves the runtime is gone, so skip the signal and go
                    // straight to the send-failure bail below.
                    if !input.is_closed() {
                        if stop_requests::fire_stop_signal(&stop) {
                            // Reassert while the same turn is active as a
                            // backstop for older interruption paths.
                            spawn_cancel_refire(
                                context.state.clone(),
                                stop.clone(),
                                params.session_id.clone(),
                                params.turn_id.clone(),
                                CANCEL_REFIRE_DELAY,
                            );
                        } else {
                            // Startup is not ready for turn cancellation yet.
                            spawn_late_stop_signal(
                                context.state.clone(),
                                stop.clone(),
                                input.clone(),
                                params.session_id.clone(),
                                params.turn_id.clone(),
                            );
                        }
                    }
                    if input.send(InlineEvent::Cancel).is_err() {
                        state.cancel_requested_turn = None;
                        bail!("selected session runtime has stopped");
                    }
                    // Bounded escalation: if this exact turn is still active
                    // after the grace period, the watchdog force-stops it and
                    // rebuilds the session from durable history. Repeat
                    // presses reuse the first press's watchdog.
                    spawn_cancel_watchdog(
                        context,
                        params.session_id.clone(),
                        params.turn_id.clone(),
                        escalation_waits_for(&state),
                    );
                } else if !input.is_closed() {
                    // A repeated press re-asserts the out-of-band signal without
                    // re-queueing: the first signal may have been consumed
                    // already, and the turn is still active. Non-blocking: the
                    // first press owned the publish wait, so repeats stay cheap.
                    stop_requests::fire_stop_signal(&stop);
                }
                if params.clear_queue && !state.queue.is_empty() {
                    state.queue.clear();
                    Some(state.queue_status(&params.session_id))
                } else {
                    None
                }
            };
            if let Some(status) = queue_status {
                publish_queue_status_sync(&context.notifications, &status);
            }
            Ok((json!({ "cancelRequested": true }), false))
        }
        method::EXEC_FORCE_CANCEL | method::EXEC_BACKGROUND => {
            let params: protocol::SessionIdParams = parse_params(&request.params)?;
            let input = {
                let state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str()) {
                    bail!("session is not selected");
                }
                let runtime = state
                    .runtime
                    .as_ref()
                    .ok_or_else(|| anyhow!("selected session is not running"))?;
                runtime.input.clone()
            };
            // No active-turn gate: a stuck foreground session can outlive its
            // turn, and the escape hatch must work whenever sessions exist.
            let event = if request.method == method::EXEC_FORCE_CANCEL {
                InlineEvent::ForceCancelPtySession
            } else {
                InlineEvent::BackgroundOperation
            };
            if input.send(event).is_err() {
                bail!("selected session runtime has stopped");
            }
            Ok((json!({ "accepted": true }), false))
        }
        method::TURN_QUEUE_TAKE_LATEST => {
            let params: protocol::TurnQueueTakeLatestParams = parse_params(&request.params)?;
            let (result, status) = {
                let mut state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str()) {
                    bail!("session is not selected");
                }
                // Text-only restore mirrors the CLI queue edit: attachments
                // stay with their dispatched turn and the composer re-attaches
                // on resubmit.
                let text = state.queue.pop_back().map(|queued| queued.input.text);
                let result = protocol::TurnQueueTakeLatestResult {
                    session_id: params.session_id,
                    queue_depth: state.queue.len(),
                    text,
                };
                let status = result.text.is_some().then(|| state.queue_status(&result.session_id));
                (result, status)
            };
            if let Some(status) = status {
                publish_queue_status_sync(&context.notifications, &status);
            }
            Ok((serde_json::to_value(result)?, false))
        }
        method::TURN_QUEUE_REMOVE => {
            let params: protocol::TurnQueueRemoveParams = parse_params(&request.params)?;
            let (result, status) = {
                let mut state = context.state.lock().await;
                if state.session_id.as_deref() != Some(params.session_id.as_str()) {
                    bail!("session is not selected");
                }
                // Accepting removes never dispatch, so an empty queue stays a
                // clean `removed: false` instead of an error: a stale id (a
                // concurrent drain won the race) is a normal outcome the
                // client reconciles from the published snapshot.
                let position = state.queue.iter().position(|queued| queued.entry_id() == params.entry_id);
                let removed = position.is_some();
                if let Some(position) = position {
                    state.queue.remove(position);
                }
                let status = removed.then(|| state.queue_status(&params.session_id));
                let result = protocol::TurnQueueRemoveResult {
                    session_id: params.session_id,
                    removed,
                    queue_depth: state.queue.len(),
                };
                (result, status)
            };
            if let Some(status) = status {
                publish_queue_status_sync(&context.notifications, &status);
            }
            Ok((serde_json::to_value(result)?, false))
        }
        method::INTERACTION_RESPOND => {
            let response: protocol::InteractionResponse = parse_params(&request.params)?;
            respond_to_interaction(context, response).await?;
            Ok((json!({ "accepted": true }), false))
        }
        method::STORAGE_SUMMARY | method::SESSION_DELETE | method::WORKTREE_LIST | method::WORKTREE_REMOVE => {
            storage::handle_storage_request(context, request).await
        }
        method::SESSION_RECENT_PROMPTS => recent_prompts::handle_recent_prompts_request(context, request).await,
        _ => bail!("unknown desktop method"),
    }
}
