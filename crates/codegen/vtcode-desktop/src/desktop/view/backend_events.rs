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
                self.drop_armed_running_switch();
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                if retry {
                    if let Some(draft) = self.pending_submission.take() {
                        self.transcript.remove(&draft.echo_id);
                        self.first_submission = Some(draft);
                        self.status = rust_i18n::t!("app.service.startup_wait").to_string();
                    }
                } else {
                    self.recover_pending_submission(false);
                    self.error = Some(rust_i18n::t!("app.service.startup_question").to_string());
                    self.status = rust_i18n::t!(i18n::COMPOSER_MESSAGE_NOT_SUBMITTED).to_string();
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
                    self.status = rust_i18n::t!("app.service.decision_rejected").to_string();
                } else {
                    self.clear_pending_interaction();
                    self.invalidate_webmcp_status();
                    self.clear_error();
                    self.status = if identity.cancelled {
                        rust_i18n::t!("app.service.interaction_cancelled").to_string()
                    } else {
                        rust_i18n::t!("app.service.decision_accepted").to_string()
                    };
                    if identity.turn_id == protocol::SESSION_START_TURN_ID
                        && self.selected_session_id.as_deref() == Some(identity.session_id.as_str())
                        && self.session_request.is_none()
                        && let Some(draft) = self.first_submission.take()
                    {
                        match self.enqueue_submission(draft, cx) {
                            Ok(()) => {}
                            Err(draft) => {
                                self.retained_submission = Some(draft);
                                self.status = rust_i18n::t!(i18n::COMPOSER_MESSAGE_NOT_SUBMITTED).to_string();
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
                    if method == protocol::method::STORAGE_SUMMARY || method == protocol::method::SESSION_DELETE {
                        self.fail_storage_summary(error.clone(), cx);
                        return;
                    }
                    if method == protocol::method::WORKTREE_LIST || method == protocol::method::WORKTREE_REMOVE {
                        self.fail_storage_worktrees(error.clone(), cx);
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
                        let had_pending = self.pending_submission.is_some();
                        self.recover_pending_submission(false);
                        if had_pending {
                            self.running = false;
                            self.cancel_requested = false;
                            self.cancel_request_sent = false;
                            self.drop_armed_running_switch();
                        } else if let Some(index) = self.queued_submissions.iter().position(|entry| !entry.acknowledged)
                        {
                            // The failed submit is the oldest unconfirmed
                            // queued entry (FIFO); the running turn it waited
                            // on is untouched.
                            let entry = self.queued_submissions.remove(index);
                            self.transcript.remove(&entry.echo_id);
                        }
                    } else if method == protocol::method::TURN_CANCEL {
                        self.cancel_request_sent = false;
                        // The service is authoritative: if it rejects a cancel
                        // for our live turn, our running state is stale (e.g. a
                        // dropped terminal event) — reconcile instead of
                        // spinning forever. Correlated against the last-sent
                        // turn so a late error cannot clear a newer turn.
                        let stale = matches!(
                            (&self.cancel_sent_turn_id, &self.active_turn_id),
                            (Some(sent), Some(active)) if sent == active
                        );
                        if stale && let Some(message) = self.error.clone() {
                            let was_running = self.running;
                            self.finish_unverified_turn(&message);
                            if was_running {
                                self.invalidate_webmcp_status();
                                self.drain_armed_running_switch(cx);
                            }
                        }
                    } else if method == protocol::method::TURN_QUEUE_TAKE_LATEST {
                        self.take_latest_pending = false;
                    }
                    self.status = rust_i18n::t!("app.service.request_failed").to_string();
                    return;
                }
                let result = result.unwrap_or(Value::Null);
                match method.as_str() {
                    protocol::method::WORKSPACE_REVIEW => {
                        match serde_json::from_value::<protocol::WorkspaceReviewResult>(result) {
                            Ok(snapshot) => self.accept_workspace_review(snapshot, cx),
                            Err(_) => {
                                self.fail_workspace_review(rust_i18n::t!("app.service.review_invalid").to_string())
                            }
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
                                    self.error = Some(rust_i18n::t!("app.service.invalid_slash_catalog").to_string());
                                }
                                self.localize_slash_catalog();
                                self.service_info = Some(info);
                                self.status = rust_i18n::t!("app.service.connected_service").to_string();
                                let mut discovery_queued = true;
                                for method in [
                                    protocol::method::CONFIG_DISCOVER,
                                    protocol::method::CONFIG_SETTINGS_READ,
                                    protocol::method::CONFIG_MODELS,
                                    protocol::method::SESSION_LIST,
                                ] {
                                    if !self.request(method, json!({})) {
                                        self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
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
                                self.error = Some(rust_i18n::t!("app.service.invalid_init").to_string());
                                self.status = rust_i18n::t!("app.service.init_failed").to_string();
                            }
                        }
                    }
                    protocol::method::CONFIG_DISCOVER => {
                        self.configuration = serde_json::from_value::<ConfigurationSnapshot>(result).ok();
                        self.sync_cli_theme(cx);
                        self.restore_workspace_permission(cx);
                        if self.credentials_missing() {
                            self.set_provider_auth_error();
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
                            Err(_) => self.error = Some(rust_i18n::t!("app.service.invalid_catalog").to_string()),
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
                            _ => self.error = Some(rust_i18n::t!("app.service.invalid_resource_catalog").to_string()),
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
                            // The service restarts the runtime under the same
                            // session id; keep the thread, transcript, and tabs.
                            self.configuration = Some(snapshot);
                            self.status = rust_i18n::t!("app.service.runtime_updated").to_string();
                        }
                        Err(_) => {
                            self.pending_permission_choice = None;
                            self.backend = None;
                            self.error = Some(rust_i18n::t!("app.service.invalid_runtime").to_string());
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
                                // A theme change leaves the live session
                                // untouched; keep the thread and transcript.
                                self.configuration = Some(snapshot);
                                self.preferences.follow_cli_theme = Some(true);
                                self.apply_theme_state(theme, None, cx);
                                self.status = rust_i18n::t!("app.service.theme_saved").to_string();
                            } else {
                                self.backend = None;
                                self.error = Some(rust_i18n::t!("app.service.unsupported_theme").to_string());
                                self.status = rust_i18n::t!("app.service.theme_failed").to_string();
                            }
                        }
                        Err(_) => {
                            self.backend = None;
                            self.error = Some(rust_i18n::t!("app.service.invalid_theme").to_string());
                        }
                    },
                    protocol::method::CONFIG_SELECT => match serde_json::from_value::<ConfigurationSnapshot>(result) {
                        Ok(snapshot) => {
                            // The service restarted the runtime with the new
                            // provider/model under the same session id; keep the
                            // thread, transcript, and tabs.
                            let model = snapshot.model.clone();
                            self.configuration = Some(snapshot);
                            self.models.clear();
                            let _ = self.request(protocol::method::CONFIG_MODELS, json!({}));
                            self.status =
                                rust_i18n::t!("app.service.switched_model", model = model.as_str()).to_string();
                        }
                        Err(_) => {
                            self.backend = None;
                            self.error = Some(rust_i18n::t!("app.service.invalid_runtime").to_string());
                        }
                    },
                    protocol::method::STORAGE_SUMMARY => self.apply_storage_summary(result),
                    protocol::method::SESSION_DELETE => self.apply_session_delete(result, cx),
                    protocol::method::WORKTREE_LIST => self.apply_worktree_list(result),
                    protocol::method::WORKTREE_REMOVE => self.apply_worktree_remove(result, cx),
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
                                    self.error = Some(rust_i18n::t!("app.service.invalid_session_list").to_string());
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
                                    self.prune_tabs();
                                    self.continue_pending_workspace_navigation(cx);
                                }
                            }
                            Err(_) => {
                                self.fail_pending_navigation();
                                self.error = Some(rust_i18n::t!("app.service.invalid_session_list").to_string());
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
                            self.ensure_tab(session.session_id.clone());
                            self.sessions.insert(0, session);
                            // The first draft rides out the reset in a local:
                            // restore its thumbnails, then prune once it lands
                            // in the pending or retained slot.
                            let thumbnails = std::mem::take(&mut self.attachment_thumbnails);
                            self.reset_session_state();
                            self.attachment_thumbnails = thumbnails;
                            self.record_current_navigation();
                            self.status = rust_i18n::t!("app.service.session_ready").to_string();
                            if let Some(draft) = first_submission {
                                match self.enqueue_submission(draft, cx) {
                                    Ok(()) => self.pending_first_submission = true,
                                    Err(draft) => {
                                        self.retained_submission = Some(draft);
                                        self.status = rust_i18n::t!(i18n::COMPOSER_MESSAGE_NOT_SUBMITTED).to_string();
                                    }
                                }
                            }
                            self.prune_attachment_thumbnails();
                        } else {
                            self.status = rust_i18n::t!("app.service.creation_failed").to_string();
                            self.retained_submission = first_submission;
                            self.error = Some(rust_i18n::t!("app.service.invalid_session").to_string());
                        }
                    }
                    protocol::method::SESSION_CLOSE => {
                        self.refresh_session_options_after_full_access();
                        let pending_navigation = self.pending_navigation.take();
                        let new_thread = self.pending_new_thread;
                        let closing = self.selected_session_id.clone();
                        let preserved = (new_thread || pending_navigation.is_some()).then(|| {
                            (std::mem::take(&mut self.attachments), std::mem::take(&mut self.attachment_thumbnails))
                        });
                        self.selected_session_id = None;
                        if !new_thread
                            && pending_navigation.is_none()
                            && let Some(closing) = closing.as_deref()
                        {
                            self.remove_tab(closing);
                        }
                        self.reset_session_state();
                        if let Some((attachments, thumbnails)) = preserved {
                            self.attachments = attachments;
                            self.attachment_thumbnails = thumbnails;
                            self.prune_attachment_thumbnails();
                        }
                        if let Some(pending) = pending_navigation {
                            let _moved = self.navigation_history.navigate_to(pending.target_index);
                        } else {
                            self.record_current_navigation();
                        }
                        self.status = if new_thread || pending_navigation.is_some() {
                            rust_i18n::t!(i18n::SESSION_THREAD_READY).to_string()
                        } else {
                            rust_i18n::t!("app.session.closed_new").to_string()
                        };
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
                                        && (accepted.queued || !accepted.turn_id.is_empty())
                                });
                        let Some(accepted) = accepted else {
                            if self.pending_submission.is_some() {
                                self.recover_pending_submission(true);
                                self.running = false;
                                self.drop_armed_running_switch();
                                self.status = rust_i18n::t!("app.service.invalid_ack").to_string();
                            }
                            // Unconfirmed queued entries stay: the ack is
                            // unactionable, and the next queue-status snapshot
                            // reconciles them.
                            self.error = Some(rust_i18n::t!("app.service.invalid_ack_detail").to_string());
                            return;
                        };
                        if accepted.queued {
                            if !self.confirm_oldest_queued_submission() {
                                // No unconfirmed entry: the ack belongs to an
                                // in-flight immediate submit that the service
                                // queued behind a fresh turn (demotion race).
                                if let Some(draft) = self.pending_submission.take() {
                                    self.pending_first_submission = false;
                                    self.queued_submissions.push(QueuedSubmission {
                                        echo_id: draft.echo_id,
                                        prompt: draft.prompt,
                                        acknowledged: true,
                                    });
                                    self.prune_attachment_thumbnails();
                                }
                            }
                            self.try_send_pending_cancel();
                            return;
                        }
                        // Immediate ack: the pending submit, or a queued entry
                        // the service promoted when the session went idle first.
                        // Lifecycle events can complete the turn before the request response arrives.
                        // An already-started turn owns its ack: queued entries
                        // are newer submissions whose own acks are still in
                        // flight, so promotion only runs while idle.
                        if self.pending_submission.is_none()
                            && self.active_turn_id.is_none()
                            && !self.confirm_oldest_queued_submission()
                        {
                            return;
                        }
                        self.pending_submission = None;
                        self.pending_first_submission = false;
                        self.prune_attachment_thumbnails();
                        self.active_turn_id = Some(accepted.turn_id);
                        self.running = true;
                        self.status = rust_i18n::t!("app.service.running").to_string();
                        self.try_send_pending_cancel();
                    }
                    protocol::method::TURN_QUEUE_TAKE_LATEST => {
                        self.take_latest_pending = false;
                        let taken = serde_json::from_value::<protocol::TurnQueueTakeLatestResult>(result)
                            .ok()
                            .filter(|taken| self.selected_session_id.as_deref() == Some(taken.session_id.as_str()));
                        let Some(taken) = taken else {
                            self.error = Some(rust_i18n::t!("app.service.invalid_ack_detail").to_string());
                            return;
                        };
                        self.queue_depth = taken.queue_depth;
                        let Some(text) = taken.text else {
                            return;
                        };
                        // Remove the newest confirmed entry (the request-time
                        // newest; anything newer arrived after the request).
                        // The service text is authoritative.
                        if let Some(index) = self.queued_submissions.iter().rposition(|entry| entry.acknowledged) {
                            let entry = self.queued_submissions.remove(index);
                            self.transcript.remove(&entry.echo_id);
                        }
                        if self.composer.read(cx).value().is_empty() {
                            self.pending_composer_restore = Some(text);
                        } else {
                            // A draft arrived during the round trip: retain the
                            // popped text instead of clobbering it.
                            self.retained_submission = Some(SubmissionDraft {
                                prompt: text,
                                attachments: Vec::new(),
                                echo_id: format!("desktop-user-{}", uuid_like_id()),
                                delivery_uncertain: false,
                            });
                            self.prune_attachment_thumbnails();
                            self.status = rust_i18n::t!("app.composer.draft_restored").to_string();
                        }
                    }
                    protocol::method::TURN_CANCEL => {
                        self.status = rust_i18n::t!("app.service.cancel_accepted").to_string();
                    }
                    protocol::method::EXEC_FORCE_CANCEL | protocol::method::EXEC_BACKGROUND => {
                        self.apply_exec_command_response(method.as_str());
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
                            self.fail_workspace_review(rust_i18n::t!("app.service.review_invalid").to_string());
                        }
                    } else {
                        self.fail_workspace_review(rust_i18n::t!("app.service.review_invalid").to_string());
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
                    self.fail_history_page(rust_i18n::t!("app.service.empty_history").to_string(), cx);
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
                if was_running && !self.running {
                    self.drain_armed_running_switch(cx);
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
                self.interaction_immediate_choice_id = None;
                self.pending_interaction_response = None;
                self.interaction = Some(interaction);
                self.invalidate_webmcp_status();
                self.status = rust_i18n::t!("app.service.awaiting_decision").to_string();
            }
            BackendEvent::UiProgress(progress) => {
                if progress
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .is_some_and(|session_id| self.selected_session_id.as_deref() == Some(session_id))
                {
                    self.ui_progress_active = progress.get("active").and_then(Value::as_bool).unwrap_or(false);
                    cx.notify();
                }
            }
            BackendEvent::TurnQueueStatus(status) => {
                let Ok(snapshot) = serde_json::from_value::<protocol::TurnQueueStatus>(status) else {
                    return;
                };
                self.apply_queue_status(snapshot);
            }
            BackendEvent::TurnStatus(status) => {
                if !status.get(keys::SESSION_ID).and_then(Value::as_str).is_some_and(|session_id| {
                    self.notification_matches(session_id, status.get(keys::TURN_ID).and_then(Value::as_str))
                }) {
                    return;
                }
                if let Some(label) = status.get(keys::STATUS).and_then(Value::as_str) {
                    self.status = if label.eq_ignore_ascii_case("running") {
                        rust_i18n::t!("app.service.running").to_string()
                    } else if label.eq_ignore_ascii_case("failed")
                        && status.get("restarted").and_then(Value::as_bool).unwrap_or(false)
                    {
                        rust_i18n::t!("app.service.turn_recovered").to_string()
                    } else {
                        label.to_string()
                    };
                    self.invalidate_webmcp_status();
                }
            }
            BackendEvent::Exited { status, authentication_required } => {
                self.cancel_session_load_task();
                self.interrupt_cli_config_write();
                self.interrupt_secret_save();
                self.interrupt_armed_running_switch();
                self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                self.fail_workspace_review(rust_i18n::t!("app.service.review_exited").to_string());
                self.clear_pending_interaction();
                self.session_request = None;
                self.pending_new_thread = false;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.backend = None;
                self.running = false;
                self.ui_progress_active = false;
                self.background_activity_count = 0;
                self.active_turn_id = None;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.clear_queue_state();
                if self.pending_workspace.is_none() {
                    self.fail_pending_navigation();
                }
                if let Some(workspace) = self.pending_workspace.take() {
                    self.start_workspace(workspace, cx);
                } else if authentication_required {
                    self.status = rust_i18n::t!("app.service.credentials_required").to_string();
                    let authentication_error = rust_i18n::t!(
                        "app.service.authentication_failed",
                        detail = service_reconnect_guidance(self.retained_submission.is_some())
                    )
                    .to_string();
                    self.error = Some(match self.error.take() {
                        Some(error) => format!("{error}\n\n{authentication_error}"),
                        None => authentication_error,
                    });
                } else {
                    self.status = match status {
                        Some(code) => rust_i18n::t!("app.service.exited_code", code = code).to_string(),
                        None => rust_i18n::t!("app.service.exited").to_string(),
                    };
                    let reconnect_error = rust_i18n::t!(
                        "app.service.disconnected_detail",
                        detail = service_reconnect_guidance(self.retained_submission.is_some())
                    )
                    .to_string();
                    self.error = Some(match self.error.take() {
                        Some(error) => format!("{error}\n\n{reconnect_error}"),
                        None => reconnect_error,
                    });
                }
            }
            BackendEvent::LaunchFailed(error) => {
                self.cancel_session_load_task();
                self.reset_webmcp_state(Some(webmcp::WebmcpNotice::Disconnected));
                self.fail_workspace_review(rust_i18n::t!("app.service.review_no_start").to_string());
                self.clear_pending_interaction();
                self.session_request = None;
                self.pending_new_thread = false;
                if let Some(draft) = self.first_submission.take() {
                    self.retained_submission = Some(draft);
                }
                self.recover_pending_submission(true);
                self.report_service_launch_failure(error);
                self.interrupt_cli_config_write();
                self.interrupt_armed_running_switch();
                self.backend = None;
                self.running = false;
                self.ui_progress_active = false;
                self.background_activity_count = 0;
                self.cancel_requested = false;
                self.cancel_request_sent = false;
                self.clear_queue_state();
            }
        }
    }
}

mod event_signals;
mod thread_events;

pub(super) use event_signals::can_coalesce_item_updates;
