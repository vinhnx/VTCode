use super::*;

pub(super) fn spawn_session_pump(
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
        let mut progress = vtcode_commons::ui_protocol::ProgressTracker::default();
        let mut background_active = false;
        let mut activity_active = false;
        let mut activity_blocked = false;
        while commands_open || events_open {
            tokio::select! {
                command = commands.recv(), if commands_open => match command {
                    Some(command) => {
                        let changed = match &command {
                            InlineCommand::UpdateProgress(update) => progress.apply(*update),
                            InlineCommand::SetLocalAgents { entries } => {
                                let active = entries.iter().any(|entry| entry.is_loading());
                                let changed = background_active != active;
                                background_active = active;
                                changed
                            }
                            InlineCommand::SetActivityState(activity) => {
                                let active = activity.is_busy();
                                let blocked = matches!(activity, vtcode_commons::ui_protocol::ActivityState::Blocked);
                                let changed = activity_active != active || activity_blocked != blocked;
                                activity_active = active;
                                activity_blocked = blocked;
                                changed
                            }
                            _ => false,
                        };
                        if changed && state.lock().await.session_id.as_deref() == Some(session_id.as_str()) {
                            let phase = progress.phase();
                            if let Err(error) = send_notification(&notifications, protocol::method::UI_PROGRESS, json!({
                                "sessionId": session_id,
                                "active": vtcode_commons::ui_protocol::LoadingActivity {
                                    blocked: activity_blocked,
                                    turn: activity_active,
                                    progress: phase.is_some_and(|phase| phase.is_animated()),
                                    background: background_active,
                                    // Thinking is derived client-side from the awaiting-content
                                    // window; the pump owns turn/activity/progress/background only.
                                    thinking: false,
                                }.is_active(),
                            })).await {
                                tracing::warn!(target: "vtcode.desktop", %error, "failed to publish progress");
                            }
                        }
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
        if completed_turn.is_some() {
            turn_finished.notify_waiters();
        }
        if (activity_active || background_active || progress.phase().is_some())
            && state.lock().await.session_id.as_deref() == Some(session_id.as_str())
            && let Err(error) = send_notification(
                &notifications,
                protocol::method::UI_PROGRESS,
                json!({"sessionId": session_id, "active": false}),
            )
            .await
        {
            tracing::warn!(target: "vtcode.desktop", %error, "failed to clear progress on runtime shutdown");
        }
        if let Some(turn_id) = completed_turn {
            let _ = send_notification(
                &notifications,
                protocol::method::TURN_STATUS,
                json!({ "sessionId": session_id, "turnId": turn_id, "status": "failed" }),
            )
            .await;
        }
    });
}

pub(super) async fn publish_primary_agent_changed(
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

pub(super) fn sanitize_primary_agent_display(display_name: Option<&str>) -> Option<String> {
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

pub(super) async fn publish_thread_event(
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

    // Cleanup depends on the state transition, not on a potentially full or
    // disconnected UI notification queue.
    if terminal_status.is_some() && turn_id.is_some() {
        turn_finished.notify_waiters();
    }

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
    }
    Ok(())
}

pub(super) async fn publish_interaction(
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
    let turn_id = state
        .active_turn_id
        .clone()
        .unwrap_or_else(|| protocol::SESSION_START_TURN_ID.to_string());
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

pub(super) async fn respond_to_interaction(
    context: &ServiceContext,
    response: protocol::InteractionResponse,
) -> Result<()> {
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
