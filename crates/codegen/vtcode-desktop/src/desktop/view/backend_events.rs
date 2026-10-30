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
                        // ItemUpdated carries a full snapshot, so intermediate
                        // snapshots for this item need not be rendered in one
                        // UI tick. The service persists every event separately.
                        pending_item_update = Some(event);
                        continue;
                    }
                    if let Some(pending) = pending_item_update.take() {
                        self.process_drained_event(pending, window, cx);
                    }
                    if is_item_update_event(&event) {
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
                    self.backend = None;
                    self.clear_pending_interaction();
                    self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
                    self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                    self.fail_workspace_review("Local service disconnected.".into());
                    self.running = false;
                    self.status = "Local service disconnected".to_string();
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
        self.maybe_notify_update_available(&event, window, cx);
        self.handle_backend_event(event, cx);
        let interaction_replaced = new_interaction
            && previous_interaction_id.as_deref()
                != self.interaction.as_ref().map(|interaction| interaction.interaction_id.as_str());
        if (self.interaction.is_none() || interaction_replaced) && !self.question_answer.read(cx).value().is_empty() {
            self.question_answer.update(cx, |state, cx| state.set_value("", window, cx));
        }
        if accepted_decision
            && self.interaction.is_none()
            && !self.settings_open
            && self.palette.is_none()
            && !self.review_open
        {
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
                match notification.event.event() {
                    vtcode_exec_events::ThreadEvent::TurnCompleted(_) => Some(BackgroundAgentActivity::Completed),
                    vtcode_exec_events::ThreadEvent::TurnFailed(_)
                    | vtcode_exec_events::ThreadEvent::TurnBlocked(_) => Some(BackgroundAgentActivity::NeedsAttention),
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
                Some(super::super::notifications::activity_for_interaction_kind(interaction.kind))
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
                Some(BackgroundAgentActivity::ServiceDisconnected)
            }
            BackendEvent::LaunchFailed(_) if self.backend.is_some() && self.pending_workspace.is_none() => {
                Some(BackgroundAgentActivity::ServiceDisconnected)
            }
            _ => None,
        };
        let Some(activity) = activity else {
            return;
        };
        if !super::super::notifications::activity_allowed(
            activity,
            &self.preferences.notifications,
            window.is_window_active(),
        ) {
            return;
        }
        super::super::notifications::background_agent_activity(window, activity, cx);
    }

    pub(super) fn handle_backend_event(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        if let BackendEvent::Response { method, .. } = &event
            && matches!(
                method.as_str(),
                protocol::method::SESSION_CREATE
                    | protocol::method::SESSION_LOAD
                    | protocol::method::SESSION_CLOSE
                    | protocol::method::CONFIG_SELECT
                    | protocol::method::CONFIG_OPTIONS
                    | protocol::method::CONFIG_THEME
                    | protocol::method::CONFIG_SETTINGS_WRITE
                    | protocol::method::CONFIG_SECRET_STORE
                    | protocol::method::TURN_SUBMIT
            )
        {
            self.invalidate_webmcp_status();
        }
        match event {
            BackendEvent::TurnBlockedByStartupInteraction { session_id } => {
                if self.selected_session_id.as_deref() != Some(session_id.as_str()) || self.pending_submission.is_none()
                {
                    return;
                }
                self.invalidate_webmcp_status();
                let retry = self.pending_first_submission && !self.cancel_requested;
                self.pending_first_submission = false;
                self.running = false;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                if retry {
                    if let Some(draft) = self.pending_submission.take() {
                        self.transcript.remove(&draft.echo_id);
                        self.first_submission = Some(draft);
                        self.status = "Waiting for the startup decision before sending…".into();
                    }
                } else {
                    self.recover_pending_submission(false);
                    self.error = Some("Resolve the startup question before sending a message.".into());
                    self.status = "Message not submitted".into();
                }
            }
            BackendEvent::InteractionResponded { identity, error } => {
                if self.pending_interaction_response.as_deref() != Some(identity.interaction_id.as_str())
                    || !self.interaction.as_ref().is_some_and(|interaction| {
                        interaction.interaction_id == identity.interaction_id
                            && interaction.session_id == identity.session_id
                            && interaction.turn_id == identity.turn_id
                    })
                {
                    return;
                }
                self.pending_interaction_response = None;
                if let Some(error) = error {
                    self.error = Some(error);
                    self.status = "The decision was not accepted. Review it and try again.".into();
                } else {
                    self.clear_pending_interaction();
                    self.invalidate_webmcp_status();
                    self.clear_error();
                    self.status = if identity.cancelled {
                        "Interaction cancelled"
                    } else {
                        "Decision accepted"
                    }
                    .into();
                    if identity.turn_id == protocol::SESSION_START_TURN_ID
                        && self.selected_session_id.as_deref() == Some(identity.session_id.as_str())
                        && self.session_request.is_none()
                        && let Some(draft) = self.first_submission.take()
                    {
                        match self.enqueue_submission(draft, cx) {
                            Ok(()) => {}
                            Err(draft) => {
                                self.retained_submission = Some(draft);
                                self.status = "Message not submitted".into();
                            }
                        }
                    }
                }
            }
            BackendEvent::Response { method, result, error } => {
                if self.session_request == Some(method.as_str()) && method != protocol::method::SESSION_LOAD {
                    self.session_request = None;
                }
                if let Some(error) = error {
                    if method == protocol::method::SESSION_HISTORY {
                        self.fail_history_page(error, cx);
                        return;
                    }
                    if self.session_request == Some(method.as_str()) {
                        self.session_request = None;
                    }
                    if matches!(
                        method.as_str(),
                        protocol::method::INITIALIZE
                            | protocol::method::SESSION_LIST
                            | protocol::method::SESSION_LOAD
                            | protocol::method::SESSION_CLOSE
                    ) {
                        self.fail_pending_navigation();
                    }
                    if method == protocol::method::SESSION_CLOSE {
                        self.pending_new_thread = false;
                    }
                    if method == protocol::method::CONFIG_OPTIONS {
                        self.pending_permission_choice = None;
                    }
                    if self.handle_webmcp_error(&method, &error) {
                        cx.notify();
                        return;
                    }
                    if method == protocol::method::COMPOSER_RESOURCES {
                        self.resources_loading = false;
                    }
                    if method == protocol::method::CONFIG_SETTINGS_WRITE {
                        self.config_write_pending = false;
                        self.config_editor_error = Some(error.clone());
                    }
                    if method == protocol::method::CONFIG_SECRET_STORE {
                        // Form-scoped: the inline error carries it; the
                        // generic banner would duplicate it away from the field.
                        self.handle_secret_store_error(error.clone(), cx);
                        return;
                    }
                    if method == protocol::method::UPDATE_STATUS {
                        self.update_request_pending = false;
                    }
                    if method == protocol::method::WORKSPACE_REVIEW {
                        self.fail_workspace_review(error);
                        return;
                    }
                    if method == protocol::method::INITIALIZE {
                        self.backend = None;
                    }
                    if method == protocol::method::SESSION_CREATE
                        && let Some(draft) = self.first_submission.take()
                    {
                        self.retained_submission = Some(draft);
                    }
                    self.error = Some(error);
                    if method == protocol::method::TURN_SUBMIT {
                        self.recover_pending_submission(false);
                        self.running = false;
                        self.cancel_requested = false;
                        self.cancel_request_sent = false;
                    } else if method == protocol::method::TURN_CANCEL {
                        self.cancel_request_sent = false;
                    }
                    self.status = "Service request failed".to_string();
                    return;
                }
                let result = result.unwrap_or(Value::Null);
                match method.as_str() {
                    protocol::method::WORKSPACE_REVIEW => {
                        match serde_json::from_value::<protocol::WorkspaceReviewResult>(result) {
                            Ok(snapshot) => self.accept_workspace_review(snapshot, cx),
                            Err(_) => self.fail_workspace_review("Invalid Git review response.".into()),
                        }
                    }
                    protocol::method::INITIALIZE => {
                        let initialized = serde_json::from_value::<protocol::InitializeResult>(result);
                        match initialized {
                            Ok(info) if info.protocol == protocol::PROTOCOL_VERSION => {
                                if valid_slash_command_catalog(&info.slash_commands) {
                                    self.slash_commands = info.slash_commands.clone();
                                } else {
                                    self.slash_commands.clear();
                                    self.error =
                                        Some("The local CLI returned an invalid slash command catalog.".into());
                                }
                                self.service_info = Some(info);
                                self.status = "Connected to the local VT Code service".to_string();
                                let mut discovery_queued = true;
                                for method in [
                                    protocol::method::CONFIG_DISCOVER,
                                    protocol::method::CONFIG_SETTINGS_READ,
                                    protocol::method::CONFIG_MODELS,
                                    protocol::method::SESSION_LIST,
                                ] {
                                    if !self.request(method, json!({})) {
                                        self.error =
                                            Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
                                        discovery_queued = false;
                                        break;
                                    }
                                }
                                if discovery_queued {
                                    self.request_webmcp_status();
                                    self.request_update_status();
                                }
                            }
                            _ => {
                                self.service_info = None;
                                self.backend = None;
                                self.fail_pending_navigation();
                                self.error = Some("The local service returned an invalid or incompatible initialization response. Rebuild the desktop app and its bundled helper together.".to_string());
                                self.status = "Could not initialize the local VT Code service".to_string();
                            }
                        }
                    }
                    protocol::method::CONFIG_DISCOVER => {
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.sync_cli_theme(cx);
                        self.restore_workspace_permission(cx);
                        if self
                            .configuration
                            .as_ref()
                            .is_some_and(|configuration| !configuration.authenticated)
                        {
                            self.error = Some(provider_auth_guidance(self.configuration.as_ref()));
                        }
                    }
                    protocol::method::CONFIG_SETTINGS_READ => {
                        self.handle_cli_config_read_response(result, cx);
                    }
                    protocol::method::CONFIG_SETTINGS_WRITE => {
                        self.handle_cli_config_write_response(result, cx);
                    }
                    protocol::method::CONFIG_OPEN_FILE => {
                        self.handle_config_open_file_response(cx);
                    }
                    protocol::method::CONFIG_SECRET_STORE => {
                        self.handle_secret_store_response(result, cx);
                    }
                    protocol::method::WEBMCP_STATUS | protocol::method::WEBMCP_PAIR | protocol::method::WEBMCP_STOP => {
                        let _handled = self.handle_webmcp_response(&method, result, cx);
                    }
                    protocol::method::UPDATE_STATUS => {
                        self.handle_update_status_response(result);
                    }
                    protocol::method::CONFIG_MODELS => {
                        match serde_json::from_value::<Vec<protocol::ModelInfo>>(result) {
                            Ok(models) => self.models = models,
                            Err(_) => self.error = Some("The local service returned an invalid model catalog.".into()),
                        }
                    }
                    protocol::method::COMPOSER_RESOURCES => {
                        self.resources_loading = false;
                        match serde_json::from_value::<Vec<protocol::ComposerResource>>(result) {
                            Ok(resources)
                                if resources.len() <= 192
                                    && resources.iter().all(|resource| {
                                        protocol::ComposerResource::is_valid_name(&resource.name)
                                            && resource.description.chars().count() <= 400
                                    }) =>
                            {
                                self.composer_resources = resources;
                                self.resources_loaded = true;
                            }
                            _ => self.error = Some("Invalid CLI resource catalog.".into()),
                        }
                    }
                    protocol::method::CONFIG_OPTIONS => match serde_json::from_value::<ConfigurationSnapshot>(result) {
                        Ok(snapshot) => {
                            if let Some(choice) = self.pending_permission_choice.take()
                                && snapshot
                                    .session_options
                                    .as_ref()
                                    .is_some_and(|options| options.permission_mode == choice)
                            {
                                self.save_workspace_permission(choice);
                            }
                            let attachments = std::mem::take(&mut self.attachments);
                            self.reset_session_state();
                            self.attachments = attachments;
                            self.selected_session_id = None;
                            self.record_current_navigation();
                            self.configuration = Some(snapshot);
                            self.status = "Runtime options updated. The next message starts a new session.".into();
                        }
                        Err(_) => {
                            self.pending_permission_choice = None;
                            self.backend = None;
                            self.error = Some("Invalid runtime options response. Reconnect before sending.".into());
                        }
                    },
                    protocol::method::CONFIG_THEME => match serde_json::from_value::<ConfigurationSnapshot>(result) {
                        Ok(snapshot) => {
                            let theme = snapshot
                                .shared_ui
                                .as_ref()
                                .map(|preferences| preferences.theme.clone())
                                .filter(|theme| themes::find(theme).is_some());
                            if let Some(theme) = theme {
                                let attachments = std::mem::take(&mut self.attachments);
                                self.reset_session_state();
                                self.attachments = attachments;
                                self.selected_session_id = None;
                                self.record_current_navigation();
                                self.configuration = Some(snapshot);
                                self.preferences.follow_cli_theme = Some(true);
                                self.apply_theme_state(theme, None, cx);
                                self.status = "CLI theme saved. The next message starts a new session.".into();
                            } else {
                                self.backend = None;
                                self.error = Some("The CLI returned an unsupported theme after saving.".into());
                                self.status = "Could not apply CLI theme".into();
                            }
                        }
                        Err(_) => {
                            self.backend = None;
                            self.error = Some("Invalid CLI theme response. Reconnect before sending.".into());
                        }
                    },
                    protocol::method::CONFIG_SELECT => {
                        // The service restarted with the new provider/model and
                        // no selected session; the next prompt starts a fresh
                        // session under it.
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.clear_session_after_service_reset();
                        self.models.clear();
                        let _ = self.request(protocol::method::CONFIG_MODELS, json!({}));
                        let model = self
                            .configuration
                            .as_ref()
                            .map(|configuration| configuration.model.clone())
                            .unwrap_or_default();
                        self.status = format!("Switched to {model}. Start a new session to use it.");
                    }
                    protocol::method::SESSION_LIST => {
                        match serde_json::from_value::<Vec<SessionSummary>>(result) {
                            Ok(mut refreshed) => {
                                let mut session_ids = HashSet::with_capacity(refreshed.len());
                                let has_invalid_session_id = refreshed.iter().any(|session| {
                                    session.session_id.trim().is_empty()
                                        || !session_ids.insert(session.session_id.as_str())
                                });
                                if has_invalid_session_id {
                                    self.fail_pending_navigation();
                                    self.error = Some("The local service returned an invalid session list.".into());
                                } else {
                                    // Keep a locally derived title when the archive has
                                    // not persisted its own preview yet.
                                    for session in &mut refreshed {
                                        if session.title.as_deref().is_none_or(str::is_empty) {
                                            session.title = self
                                                .sessions
                                                .iter()
                                                .find(|existing| existing.session_id == session.session_id)
                                                .and_then(|existing| existing.title.clone());
                                        }
                                    }
                                    self.sessions = refreshed;
                                    self.continue_pending_workspace_navigation(cx);
                                }
                            }
                            Err(_) => {
                                self.fail_pending_navigation();
                                self.error = Some("The local service returned an invalid session list.".into());
                            }
                        }
                    }
                    protocol::method::SESSION_CREATE => {
                        let first_submission = self.first_submission.take();
                        if let Some(session) = serde_json::from_value::<SessionSummary>(result)
                            .ok()
                            .filter(|session| !session.session_id.is_empty())
                        {
                            self.selected_session_id = Some(session.session_id.clone());
                            self.sessions.insert(0, session);
                            self.reset_session_state();
                            self.record_current_navigation();
                            self.status = "Session ready. Submit a prompt to begin.".to_string();
                            if let Some(draft) = first_submission {
                                match self.enqueue_submission(draft, cx) {
                                    Ok(()) => self.pending_first_submission = true,
                                    Err(draft) => {
                                        self.retained_submission = Some(draft);
                                        self.status = "Message not submitted".to_string();
                                    }
                                }
                            }
                        } else {
                            self.status = "Session creation failed".to_string();
                            self.retained_submission = first_submission;
                            self.error = Some(
                                "The service returned an invalid session. Your message has not been submitted."
                                    .to_string(),
                            );
                        }
                    }
                    protocol::method::SESSION_CLOSE => {
                        self.refresh_session_options_after_full_access();
                        let pending_navigation = self.pending_navigation.take();
                        let new_thread = self.pending_new_thread;
                        let attachments =
                            (new_thread || pending_navigation.is_some()).then(|| std::mem::take(&mut self.attachments));
                        self.selected_session_id = None;
                        self.reset_session_state();
                        if let Some(attachments) = attachments {
                            self.attachments = attachments;
                        }
                        if let Some(pending) = pending_navigation {
                            let _moved = self.navigation_history.navigate_to(pending.target_index);
                        } else {
                            self.record_current_navigation();
                        }
                        self.status = if new_thread || pending_navigation.is_some() {
                            "New thread ready. Submit a prompt to begin."
                        } else {
                            "Session closed. Start a new session to continue."
                        }
                        .to_string();
                    }
                    protocol::method::SESSION_LOAD => {
                        self.refresh_session_options_after_full_access();
                        self.begin_session_load(result, cx);
                    }
                    protocol::method::SESSION_HISTORY => self.begin_session_history_page(result, cx),
                    protocol::method::TURN_SUBMIT => {
                        let accepted =
                            serde_json::from_value::<protocol::TurnAccepted>(result)
                                .ok()
                                .filter(|accepted| {
                                    self.selected_session_id.as_deref() == Some(accepted.session_id.as_str())
                                        && !accepted.turn_id.is_empty()
                                });
                        let Some(accepted) = accepted else {
                            if self.pending_submission.is_some() {
                                self.recover_pending_submission(true);
                                self.running = false;
                                self.status = "Invalid turn acknowledgement".to_string();
                            }
                            self.error = Some("The service returned an invalid turn acknowledgement. Delivery could not be confirmed.".to_string());
                            return;
                        };
                        // Lifecycle events can complete the turn before the request response arrives.
                        if self.pending_submission.is_none() && self.active_turn_id.is_none() {
                            return;
                        }
                        self.pending_submission = None;
                        self.pending_first_submission = false;
                        self.active_turn_id = Some(accepted.turn_id);
                        self.running = true;
                        self.status = "Running".to_string();
                        self.try_send_pending_cancel();
                    }
                    protocol::method::TURN_CANCEL => {
                        self.status = "Cancellation accepted; waiting for cleanup…".to_string();
                    }
                    _ => {}
                }
            }
            BackendEvent::WorkspaceReviewResponse { request_id, result, error } => {
                if self
                    .workspace_review
                    .as_ref()
                    .is_some_and(|review| review.pending && review.request_id == request_id)
                {
                    if let Some(error) = error {
                        self.fail_workspace_review(error);
                    } else if let Ok(snapshot) =
                        serde_json::from_value::<protocol::WorkspaceReviewResult>(result.unwrap_or(Value::Null))
                    {
                        if self.workspace_review.as_ref().is_some_and(|review| {
                            review.request_id == snapshot.request_id && review.workspace == snapshot.workspace
                        }) {
                            self.accept_workspace_review(snapshot, cx);
                        } else {
                            self.fail_workspace_review("Invalid Git review response.".into());
                        }
                    } else {
                        self.fail_workspace_review("Invalid Git review response.".into());
                    }
                    cx.notify();
                }
            }
            BackendEvent::SessionHistoryResponse { request_id, result, error } => {
                if self.history_page_request_id.as_deref() != Some(request_id.as_str()) {
                    return;
                }
                if let Some(error) = error {
                    self.fail_history_page(error, cx);
                } else if let Some(result) = result {
                    self.begin_session_history_page(result, cx);
                } else {
                    self.fail_history_page("The local service returned an empty history response.".into(), cx);
                }
            }
            BackendEvent::SessionOutputResponse { request_id, result, error } => {
                if self.output_page_pending.as_ref().map(|pending| pending.request_id.as_str())
                    != Some(request_id.as_str())
                {
                    return;
                }
                if error.is_some() {
                    self.fail_output_page(cx);
                } else if let Some(result) = result {
                    self.begin_session_output_page(result, cx);
                } else {
                    self.fail_output_page(cx);
                }
            }
            BackendEvent::McpAppResponse { request_id, result, error } => {
                if !self.mcp_app_pending.contains_key(request_id.as_str()) {
                    return;
                }
                if error.is_some() {
                    self.fail_mcp_app_preview(request_id.as_str(), cx);
                } else if let Some(result) = result {
                    self.begin_mcp_app_preview(result, cx);
                } else {
                    self.fail_mcp_app_preview(request_id.as_str(), cx);
                }
            }
            BackendEvent::ThreadEvent(notification) => {
                if !self.notification_matches(&notification.session_id, notification.turn_id.as_deref()) {
                    return;
                }
                let was_running = self.running;
                self.handle_thread_event(notification);
                if was_running != self.running {
                    self.invalidate_webmcp_status();
                }
                self.refresh_selected_change(cx);
                self.handle_cancel_state_after_turn_event();
            }
            BackendEvent::PrimaryAgentChanged(notification) => {
                if self.selected_session_id.as_deref() != Some(notification.session_id.as_str()) {
                    return;
                }
                self.active_primary_agent_display = notification
                    .display_name
                    .filter(|name| !name.trim().is_empty())
                    .map(|display_name| ActivePrimaryAgentDisplay {
                        session_id: notification.session_id,
                        display_name,
                    });
            }
            BackendEvent::Interaction(interaction) => {
                if self.selected_session_id.as_deref() != Some(interaction.session_id.as_str())
                    || (interaction.turn_id != protocol::SESSION_START_TURN_ID
                        && !self.notification_matches(&interaction.session_id, Some(&interaction.turn_id)))
                    || self
                        .interaction
                        .as_ref()
                        .is_some_and(|current| current.interaction_id == interaction.interaction_id)
                {
                    return;
                }
                self.interaction_choice_ids =
                    interaction.steps.iter().map(|step| step.selected_choice_id.clone()).collect();
                if self.interaction_choice_ids.is_empty() {
                    self.interaction_choice_ids = interaction.selected_choice_ids.iter().cloned().map(Some).collect();
                }
                self.interaction_answer_texts = vec![None; interaction.steps.len()];
                self.interaction_step_index = 0;
                self.pending_interaction_response = None;
                self.interaction = Some(interaction);
                self.invalidate_webmcp_status();
                self.status = "Waiting for your decision".to_string();
            }
            BackendEvent::TurnStatus(status) => {
                if !status.get(keys::SESSION_ID).and_then(Value::as_str).is_some_and(|session_id| {
                    self.notification_matches(session_id, status.get(keys::TURN_ID).and_then(Value::as_str))
                }) {
                    return;
                }
                if let Some(label) = status.get(keys::STATUS).and_then(Value::as_str) {
                    self.status = label.to_string();
                    self.invalidate_webmcp_status();
                }
            }
            BackendEvent::Exited { status, authentication_required } => {
                self.cancel_session_load_task();
                self.interrupt_cli_config_write();
                self.interrupt_secret_save();
                self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                self.fail_workspace_review("The local service exited before Git review completed.".into());
                self.clear_pending_interaction();
                self.session_request = None;
                self.pending_new_thread = false;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.backend = None;
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                if self.pending_workspace.is_none() {
                    self.fail_pending_navigation();
                }
                if let Some(workspace) = self.pending_workspace.take() {
                    self.start_workspace(workspace, cx);
                } else if authentication_required {
                    self.status = "Provider credentials required".to_string();
                    let authentication_error = format!(
                        "VT Code could not authenticate the configured provider. In Terminal, run `vtcode auth` or `vtcode login <provider>`. {}",
                        service_reconnect_guidance(self.retained_submission.is_some())
                    );
                    self.error = Some(match self.error.take() {
                        Some(error) => format!("{error}\n\n{authentication_error}"),
                        None => authentication_error,
                    });
                } else {
                    self.status = match status {
                        Some(code) => format!("Local service exited with code {code}"),
                        None => "Local service exited".to_string(),
                    };
                    let reconnect_error = format!(
                        "The local VT Code service disconnected. {}",
                        service_reconnect_guidance(self.retained_submission.is_some())
                    );
                    self.error = Some(match self.error.take() {
                        Some(error) => format!("{error}\n\n{reconnect_error}"),
                        None => reconnect_error,
                    });
                }
            }
            BackendEvent::LaunchFailed(error) => {
                self.cancel_session_load_task();
                self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                self.fail_workspace_review("The local service could not start.".into());
                self.clear_pending_interaction();
                self.session_request = None;
                self.pending_new_thread = false;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.report_service_launch_failure(error);
                self.interrupt_cli_config_write();
                self.backend = None;
                self.running = false;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
            }
        }
    }

    fn notification_matches(&self, session_id: &str, turn_id: Option<&str>) -> bool {
        if self.selected_session_id.as_deref() != Some(session_id) {
            return false;
        }
        if let Some(turn_id) = turn_id {
            if self.active_turn_id.as_deref().is_some_and(|active| active != turn_id) {
                return false;
            }
            if self.running && self.finished_turn_id.as_deref() == Some(turn_id) {
                return false;
            }
        }
        true
    }

    pub(super) fn handle_thread_event(&mut self, notification: ThreadEventNotification) {
        use vtcode_exec_events::ThreadEvent;
        let terminal_turn_id = matches!(
            notification.event.event(),
            ThreadEvent::TurnCompleted(_) | ThreadEvent::TurnFailed(_) | ThreadEvent::TurnBlocked(_)
        )
        .then(|| notification.turn_id.clone())
        .flatten();
        match notification.event.event() {
            ThreadEvent::TurnStarted(_) => {
                self.pending_submission = None;
                self.pending_first_submission = false;
                self.running = true;
                self.active_turn_id = notification.turn_id;
                self.set_selected_session_completed(false);
                self.status = "Running".to_string();
            }
            ThreadEvent::TurnCompleted(event) => {
                self.pending_submission = None;
                self.pending_first_submission = false;
                self.running = false;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.status = "Completed".to_string();
                self.last_usage = Some(event.usage.clone());
                self.set_selected_session_completed(true);
            }
            ThreadEvent::TurnFailed(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::TurnBlocked(event) => {
                self.finish_unverified_turn(&event.message);
            }
            ThreadEvent::PlanDelta(delta) => {
                self.transcript.append_text(
                    delta.item_id.clone(),
                    TranscriptRole::Assistant.as_str(),
                    "Plan".into(),
                    &delta.delta,
                );
            }
            ThreadEvent::ItemStarted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemUpdated(event) => self.update_runtime_item(&event.item),
            ThreadEvent::ItemCompleted(event) => self.update_runtime_item(&event.item),
            ThreadEvent::Error(event) => {
                self.transcript.push(TranscriptItem::notice(
                    format!("runtime-error-{}", uuid_like_id()),
                    "Runtime error".into(),
                    event.message.clone(),
                    NoticeSeverity::Error,
                ));
            }
            ThreadEvent::ThreadCompactBoundary(event) => {
                // Pre-boundary usage no longer describes the live context; the
                // next completed turn reports fresh post-boundary usage.
                self.last_usage = None;
                self.transcript.push(TranscriptItem::notice(
                    format!("compact-boundary-{}", uuid_like_id()),
                    rust_i18n::t!("app.transcript.compact_boundary_title").to_string().into(),
                    rust_i18n::t!(
                        "app.transcript.compact_boundary_body",
                        original = event.original_message_count,
                        compacted = event.compacted_message_count,
                        trigger = event.trigger.as_str(),
                        mode = event.mode.as_str()
                    )
                    .to_string(),
                    NoticeSeverity::Info,
                ));
            }
            ThreadEvent::ContextReset(event) => {
                self.last_usage = None;
                let trigger = match event.trigger {
                    vtcode_exec_events::ContextResetTrigger::PlanApproval => "plan approval",
                    vtcode_exec_events::ContextResetTrigger::Unknown => "unknown",
                };
                self.transcript.push(TranscriptItem::notice(
                    format!("context-reset-{}", uuid_like_id()),
                    rust_i18n::t!("app.transcript.context_reset_title").to_string().into(),
                    rust_i18n::t!(
                        "app.transcript.context_reset_body",
                        trigger = trigger,
                        previous = event.previous_context_usage_percent
                    )
                    .to_string(),
                    NoticeSeverity::Info,
                ));
            }
            ThreadEvent::AppSandboxDenied(event) => {
                self.transcript.push(TranscriptItem::notice(
                    format!("app-sandbox-denied-{}", uuid_like_id()),
                    "App UI refused".into(),
                    format!(
                        "{} offered app UI refused by the '{}' tier: {}",
                        event.tool,
                        event.tier,
                        event.uris.join(", "),
                    ),
                    NoticeSeverity::Warning,
                ));
            }
            _ => {}
        }
        if let Some(turn_id) = terminal_turn_id {
            self.finished_turn_id = Some(turn_id);
        }
    }
}

fn item_update_identity(event: &BackendEvent) -> Option<(&str, Option<&str>, &str)> {
    let BackendEvent::ThreadEvent(notification) = event else {
        return None;
    };
    let vtcode_exec_events::ThreadEvent::ItemUpdated(update) = notification.event.event() else {
        return None;
    };
    Some((&notification.session_id, notification.turn_id.as_deref(), &update.item.id))
}

fn is_item_update_event(event: &BackendEvent) -> bool {
    item_update_identity(event).is_some()
}

pub(super) fn can_coalesce_item_updates(previous: &BackendEvent, next: &BackendEvent) -> bool {
    matches!(
        (item_update_identity(previous), item_update_identity(next)),
        (Some((previous_session, previous_turn, previous_item)), Some((next_session, next_turn, next_item)))
            if previous_session == next_session && previous_turn == next_turn && previous_item == next_item
    )
}
