//! Backend event intake: drain loop, dispatch, thread-event projection.

use super::super::notifications::BackgroundAgentActivity;
use super::*;

impl DesktopView {
    pub(super) fn start_event_drain(&mut self, cx: &mut Context<Self>) {
        if self.event_drain_task.is_some() {
            return;
        }

        let event_tx = self.event_tx.clone();
        self.event_drain_task = Some(cx.spawn(async move |weak_view, cx| {
            loop {
                event_tx.notified().await;
                match weak_view.update_in(cx, |view, window, cx| view.drain_events(window, cx)) {
                    Ok(batch_full) if batch_full => event_tx.wake(),
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        }));
    }

    pub(super) fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.session_load_task.is_some() {
            return false;
        }
        let mut changed = false;
        let mut processed = 0;
        let mut pending_item_update = None;
        for _ in 0..MAX_UI_EVENTS_PER_TICK {
            match self.event_rx.try_recv() {
                Ok(event) => {
                    processed += 1;
                    changed = true;
                    if pending_item_update
                        .as_ref()
                        .is_some_and(|pending| can_coalesce_item_updates(pending, &event))
                    {
                        // Snapshot updates can skip intermediate renders.
                        // Delta-bearing output updates are excluded by the
                        // coalescing predicate.
                        pending_item_update = Some(event);
                        continue;
                    }
                    if let Some(pending) = pending_item_update.take() {
                        self.process_drained_event(pending, window, cx);
                    }
                    if event_signals::is_item_update_event(&event) {
                        pending_item_update = Some(event);
                    } else {
                        self.process_drained_event(event, window, cx);
                    }
                    if self.session_load_task.is_some() {
                        break;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if let Some(pending) = pending_item_update.take() {
                        self.process_drained_event(pending, window, cx);
                    }
                    self.cancel_session_load_task();
                    self.interrupt_cli_config_write();
                    self.interrupt_armed_running_switch();
                    self.backend = None;
                    self.clear_pending_interaction();
                    self.clear_pending_turn_undos();
                    self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
                    self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                    self.fail_workspace_review(rust_i18n::t!("app.service.review_disconnected").to_string());
                    self.running = false;
                    self.clear_queue_state();
                    self.status = rust_i18n::t!("app.service.disconnected").to_string();
                    changed = true;
                    break;
                }
            }
        }
        if let Some(pending) = pending_item_update {
            self.process_drained_event(pending, window, cx);
        }
        if changed {
            cx.notify();
        }
        self.session_load_task.is_none() && processed == MAX_UI_EVENTS_PER_TICK
    }

    fn process_drained_event(&mut self, event: BackendEvent, window: &mut Window, cx: &mut Context<Self>) {
        let accepted_decision = matches!(&event,
            BackendEvent::InteractionResponded { identity, error: None }
                if self.pending_interaction_response.as_deref() == Some(identity.interaction_id.as_str()))
            && self.interaction_focus.contains_focused(window, cx);
        let new_interaction = matches!(&event, BackendEvent::Interaction(_));
        let previous_interaction_id = new_interaction
            .then(|| self.interaction.as_ref().map(|interaction| interaction.interaction_id.clone()))
            .flatten();
        self.notify_agent_activity(&event, window, cx);
        self.notify_turn_undo(&event, window, cx);
        self.maybe_notify_update_available(&event, window, cx);
        self.handle_backend_event(event, cx);
        if let Some(text) = self.pending_composer_restore.take() {
            self.replace_composer_text(text, window, cx);
            self.reset_history_recall();
            self.status = rust_i18n::t!("app.composer.draft_restored").to_string();
        }
        let interaction_replaced = new_interaction
            && previous_interaction_id.as_deref()
                != self.interaction.as_ref().map(|interaction| interaction.interaction_id.as_str());
        if (self.interaction.is_none() || interaction_replaced) && !self.question_answer.read(cx).value().is_empty() {
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        let overlay_open = self.current_route() != Route::Workspace || self.palette.is_some();
        if accepted_decision && self.interaction.is_none() && !overlay_open {
            self.focus_composer(window, cx);
        }
        self.restore_focus_after_review_close(window, cx);
    }

    fn notify_agent_activity(&self, event: &BackendEvent, window: &mut Window, cx: &mut Context<Self>) {
        let activity = match event {
            BackendEvent::ThreadEvent(notification) => {
                if self.selected_session_id.as_deref() != Some(notification.session_id.as_str())
                    || !self.running
                    || !notification
                        .turn_id
                        .as_deref()
                        .is_some_and(|turn_id| self.active_turn_id.as_deref() == Some(turn_id))
                    || !self.notification_matches(&notification.session_id, notification.turn_id.as_deref())
                {
                    return;
                }
                let target = deeplinks::NotificationTarget::Thread { session_id: notification.session_id.clone() };
                match notification.event.event() {
                    vtcode_exec_events::ThreadEvent::TurnCompleted(_) => {
                        Some((BackgroundAgentActivity::Completed, target))
                    }
                    vtcode_exec_events::ThreadEvent::TurnFailed(_)
                    | vtcode_exec_events::ThreadEvent::TurnBlocked(_) => {
                        Some((BackgroundAgentActivity::NeedsAttention, target))
                    }
                    _ => None,
                }
            }
            BackendEvent::Interaction(interaction) => {
                let current_turn = interaction.turn_id == protocol::SESSION_START_TURN_ID
                    || (self.running
                        && self.active_turn_id.as_deref() == Some(interaction.turn_id.as_str())
                        && self.notification_matches(&interaction.session_id, Some(&interaction.turn_id)));
                if self.selected_session_id.as_deref() != Some(interaction.session_id.as_str())
                    || !current_turn
                    || self
                        .interaction
                        .as_ref()
                        .is_some_and(|current| current.interaction_id == interaction.interaction_id)
                {
                    return;
                }
                Some((
                    super::super::notifications::activity_for_interaction_kind(interaction.kind),
                    deeplinks::NotificationTarget::Interaction {
                        session_id: interaction.session_id.clone(),
                        interaction_id: interaction.interaction_id.clone(),
                    },
                ))
            }
            // The transport reports its terminal state before the channel
            // drops, so these fire exactly once per disconnect. Intentional
            // restarts (workspace switch, settings reload) carry a pending
            // workspace and clean exits need no banner; `backend.is_some()`
            // collapses a LaunchFailed+Exited pair into one banner because the
            // first handler clears the backend before the second arrives.
            BackendEvent::Exited { status, .. }
                if self.backend.is_some() && self.pending_workspace.is_none() && *status != Some(0) =>
            {
                Some((BackgroundAgentActivity::ServiceDisconnected, deeplinks::NotificationTarget::Workspace))
            }
            BackendEvent::LaunchFailed(_) if self.backend.is_some() && self.pending_workspace.is_none() => {
                Some((BackgroundAgentActivity::ServiceDisconnected, deeplinks::NotificationTarget::Workspace))
            }
            _ => None,
        };
        let Some((activity, target)) = activity else {
            return;
        };
        if !super::super::notifications::activity_allowed(
            activity,
            &self.preferences.notifications,
            window.is_window_active(),
        ) {
            return;
        }
        // Focused windows toast in-app where the click target lives;
        // backgrounded windows banner through the OS center, mirroring the
        // update notice. Either way the click resolves the same target.
        let system = !window.is_window_active();
        let weak = cx.entity().downgrade();
        super::super::notifications::background_agent_activity(
            window,
            activity,
            system,
            deeplinks::click_handler(target, weak),
            cx,
        );
    }
}

mod event_signals;
mod handle;
mod thread_events;

pub(super) use event_signals::can_coalesce_item_updates;
