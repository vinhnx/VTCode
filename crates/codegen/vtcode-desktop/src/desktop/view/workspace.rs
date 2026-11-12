//! Workspace selection, backend startup, and navigation history.

use super::*;
use gpui_kit::TouchPhase;

impl DesktopView {
    /// Remember a workspace as recently used.
    fn remember_workspace(&mut self, workspace: &Path) {
        self.preferences.touch_workspace(&workspace.display().to_string());
        self.preferences.save();
    }

    /// The saved recent workspaces for the sidebar.
    pub(super) fn recent_workspaces(&self) -> Vec<String> {
        self.preferences.recent_workspaces.clone()
    }

    /// Short display name for the active workspace: the final path component
    /// (lossy), or `None` when no workspace is set. Shared by the welcome,
    /// composer, and utilities call sites; the sidebar, title bar, and status
    /// bar keep their own `to_str` chains, which fall back to placeholders on
    /// non-UTF8 paths instead of showing a lossy name.
    pub(super) fn workspace_file_name(&self) -> Option<String> {
        self.workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }

    pub(super) fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_picker_open {
            return;
        }
        if self.retained_submission.is_some() {
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            cx.notify();
            return;
        }
        // Panels stay out of the way of a workspace switch: the native picker
        // floats above everything, and the new workspace lands on a clean page.
        self.close_active_route(window, cx);

