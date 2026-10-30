use super::*;

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
    result
}

pub(super) fn dispatch_interaction_response(
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

pub(super) async fn shutdown_current_session(context: &ServiceContext, cancel_active: bool) -> Result<()> {
    let (session_id, input, active_turn, should_send_cancel, mut runtime) = {
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
            state.restored_output_spools.clear();
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
            state.interaction_registry.close_session(&session_id);
            state.pending_interactions.retain(|_, pending| pending.session_id != session_id);
        }
        crate::main_helpers::set_runtime_archive_session_id(None);
    }
    context.turn_finished.notify_waiters();
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

pub(super) fn session_summary(listing: &SessionListing) -> SessionSummary {
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
