use super::*;

pub(super) fn spawn_session_pump(
    session_id: String,
    mut commands: mpsc::Receiver<InlineCommand>,
    events: protocol::LiveEventReceiver,
    state: std::sync::Arc<Mutex<ServerState>>,
    notifications: mpsc::Sender<protocol::Notification>,
    turn_finished: std::sync::Arc<Notify>,
    generation: Option<uuid::Uuid>,
) -> tokio::task::JoinHandle<()> {
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
                        if changed && runtime_is_current(&*state.lock().await, &session_id, generation) {
                            let phase = progress.phase();
                            if let Err(error) = send_runtime_notification(&notifications, protocol::method::UI_PROGRESS, json!({
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
                            }), generation).await {
                                tracing::warn!(target: "vtcode.desktop", %error, "failed to publish progress");
                            }
                        }
                        if let InlineCommand::SetPrimaryAgent { name, .. } = &command
                            && let Err(error) = publish_primary_agent_changed_for_runtime(
                                &session_id,
                                name.as_deref(),
                                &state,
                                &notifications,
                                generation,
                            )
                            .await
                        {
                            tracing::warn!(target: "vtcode.desktop", error = %error, "failed to publish active primary agent");
                        }
                        if let Some(view) = host_interaction_from_command(&command) {
                            if let Err(error) = publish_interaction_for_runtime(
                                &session_id,
                                view,
                                &state,
                                &notifications,
                                generation,
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
                            if let Err(error) = publish_thread_event_for_runtime(
                                &session_id,
                                event,
                                &state,
                                &notifications,
                                &turn_finished,
                                generation,
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
            if runtime_is_current(&state, &session_id, generation) {
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
            && runtime_is_current(&*state.lock().await, &session_id, generation)
            && let Err(error) = send_runtime_notification(
                &notifications,
                protocol::method::UI_PROGRESS,
                json!({"sessionId": session_id, "active": false}),
                generation,
            )
            .await
        {
            tracing::warn!(target: "vtcode.desktop", %error, "failed to clear progress on runtime shutdown");
        }
        if let Some(turn_id) = completed_turn {
            let _ = send_runtime_notification(
                &notifications,
                protocol::method::TURN_STATUS,
                json!({ "sessionId": session_id, "turnId": turn_id, "status": "failed" }),
                generation,
            )
            .await;
        }
    })
}

fn runtime_is_current(state: &ServerState, session_id: &str, generation: Option<uuid::Uuid>) -> bool {
    state.session_id.as_deref() == Some(session_id) && state.runtime_generation == generation
}

pub(super) async fn send_runtime_notification(
    sender: &mpsc::Sender<protocol::Notification>,
    method: &str,
    mut params: Value,
    generation: Option<uuid::Uuid>,
) -> Result<()> {
    if let Some(generation) = generation {
        let object = params
            .as_object_mut()
            .ok_or_else(|| anyhow!("runtime notification parameters must be an object"))?;
        object.insert(INTERNAL_RUNTIME_GENERATION.to_owned(), json!(generation.to_string()));
    }
    send_notification(sender, method, params).await
}

async fn publish_primary_agent_changed_for_runtime(
    session_id: &str,
    display_name: Option<&str>,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    generation: Option<uuid::Uuid>,
) -> Result<()> {
    if !runtime_is_current(&*state.lock().await, session_id, generation) {
        return Ok(());
    }

    let notification = protocol::PrimaryAgentChanged {
        session_id: session_id.to_string(),
        display_name: sanitize_primary_agent_display(display_name),
    };
    send_runtime_notification(
        notifications,
        protocol::method::PRIMARY_AGENT_CHANGED,
        serde_json::to_value(notification)?,
        generation,
    )
    .await
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

async fn publish_thread_event_for_runtime(
    session_id: &str,
    event: vtcode_core::exec::events::VersionedThreadEvent,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    turn_finished: &Notify,
    generation: Option<uuid::Uuid>,
) -> Result<()> {
    use vtcode_core::exec::events::ThreadEvent;

    let terminal_status = match event.event() {
        ThreadEvent::TurnCompleted(_) => Some("completed"),
        ThreadEvent::TurnFailed(_) => Some("failed"),
        ThreadEvent::TurnBlocked(_) => Some("blocked"),
        _ => None,
    };
    let (turn_id, webmcp_event_hub, drained) = {
        let mut state = state.lock().await;
        if !runtime_is_current(&state, session_id, generation) {
            return Ok(());
        }
        let turn_id = state.active_turn_id.clone();
        let mut drained = None;
        if terminal_status.is_some() && turn_id.is_some() && state.session_id.as_deref() == Some(session_id) {
            if let Some(turn_id) = turn_id.as_deref() {
                state.interaction_registry.finish_turn(session_id, turn_id);
                state
                    .pending_interactions
                    .retain(|_, interaction| interaction.session_id != session_id || interaction.turn_id != turn_id);
            }
            state.active_turn_id = None;
            state.cancel_requested_turn = None;
            // Atomically with the release: pop the next queued batch (if any)
            // so a concurrent submit cannot slip an immediate turn ahead of it.
            drained = state.pop_next_queued_turn();
        }
        (turn_id, state.webmcp_event_hub.clone(), drained)
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
    send_runtime_notification(
        notifications,
        protocol::method::THREAD_EVENT,
        serde_json::to_value(notification)?,
        generation,
    )
    .await?;
    if let (Some(status), Some(turn_id)) = (terminal_status, turn_id) {
        send_runtime_notification(
            notifications,
            protocol::method::TURN_STATUS,
            json!({ "sessionId": session_id, "turnId": turn_id, "status": status }),
            generation,
        )
        .await?;
    }
    if let Some(drained) = drained {
        dispatch_drained_turn(
            state,
            notifications,
            turn_finished,
            session_id,
            drained.turn_id,
            drained.batch,
            drained.input,
        )
        .await?;
    }
    Ok(())
}

async fn publish_interaction_for_runtime(
    session_id: &str,
    view: HostInteraction,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    generation: Option<uuid::Uuid>,
) -> Result<()> {
    let interaction_id = uuid::Uuid::new_v4().to_string();
    let mut state = state.lock().await;
    if !runtime_is_current(&state, session_id, generation) {
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
                .map(|choice| {
                    let (label, recommended) = split_recommended_marker(&choice.label);
                    InteractionChoice {
                        id: choice.id.clone(),
                        label,
                        description: choice.description.clone(),
                        recommended,
                    }
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
    send_runtime_notification(
        notifications,
        protocol::method::INTERACTION_REQUESTED,
        serde_json::to_value(request)?,
        generation,
    )
    .await
}

#[cfg(test)]
pub(super) async fn publish_thread_event(
    session_id: &str,
    event: vtcode_core::exec::events::VersionedThreadEvent,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
    turn_finished: &Notify,
) -> Result<()> {
    publish_thread_event_for_runtime(session_id, event, state, notifications, turn_finished, None).await
}

#[cfg(test)]
pub(super) async fn publish_primary_agent_changed(
    session_id: &str,
    display_name: Option<&str>,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
) -> Result<()> {
    publish_primary_agent_changed_for_runtime(session_id, display_name, state, notifications, None).await
}

#[cfg(test)]
pub(super) async fn publish_interaction(
    session_id: &str,
    view: HostInteraction,
    state: &Mutex<ServerState>,
    notifications: &mpsc::Sender<protocol::Notification>,
) -> Result<()> {
    publish_interaction_for_runtime(session_id, view, state, notifications, None).await
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
    dispatch_interaction_response(&mut state, &response, &choice_ids, &answer_texts, &input)?;
    drop(state);
    context.turn_finished.notify_waiters();
    Ok(())
}

/// Split a service choice label into display text plus the recommended flag.
///
/// TUI rows embed the marker in the title (`Opt (Recommended)`); the desktop
/// client renders its own localized marker from the flag, so the embedded
/// copy is stripped to avoid showing it twice. Matching is
/// case-insensitive, but only an exact trailing marker is stripped — a label
/// that merely mentions the word keeps its full text.
fn split_recommended_marker(label: &str) -> (String, bool) {
    const MARKER: &str = " (recommended)";
    let lower = label.to_ascii_lowercase();
    let recommended = lower.contains("recommended");
    if lower.ends_with(MARKER) {
        // `to_ascii_lowercase` preserves byte length and the cut point
        // precedes ASCII bytes, so it is always a char boundary.
        let mut display = label.to_string();
        display.truncate(label.len() - MARKER.len());
        (display, true)
    } else {
        (label.to_string(), recommended)
    }
}

#[cfg(test)]
mod tests {
    use super::split_recommended_marker;

    #[test]
    fn recommended_marker_splits_suffix_and_flag() {
        assert_eq!(
            split_recommended_marker("Dependency-first slices (Recommended)"),
            ("Dependency-first slices".to_string(), true)
        );
        assert_eq!(split_recommended_marker("1. Guided setup (recommended)"), ("1. Guided setup".to_string(), true));
        assert_eq!(split_recommended_marker("Scope (RECOMMENDED)"), ("Scope".to_string(), true));
    }

    #[test]
    fn recommended_marker_keeps_unmarked_and_mid_string_labels() {
        assert_eq!(split_recommended_marker("User-flow slices"), ("User-flow slices".to_string(), false));
        assert_eq!(
            split_recommended_marker("Recommended actions overview"),
            ("Recommended actions overview".to_string(), true)
        );
        assert_eq!(split_recommended_marker(""), ("".to_string(), false));
    }

    #[test]
    fn recommended_marker_strip_is_char_boundary_safe() {
        assert_eq!(split_recommended_marker("Tùy chọn phạm vi (Recommended)"), ("Tùy chọn phạm vi".to_string(), true));
    }
}