        self.workspace_picker_open = true;
        let paths = cx.prompt_for_paths(workspace_picker_options());
        cx.notify();
        self.workspace_picker_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = paths.await;
            let _updated = view.update_in(cx, |view, window, cx| {
                view.workspace_picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.select_workspace_from_ui(path, window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        view.error =
                            Some(rust_i18n::t!("app.workspace.picker_failed", error = error.to_string()).to_string())
                    }
                    Ok(Ok(None)) => {}
                    Err(_) => view.error = Some(rust_i18n::t!("app.workspace.picker_closed").to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// Whether no workspace/session transition is in flight, regardless of
    /// turn activity. Shared by the switch gate (which also requires an idle
    /// turn) and the submit gate (which queues behind a running turn).
    pub(super) fn workspace_transition_idle(&self) -> bool {
        self.pending_workspace.is_none()
            && !self.workspace_selection_pending
            && self.session_request.is_none()
            && self.first_submission.is_none()
    }

    pub(super) fn workspace_switch_idle(&self) -> bool {
        !self.running && self.workspace_transition_idle()
    }

    fn current_navigation_location(&self) -> Option<NavigationLocation> {
        Some(NavigationLocation {
            workspace: self.workspace.clone()?,
            session_id: self.selected_session_id.clone(),
        })
    }

    pub(super) fn record_current_navigation(&mut self) {
        // A new history entry moves the cursor, so an armed swipe's advertised
        // target is stale: retire the arm before recording. The canceled arm's
        // timer no-ops, and the next render unmounts the pill.
        let _retired = self.swipe_arming.cancel();
        if let Some(location) = self.current_navigation_location() {
            self.navigation_history.record(location);
        }
    }

    /// History navigation deliberately stays on the idle-only gate: routing it
    /// through the running-switch modal would entangle `pending_navigation`
    /// with the armed wait, so mid-turn history stays parked (pre-existing
    /// behavior, kept).
    pub(super) fn can_navigate_back(&self) -> bool {
        self.navigation_history.back_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    pub(super) fn can_navigate_forward(&self) -> bool {
        self.navigation_history.forward_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    /// Direction of the in-flight history navigation, derived from the pending
    /// target relative to the current cursor. The cursor only moves when the
    /// pending navigation resolves, so the comparison is stable while pending.
    pub(super) fn pending_navigation_direction(&self) -> Option<NavigationDirection> {
        let pending = self.pending_navigation?;
        let current = self.navigation_history.current_index()?;
        if pending.target_index < current {
            Some(NavigationDirection::Back)
        } else if pending.target_index > current {
            Some(NavigationDirection::Forward)
        } else {
            None
        }
    }

    /// Whether a trackpad swipe or mouse back/forward button may navigate.
    /// History, pending-transition, and session guards live in
    /// `can_navigate_back`/`can_navigate_forward`; this adds the modal-overlay
    /// guard so a swipe over an open palette, settings, preview, or review
    /// cannot navigate the background underneath it.
    pub(super) fn swipe_navigation_allowed(&self) -> bool {
        self.preferences.swipe_navigation
            && self.current_route() == Route::Workspace
            && self.palette.is_none()
            && !self.config_page_open
            && self.config_editor.is_none()
            && self.file_preview.is_none()
    }

    /// Whether history exists in `direction` and no transition blocks it.
    /// Shared by the scroll path and the toolbar cue so the cue never
    /// advertises a navigation that would no-op.
    pub(super) fn swipe_direction_available(&self, direction: NavigationDirection) -> bool {
        match direction {
            NavigationDirection::Back => self.can_navigate_back(),
            NavigationDirection::Forward => self.can_navigate_forward(),
        }
    }

    /// Feeds a trackpad/mouse scroll event into swipe navigation. Sustained
    /// horizontal scrolling arms like a discrete swipe; everything else —
    /// including all vertical scrolling — is left for the scrollable under the
    /// cursor. Observes only: never stops propagation. The toolbar cue updates
    /// only when the armed direction changes, not on every scroll event.
    pub(super) fn handle_swipe_scroll(&mut self, event: &ScrollEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.swipe_navigation_allowed() {
            let was_armed = self.swipe_scroll.armed().is_some();
            self.swipe_scroll.reset();
            self.cancel_pending_swipe(cx);
            if was_armed {
                cx.notify();
            }
            return;
        }
        let natural_scroll = natural_scroll_enabled();
        let before = self.swipe_scroll.armed();
        let delta = event.delta.pixel_delta(window.line_height());
        let dx_px = f32::from(delta.x);
        let dy_px = f32::from(delta.y);
        // While an arm is pending, input that fights the gesture cancels it:
        // non-finite deltas, vertical-dominant drift, or a lean toward the
        // opposite direction. Same-direction input just holds the arm; the
        // `Ended` fast path or the settle timer commits it.
        if matches!(event.touch_phase, TouchPhase::Moved)
            && self.swipe_arming.pending_direction().is_some()
            && (!dx_px.is_finite()
                || !dy_px.is_finite()
                || is_vertical_dominant_scroll(dx_px, dy_px)
                || swipe_direction_for_dx(dx_px, natural_scroll)
                    .is_some_and(|lean| Some(lean) != self.swipe_arming.pending_direction()))
        {
            self.cancel_pending_swipe(cx);
        }
        let crossed = self.swipe_scroll.push(dx_px, dy_px, event.touch_phase, natural_scroll);
        if self.swipe_scroll.armed() != before {
            cx.notify();
        }
        match event.touch_phase {
            // The terminal phases never carry a crossing (`push` resets
            // without firing), so they only resolve a pending arm: finger
            // lift commits, a fresh touch or cancellation fails closed.
            TouchPhase::Ended => self.commit_pending_swipe_current(cx),
            TouchPhase::Cancelled | TouchPhase::Started => self.cancel_pending_swipe(cx),
            TouchPhase::Moved => {
                if let Some(direction) = crossed.filter(|direction| self.swipe_direction_available(*direction)) {
                    self.arm_pending_swipe(direction, cx);
                }
            }
        }
    }

    /// Arms a swipe navigation toward `direction`, showing the window-level
    /// overlay until the commit (finger lift or settle timer) or a cancel.
    /// Same-direction input while armed is ignored; opposite input supersedes
    /// and restarts the timer.
    pub(super) fn arm_pending_swipe(&mut self, direction: NavigationDirection, cx: &mut Context<Self>) {
        let (outcome, generation) = self.swipe_arming.arm(direction, Instant::now());
        match outcome {
            ArmOutcome::Ignored => {}
            ArmOutcome::Armed | ArmOutcome::Superseded => {
                cx.notify();
                cx.spawn(async move |view, cx| {
                    cx.background_executor().timer(SWIPE_COMMIT_DELAY).await;
                    let _updated = view.update(cx, |view, cx| {
                        view.commit_pending_swipe(generation, cx);
                    });
                })
                .detach();
            }
        }
    }

    /// Drops a pending swipe arm, hiding the overlay. Redraws only when an
    /// arm was actually pending.
    pub(super) fn cancel_pending_swipe(&mut self, cx: &mut Context<Self>) {
        if self.swipe_arming.cancel() {
            cx.notify();
        }
    }

    /// Commits the arm held by `generation` (settle-timer path). Re-validates
    /// the overlay and history guards, so a stale arm can never fire into a
    /// blocked navigation; records the arm cooldown only when a navigation
    /// actually issues.
    pub(super) fn commit_pending_swipe(&mut self, generation: u64, cx: &mut Context<Self>) {
        let Some(direction) = self.swipe_arming.take_for_commit(generation) else {
            return;
        };
        if !self.swipe_navigation_allowed() || !self.swipe_direction_available(direction) {
            cx.notify();
            return;
        }
        // The cooldown starts only when a navigation actually issues: a
        // guard-passing commit can still fail (target thread deleted by a
        // list refresh mid-arm, queue full), and a failed commit must not
        // eat the user's retry.
        let issued = self.navigate_history_toward(direction, cx);
        if issued {
            self.swipe_arming.record_commit(Instant::now());
        }
        cx.notify();
    }

    /// Commits whatever arm is live now (finger-lift path).
    pub(super) fn commit_pending_swipe_current(&mut self, cx: &mut Context<Self>) {
        let generation = self.swipe_arming.generation();
        self.commit_pending_swipe(generation, cx);
    }

    /// Step back in session history. Retires any armed swipe first:
    /// toolbar, keyboard, and menu entries land here directly, bypassing
    /// the arm layer, so without this the stale arm's timer would fire
    /// into the moved cursor as a second navigation.
    pub(super) fn navigate_back(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_swipe(cx);
        let _issued = self.navigate_history_toward(NavigationDirection::Back, cx);
    }

    /// Step forward in session history. Retires any armed swipe first,
    /// like [`Self::navigate_back`].
    pub(super) fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        self.cancel_pending_swipe(cx);
        let _issued = self.navigate_history_toward(NavigationDirection::Forward, cx);
    }

    /// Gate and dispatch one history step toward `direction`. Shared by the
    /// direct entries above and the swipe commit (whose arm is already
    /// consumed, so it must not re-cancel). Returns whether a navigation
    /// issued; the commit starts its cooldown only on `true`.
    fn navigate_history_toward(&mut self, direction: NavigationDirection, cx: &mut Context<Self>) -> bool {
        let index = match direction {
            NavigationDirection::Back if self.can_navigate_back() => self.navigation_history.back_index(),
            NavigationDirection::Forward if self.can_navigate_forward() => self.navigation_history.forward_index(),
            _ => None,
        };
        let Some(index) = index else {
            return false;
        };
        self.navigate_to_history_index(index, cx)
    }

    /// Handle a `vtcode://` handoff link in the running app. Same workspace
    /// loads directly through the UI entry (running-turn confirm included),
    /// skipping the session-list wait; another workspace navigates through
    /// the pending path like back/forward.
    pub(super) fn handle_handoff_link(
        &mut self,
        link: protocol::handoff_url::HandoffLink,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = files::canonicalize_workspace_folder(&link.workspace) else {
            self.error = Some(rust_i18n::t!("app.workspace.invalid_folder").to_string());
            cx.notify();
            return;
        };
        self.notify_handoff_received(&link.session_id, window, cx);
        if self.workspace.as_ref() == Some(&workspace) {
            // False is normal here: the running-turn confirm opened, or one
            // is already up and the link yields to it.
            let _ = self.load_session_from_ui(link.session_id, window, cx);
        } else {
            self.arm_session_link(workspace, link.session_id, cx);
        }
    }

    /// Announce an incoming CLI handoff. The notification is the only
    /// surface that tells the user a thread arrived while the window was in
    /// the background, and its click opens the handed-off thread.
    fn notify_handoff_received(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !super::super::notifications::handoff_allowed(&self.preferences.notifications) {
            return;
        }
        let system = !window.is_window_active();
        let weak = cx.entity().downgrade();
        super::super::notifications::handoff_received(
            window,
            system,
            deeplinks::click_handler(deeplinks::NotificationTarget::Thread { session_id: session_id.to_owned() }, weak),
            cx,
        );
    }

    /// Arm a CLI session handoff (`/desktop` argv or `vtcode://` link):
    /// record the link target in history, select its workspace, and let the
    /// session-list continuation load the thread — the same path as
    /// back/forward workspace navigation.
    pub(super) fn arm_session_link(&mut self, workspace: PathBuf, session_id: String, cx: &mut Context<Self>) {
        // Handoff links rewrite history like any other record: retire an armed
        // swipe first, or a fast-failing select would leave it advertising a
        // target the rewritten cursor no longer points at.
        self.cancel_pending_swipe(cx);
        self.navigation_history.record(NavigationLocation {
            workspace: workspace.clone(),
            session_id: Some(session_id),
        });
        let Some(index) = self.navigation_history.current_index() else {
            return;
        };
        self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: true });
        self.select_workspace(workspace.clone(), cx);
        if self.workspace.as_ref() != Some(&workspace)
            && self.pending_workspace.as_ref() != Some(&workspace)
            && !self.workspace_selection_pending
        {
            self.pending_navigation = None;
        }
    }

    /// Drive the cursor or session load toward `index`. Returns whether a
    /// navigation issued: the swipe commit starts its cooldown only on
    /// `true`, so every silent failure below must return `false`.
    fn navigate_to_history_index(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(target) = self.navigation_history.location(index).cloned() else {
            return false;
        };
        let workspace_switch = self.workspace.as_ref() != Some(&target.workspace);
        if workspace_switch {
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: true });
            self.select_workspace(target.workspace.clone(), cx);
            if self.workspace.as_ref() != Some(&target.workspace)
                && self.pending_workspace.as_ref() != Some(&target.workspace)
                && !self.workspace_selection_pending
            {
                self.pending_navigation = None;
                return false;
            }
            return true;
        }

        if target.session_id == self.selected_session_id {
            let _moved = self.navigation_history.navigate_to(index);
            cx.notify();
            return true;
        }

        if let Some(session_id) = target.session_id {
            if !self.sessions.iter().any(|session| session.session_id == session_id) {
                self.error = Some(rust_i18n::t!("app.workspace.thread_unavailable").to_string());
                cx.notify();
                return false;
            }
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: false });
            if !self.request_session_change(protocol::method::SESSION_LOAD, json!({ keys::SESSION_ID: session_id }), cx)
            {
                self.pending_navigation = None;
                return false;
            }
            true
        } else {
            let Some(session_id) = self.selected_session_id.clone() else {
                return false;
            };
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: false });
            if !self.request_session_change(
                protocol::method::SESSION_CLOSE,
                json!({ keys::SESSION_ID: session_id }),
                cx,
            ) {
                self.pending_navigation = None;
                return false;
            }
            true
        }
    }

    pub(super) fn continue_pending_workspace_navigation(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_navigation.filter(|pending| pending.workspace_switch) else {
            return;
        };
        let Some(target) = self.navigation_history.location(pending.target_index).cloned() else {
            self.fail_pending_navigation();
            return;
        };
        if self.workspace_selection_pending || self.pending_workspace.is_some() {
            // The switch itself is still in flight; its completion
            // retries. Judging the workspace now would fail a handoff
            // on a stale pre-switch value.
            return;
        }
        if self.workspace.as_ref() != Some(&target.workspace) {
            self.fail_pending_navigation();
            return;
        };
        if !self.session_list_received {
            // The list is still in flight; its arrival retries. Judging
            // membership now would fail a handoff the list would satisfy.
            return;
        }
        let Some(session_id) = target.session_id else {
            self.pending_navigation = None;
            let _moved = self.navigation_history.navigate_to(pending.target_index);
            self.status = rust_i18n::t!("app.workspace.workspace_ready").to_string();
            return;
        };
        if !self.sessions.iter().any(|session| session.session_id == session_id) {
            self.error = Some(rust_i18n::t!("app.workspace.thread_unavailable").to_string());
            self.fail_pending_navigation();
            return;
        }
        let load_sent =
            self.request_session_change(protocol::method::SESSION_LOAD, json!({ keys::SESSION_ID: session_id }), cx);
        if !load_sent && self.session_request.is_none() {
            // Durable block (running turn, retained draft, open
            // interaction): fail fast like before. A held gate means a
            // response is coming that will retry the load.
            self.fail_pending_navigation();
        }
    }

    pub(super) fn fail_pending_navigation(&mut self) {
        let Some(pending) = self.pending_navigation.take() else {
            return;
        };
        if pending.workspace_switch
            && let Some(location) = self.current_navigation_location()
        {
            let _replaced = self.navigation_history.replace_and_navigate_to(pending.target_index, location);
        }
    }

    pub(super) fn select_workspace_from_ui(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.close_active_route(window, cx);
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
        self.restore_focus_after_review_close(window, cx);
    }

    pub(super) fn select_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
    }

    pub(super) fn select_workspace_on_startup(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, false, cx);
    }

    fn select_workspace_with_invalid_path_feedback(
        &mut self,
        path: PathBuf,
        show_invalid_path_error: bool,
        cx: &mut Context<Self>,
    ) {
        // A turn may have started while the asynchronous picker was open.
        if self.retained_submission.is_some() {
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            cx.notify();
            return;
        }
        self.workspace_selection_pending = true;
        self.error = None;
        self.workspace_selection_task = Some(cx.spawn(async move |view, cx| {
            let canonical = cx
                .background_spawn(async move { files::canonicalize_workspace_folder(&path) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                view.workspace_selection_pending = false;
                match canonical {
                    Some(canonical) => view.select_canonical_workspace(canonical, cx),
                    None => {
                        view.fail_pending_navigation();
                        if show_invalid_path_error {
                            view.error = Some(rust_i18n::t!("app.workspace.invalid_folder").to_string());
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_canonical_workspace(&mut self, canonical: PathBuf, cx: &mut Context<Self>) {
        // Revalidate because a turn or session transition may start while the path is checked.
        if self.retained_submission.is_some() {
            self.fail_pending_navigation();
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            return;
        }
        if !self.workspace_switch_idle() {
            self.fail_pending_navigation();
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            return;
        }
        if self.workspace.as_ref() == Some(&canonical) {
            self.continue_pending_workspace_navigation(cx);
            return;
        }
        if let Some(backend) = self.backend.as_ref() {
            self.pending_workspace = Some(canonical);
            self.status = rust_i18n::t!("app.workspace.shutting_down").to_string();
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.pending_workspace = None;
                self.fail_pending_navigation();
                self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
            }
            cx.notify();
            return;
        }
        self.start_workspace(canonical, cx);
    }

    pub(super) fn start_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        self.prepare_workspace(&workspace, cx);
        match launch_backend(workspace, self.event_tx.clone()) {
            Ok(backend) => self.initialize_backend(backend),
            Err(error) => self.report_service_launch_failure(error),
        }
        cx.notify();
    }

    pub(super) fn report_service_launch_failure(&mut self, error: String) {
        self.fail_pending_navigation();
        self.status = rust_i18n::t!("app.workspace.connect_failed").to_string();
        self.error = Some(format!("{error}\n\n{}", service_reconnect_guidance(self.retained_submission.is_some())));
    }

    pub(super) fn prepare_workspace(&mut self, workspace: &Path, cx: &mut Context<Self>) {
        // A same-workspace restart reloads CLI configuration without losing draft sources.
        let same_workspace = self.workspace.as_deref() == Some(workspace);
        if !same_workspace {
            self.terminal = None;
        }
        self.reset_webmcp_state(None);
        let draft_sources = same_workspace.then(|| std::mem::take(&mut self.attachments));
        self.workspace = Some(workspace.to_path_buf());
        self.remember_workspace(workspace);
        self.pending_workspace = None;
        self.sessions.clear();
        self.session_list_received = false;
        self.session_list_has_older = false;
        self.selected_session_id = None;
        self.reset_session_state();
        if self.pending_navigation.is_none() {
            self.record_current_navigation();
        }
        if let Some(sources) = draft_sources {
            self.attachments = sources;
        }
        self.configuration = None;
        self.cli_config_settings = None;
        self.config_catalog_cache = None;
        self.config_search_corpus = None;
        self.config_editor = None;
        self.config_editor_error = None;
        self.config_write_pending = false;
        if !same_workspace {
            self.config_target = protocol::ConfigTarget::Workspace;
            self.config_write_notice = None;
            // A new workspace starts paged; same-workspace restarts keep the
            // expanded depth so refreshes never prune tabs past the first page.
            self.session_list_limit = SESSION_LIST_PAGE_SIZE;
        }
        self.service_info = None;
        self.pending_permission_choice = None;
        self.composer_resources.clear();
        self.slash_commands.clear();
        self.resources_loading = false;
        self.resources_loaded = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        // Explicit though reset_session_state above already interrupts: the
        // armed wait must never survive a workspace start by ordering luck.
        self.interrupt_armed_running_switch();
        self.running = false;
        self.error = None;
        self.status = rust_i18n::t!("app.workspace.starting_service").to_string();
        if !same_workspace {
            self.files.clear();
            self.files_truncated = false;
            self.file_directory_paths = Arc::new(HashSet::new());
            self.expanded_file_directories.clear();
            self.expanded_picker_directories.clear();
            self.file_tree.update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
            // The rescan rebuilds both trees; until it lands, rows, queries,
            // and counts must read empty rather than stale. Filter text
            // survives: it needs no window to keep, and the rebuild applies
            // it to the new workspace.
            self.file_search_hit_count = None;
            self.picker_tree.update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
            self.picker_search_hit_count = None;
        }
        self.git_branch = None;
        self.preview_task = None;
        self.external_launch_task = None;
        self.refresh_files(cx);
        self.file_preview = None;
        self.file_rename = None;
        self.rename_return_focus = None;
        self.models.clear();
    }

    pub(super) fn initialize_backend(&mut self, backend: BackendHandle) {
        let initialize = protocol::InitializeParams {
            protocol: protocol::ProtocolRange {
                minimum: protocol::PROTOCOL_VERSION,
                maximum: protocol::PROTOCOL_VERSION,
            },
            client_name: "VT Code Desktop".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        if send_request(&backend.commands, protocol::method::INITIALIZE, json!(initialize)) {
            self.backend = Some(backend);
            // Reuse the session transition gate until the handshake is validated.
            self.session_request = Some(protocol::method::INITIALIZE);
        } else {
            self.report_service_launch_failure(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
    }
}
